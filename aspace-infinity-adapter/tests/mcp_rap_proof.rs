//! Live MCP / RAP proofs — D-N051 F8 pas 3, H-P3-4 / H-P3-6 / H-P3-8.
//!
//! 1. MCP proxy: `tools/list` round-trips through the upstream proxy
//!    (`mcp_proxy::start_proxy_server`) and the RAP callback channel —
//!    the same shape as the upstream `mcp_proxy_callback.rs` test, run
//!    from the adapter.
//! 2. Real RAP servers, unchanged: the two stub servers of
//!    `rap-test-servers` (`start_stub_image_server`, `start_stub_diff_server`)
//!    are started as-is, their manifest is fetched, an invocation is
//!    POSTed, and the tool result arrives on the RAP callback URL —
//!    delivered to an agent that has no process (H-P3-4).

use async_trait::async_trait;
use infinity_daemon::mcp_proxy::start_proxy_server;
use infinity_mcp_bridge::{BoxError, McpTransport, McpTransportFactory};
use rap_client::callback_server::start_callback_channel;
use rap_protocol::{DisplaySegment, RapCallback, RapInvocation};
use rap_test_servers::{start_stub_diff_server, start_stub_image_server};
use std::sync::Arc;

struct MockMcpTransport;

#[async_trait]
impl McpTransport for MockMcpTransport {
    async fn request(
        &mut self,
        method: &str,
        _params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, BoxError> {
        match method {
            "tools/list" => Ok(serde_json::json!({
                "tools": [
                    {"name": "echo", "description": "Echoes input", "inputSchema": {"type": "object"}}
                ]
            })),
            _ => Err(format!("unexpected method: {method}").into()),
        }
    }
}

fn mock_factory() -> McpTransportFactory {
    Arc::new(|| Box::pin(async { Ok(Box::new(MockMcpTransport) as Box<dyn McpTransport>) }))
}

#[tokio::test]
async fn mcp_proxy_list_tools_callback_round_trips() {
    let _ = tracing_subscriber::fmt::try_init();
    let (port, _task) = start_proxy_server("mock".to_owned(), mock_factory())
        .await
        .expect("start proxy server");
    let proxy_url = format!("http://127.0.0.1:{port}");
    assert!(
        proxy_url.starts_with("http://127.0.0.1:"),
        "proxy binds loopback only (D6)"
    );

    let (callback_url, mut rx) = start_callback_channel()
        .await
        .expect("start callback channel");
    assert!(
        callback_url.starts_with("http://127.0.0.1:"),
        "callback channel is loopback only: {callback_url}"
    );

    let inv = RapInvocation {
        operation: "mock_list_tools".to_owned(),
        arguments: serde_json::json!({}),
        id: "test-1".into(),
        call_id: None,
        callback_url,
        group_id: "g1".into(),
        user_id: None,
        thread_ancestors: None,
    };
    reqwest::Client::new()
        .post(&proxy_url)
        .json(&inv)
        .send()
        .await
        .expect("send invocation to proxy");

    let cb = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
        .await
        .expect("timed out waiting for the RAP callback")
        .expect("callback channel closed");
    match cb {
        RapCallback::ToolResult(tr) => {
            let text = tr.text.as_deref().unwrap_or_default();
            assert!(
                text.contains("echo"),
                "tool list must mention 'echo': {text}"
            );
            let segments = tr.display_as.expect("display_as should be Some");
            assert!(matches!(segments[0], DisplaySegment::Text(_)));
        }
        other => panic!("expected ToolResult, got: {other:?}"),
    }
}

async fn invoke_real_rap_server(
    server_port: u16,
    operation: &str,
    arguments: serde_json::Value,
) -> RapCallback {
    let base = format!("http://127.0.0.1:{server_port}");
    // The manifest is served unchanged by the stub server.
    let manifest: serde_json::Value = reqwest::Client::new()
        .get(format!("{base}/.well-known/rap-toolset"))
        .send()
        .await
        .expect("fetch RAP manifest")
        .json()
        .await
        .expect("parse RAP manifest");
    let endpoint = manifest["endpoint"]
        .as_str()
        .expect("manifest must carry an endpoint")
        .to_owned();
    assert!(
        endpoint.starts_with("http://127.0.0.1:"),
        "stub server endpoint is loopback only: {endpoint}"
    );

    let (callback_url, mut rx) = start_callback_channel()
        .await
        .expect("start callback channel");
    let inv = RapInvocation {
        operation: operation.to_owned(),
        arguments,
        id: format!("adapter-{operation}").into(),
        call_id: None,
        callback_url,
        group_id: "g-adapter".into(),
        user_id: None,
        thread_ancestors: None,
    };
    reqwest::Client::new()
        .post(&endpoint)
        .json(&inv)
        .send()
        .await
        .expect("send invocation to the real RAP server");
    tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
        .await
        .expect("timed out waiting for the real server callback")
        .expect("callback channel closed")
}

#[tokio::test]
async fn real_rap_image_server_runs_unchanged() {
    let _ = tracing_subscriber::fmt::try_init();
    let port = start_stub_image_server()
        .await
        .expect("start stub image server");
    let cb = invoke_real_rap_server(
        port,
        "read_image",
        serde_json::json!({"path": "src/lib.rs"}),
    )
    .await;
    match cb {
        RapCallback::ToolResult(tr) => {
            // The stub answers with structured content (text + image) and
            // an image display segment with a text fallback.
            let content = tr.content.expect("image tool must carry content");
            assert_eq!(content.len(), 2, "text + image content items");
            let display = tr.display_as.expect("display_as should be Some");
            assert!(
                matches!(display[0], DisplaySegment::Image(_)),
                "first display segment must be the image"
            );
        }
        other => panic!("expected ToolResult, got: {other:?}"),
    }
}

#[tokio::test]
async fn real_rap_diff_server_runs_unchanged() {
    let _ = tracing_subscriber::fmt::try_init();
    let port = start_stub_diff_server()
        .await
        .expect("start stub diff server");
    let cb = invoke_real_rap_server(
        port,
        "edit_file",
        serde_json::json!({"path": "src/main.rs"}),
    )
    .await;
    match cb {
        RapCallback::ToolResult(tr) => {
            let text = tr.text.as_deref().unwrap_or_default();
            assert!(
                text.contains("Replaced text in src/main.rs"),
                "diff tool must confirm the edit: {text}"
            );
            let display = tr.display_as.expect("display_as should be Some");
            assert!(
                matches!(display[0], DisplaySegment::Diff(_)),
                "first display segment must be the diff"
            );
        }
        other => panic!("expected ToolResult, got: {other:?}"),
    }
}
