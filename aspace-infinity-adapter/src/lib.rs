//! A'Space adapter for `hydro-project/infinity` — D-N051 F8 pas 3.
//!
//! The adapter pins the upstream surface at the gel (`e1a3e4b`) without
//! touching any upstream file (H-M2: the diff of the fork is this directory
//! only). It carries:
//!
//! * the hibernation state machine (`SessionEntry`: `idle` -> `shut_down`,
//!   wake on user text input only) — H-P3-3;
//! * the RAP delivery outbox: a tool result is delivered to an agent even
//!   when that agent currently has no process at all — H-P3-4;
//! * the MCP-proxy / RAP surface pins (verified against the upstream
//!   sources at the gel, see [`check_upstream_surface`]) — H-P3-6;
//! * the localhost-only network policy (127.0.0.1 / ::1, never 0.0.0.0)
//!   — D6 / H-P3-8;
//! * the population cap (`75_000`) carried as a **constant, not a
//!   guarantee** — D8 — and the `fractal_layer` passed explicitly as
//!   `"4L"` (the plugin is authoritative; the bind scheduler's `8L`
//!   default is never left to apply) — D2.
//!
//! Telemetry: zero by default. The adapter performs no network call of its
//! own; every outbound call of the fork is named in
//! `infinity-TELEMETRY-AUDIT.md` (evidence `D-N051/p3/`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// A'Space fiber address of this module.
pub const FIBER_ID: &str = "fiber:a0:infinity";
/// Frozen upstream commit (the gel, D1). Never re-frozen without a design amendment.
pub const GEL_COMMIT: &str = "e1a3e4b36e5f6143e03a59a7ad86e1abacfd22e8";
/// Short form of the gel, as used by the register.
pub const GEL_COMMIT_SHORT: &str = "e1a3e4b";
/// Adapter version.
pub const ADAPTER_VERSION: &str = "0.1.0";
/// Population cap, carried as a constant (D8). **Not measured, not
/// guaranteed**: it is the scheduler cap from the bind / README claim
/// ("75k agents in the memory of a Raspberry Pi"), not a load test.
pub const INFINITY_POPULATION_CAP: u32 = 75_000;
/// Fractal layer the plugin declares authoritative (4L). The adapter
/// passes it explicitly everywhere; the bind scheduler's `8L` default
/// never applies (D2 — the 4L/8L mismatch is cosmetic, F8 D6).
pub const FRACTAL_LAYER: &str = "4L";
/// The daemon's only stdout line, printed once it is serving.
pub const DAEMON_READY_LINE: &str = "infinity-daemon ready";
/// Default WebSocket bind of the upstream daemon (lib.rs): loopback only.
pub const WS_DEFAULT_BIND: &str = "127.0.0.1";
/// Default WebSocket port of the upstream daemon.
pub const WS_DEFAULT_PORT: u16 = 8080;

/// Hibernation state of one agent session, mirroring upstream
/// `SessionEntry` (`crates/infinity-daemon/src/session_store.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Active work in progress.
    Running,
    /// No active work, but still resident.
    Idle,
    /// Hibernated: pure data between turns, no task, no stack, no open
    /// connection. Only user text input re-awakens the agent.
    ShutDown,
    /// Migrated away; must not be displayed.
    Archived,
}

/// A'Space mirror of upstream `SessionEntry` (same four fields, same
/// serde shape, so a stored session round-trips through either side).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AspaceSessionEntry {
    pub cwd: PathBuf,
    /// When true, only user text input should re-awaken the agent.
    #[serde(default)]
    pub shut_down: bool,
    /// When true, the agent is idle (no active work).
    #[serde(default)]
    pub idle: bool,
    /// When true, the session has been migrated away and should not be displayed.
    #[serde(default)]
    pub archived: bool,
}

impl AspaceSessionEntry {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            shut_down: false,
            idle: false,
            archived: false,
        }
    }

    /// Status following the upstream precedence: archived > idle >
    /// shut_down > running (upstream `SessionEntry::status`, minus the
    /// pending-choices branch which lives in the state store).
    pub fn status(&self) -> SessionState {
        if self.archived {
            SessionState::Archived
        } else if self.idle {
            SessionState::Idle
        } else if self.shut_down {
            SessionState::ShutDown
        } else {
            SessionState::Running
        }
    }

    /// The agent is pure data between turns: hibernating, costing nothing
    /// until the next event arrives.
    pub fn is_hibernating(&self) -> bool {
        self.shut_down && !self.archived
    }

    /// Idle first, then shut down: the two-step hibernation (H-P3-3).
    pub fn mark_idle(&mut self) {
        self.idle = true;
    }

    pub fn mark_shut_down(&mut self) {
        self.shut_down = true;
    }

    /// Wake rule, verbatim from the upstream contract: **only user text
    /// input re-awakens the agent**. Any other event (tool result, view
    /// update, heartbeat tick) leaves a hibernated session asleep and is
    /// queued instead. Returns `true` iff the session was woken.
    pub fn wake_on_user_text(&mut self) -> bool {
        if self.shut_down && !self.archived {
            self.shut_down = false;
            self.idle = false;
            true
        } else {
            false
        }
    }
}

/// A RAP tool result held for an agent that currently has no process at
/// all (H-P3-4). Delivery never requires the agent to be resident: the
/// result waits in the outbox and is collected when the agent wakes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RapDelivery {
    pub thread_id: String,
    pub group_id: String,
    pub invocation_id: String,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct RapOutbox {
    pending: HashMap<String, Vec<RapDelivery>>,
}

impl RapOutbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Deliver a result to `thread_id`, whether or not any process is
    /// currently hosting that agent.
    pub fn deliver(&mut self, delivery: RapDelivery) {
        self.pending
            .entry(delivery.thread_id.clone())
            .or_default()
            .push(delivery);
    }

    /// Collect everything waiting for `thread_id` (called on wake).
    pub fn collect(&mut self, thread_id: &str) -> Vec<RapDelivery> {
        self.pending.remove(thread_id).unwrap_or_default()
    }

    pub fn pending_count(&self, thread_id: &str) -> usize {
        self.pending.get(thread_id).map_or(0, Vec::len)
    }
}

/// Network policy of the adapter (D6): the daemon is local-first. Only
/// loopback binds are allowed; `0.0.0.0` or any routable address is
/// refused before any listener is created.
pub struct LocalhostPolicy;

impl LocalhostPolicy {
    /// `true` iff `host` is a loopback address (IPv4 `127.0.0.0/8` or
    /// IPv6 `::1` / `localhost`).
    pub fn is_loopback(host: &str) -> bool {
        let host = host.trim().trim_matches(['[', ']']);
        host == "localhost"
            || host == "::1"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    }

    /// Refuse anything that is not loopback. Returns `Ok(())` for
    /// loopback, `Err(reason)` otherwise.
    pub fn require_loopback(host: &str) -> Result<(), String> {
        if Self::is_loopback(host) {
            Ok(())
        } else {
            Err(format!(
                "refused non-loopback bind {host:?}: the infinity daemon is localhost-only (D6)"
            ))
        }
    }
}

/// A goldfish scheduling request. The adapter always passes
/// `fractal_layer` explicitly as `"4L"` (the plugin is authoritative);
/// callers cannot leave the bind scheduler's `8L` default to apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldfishSchedule {
    pub holon_id: String,
    pub fractal_layer: String,
    pub cap: u32,
}

impl GoldfishSchedule {
    pub fn new(holon_id: impl Into<String>) -> Self {
        Self {
            holon_id: holon_id.into(),
            fractal_layer: FRACTAL_LAYER.to_owned(),
            cap: INFINITY_POPULATION_CAP,
        }
    }
}

/// Result of the gel-lineage check (H-M1 / H-M2, local mirror of the API
/// compare).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GelLineage {
    pub gel: String,
    pub head: String,
    pub gel_is_ancestor_of_head: bool,
    pub changed_files: Vec<String>,
    pub non_adapter_files: Vec<String>,
}

impl GelLineage {
    /// Short form of the gel commit (first 7 hex chars).
    pub fn gel_short(&self) -> &str {
        self.gel.get(..7).unwrap_or(&self.gel)
    }
}

/// Verify, from the local git history, that `HEAD` descends from the gel
/// and that every file changed since the gel lives under
/// `aspace-infinity-adapter/` (H-M1/H-M2). Fails closed: any git error, a
/// gel that is not an ancestor, or a single non-adapter file is an error.
pub fn verify_gel_lineage(repo_root: &Path) -> Result<GelLineage, String> {
    let git = |args: &[&str]| -> Result<String, String> {
        let out = Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .output()
            .map_err(|e| format!("git {} failed to spawn: {e}", args.join(" ")))?;
        if !out.status.success() {
            return Err(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    let head = git(&["rev-parse", "HEAD"])?;
    let ancestor = Command::new("git")
        .args(["merge-base", "--is-ancestor", GEL_COMMIT, "HEAD"])
        .current_dir(repo_root)
        .status()
        .map_err(|e| format!("git merge-base failed to spawn: {e}"))?;
    let gel_is_ancestor_of_head = ancestor.success();
    let diff = git(&["diff", "--name-only", GEL_COMMIT, "HEAD"])?;
    let changed_files: Vec<String> = diff.lines().map(str::to_owned).collect();
    let non_adapter_files: Vec<String> = changed_files
        .iter()
        .filter(|f| !f.starts_with("aspace-infinity-adapter/"))
        .cloned()
        .collect();
    if !gel_is_ancestor_of_head {
        return Err(format!(
            "gel {GEL_COMMIT} is not an ancestor of HEAD {head} (H-M1 failed)"
        ));
    }
    if !non_adapter_files.is_empty() {
        return Err(format!(
            "non-adapter files changed since the gel (H-M2 failed): {}",
            non_adapter_files.join(", ")
        ));
    }
    Ok(GelLineage {
        gel: GEL_COMMIT.to_owned(),
        head,
        gel_is_ancestor_of_head,
        changed_files,
        non_adapter_files,
    })
}

/// One pinned fact of the upstream surface, re-checked against the gel
/// sources on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinCheck {
    pub name: String,
    pub file: String,
    pub needle: String,
    pub found: bool,
}

fn pin_contains(repo_root: &Path, name: &str, rel: &str, needle: &str) -> PinCheck {
    let path = repo_root.join(rel);
    let found = std::fs::read_to_string(&path)
        .map(|s| s.contains(needle))
        .unwrap_or(false);
    PinCheck {
        name: name.to_owned(),
        file: rel.to_owned(),
        needle: needle.to_owned(),
        found,
    }
}

/// Re-check the pinned upstream surface against the gel sources (H-M1 /
/// H-P3-3 / H-P3-6 / H-P3-8). Every check must be `found`; a missing pin
/// means the gel moved under the adapter and the design must be amended
/// (never silently re-pinned).
pub fn check_upstream_surface(repo_root: &Path) -> Vec<PinCheck> {
    vec![
        pin_contains(
            repo_root,
            "session_entry.fields",
            "crates/infinity-daemon/src/session_store.rs",
            "pub shut_down: bool",
        ),
        pin_contains(
            repo_root,
            "session_entry.wake_rule",
            "crates/infinity-daemon/src/session_store.rs",
            "only user text input should re-awaken the agent",
        ),
        pin_contains(
            repo_root,
            "session_entry.idle",
            "crates/infinity-daemon/src/session_store.rs",
            "pub idle: bool",
        ),
        pin_contains(
            repo_root,
            "daemon.ready_line",
            "crates/infinity-daemon/src/lib.rs",
            "infinity-daemon ready",
        ),
        pin_contains(
            repo_root,
            "daemon.ws_loopback_default",
            "crates/infinity-daemon/src/lib.rs",
            "\"127.0.0.1\"",
        ),
        pin_contains(
            repo_root,
            "mcp_proxy.loopback_bind",
            "crates/infinity-daemon/src/mcp_proxy.rs",
            "TcpListener::bind(\"127.0.0.1:0\")",
        ),
        pin_contains(
            repo_root,
            "rap_callback.serve",
            "crates/infinity-daemon/src/rap_callback.rs",
            "pub fn serve_callbacks",
        ),
        pin_contains(
            repo_root,
            "memory_store.persistent_conversation",
            "crates/infinity-daemon/src/memory_store.rs",
            "PersistentConversationStore",
        ),
        pin_contains(repo_root, "readme.cap_claim", "README.md", "75k agents"),
        pin_contains(
            repo_root,
            "workspace.daemon_member",
            "Cargo.toml",
            "crates/infinity-daemon",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hibernation_two_step_and_wake_on_user_text_only() {
        let mut entry = AspaceSessionEntry::new(PathBuf::from("/tmp"));
        assert_eq!(entry.status(), SessionState::Running);
        entry.mark_idle();
        assert_eq!(entry.status(), SessionState::Idle);
        assert!(!entry.is_hibernating());
        entry.mark_shut_down();
        // Upstream precedence (session_store.rs): `idle` is checked
        // before `shut_down`, so the status still reads Idle — but the
        // session IS hibernating (the shut_down flag is set).
        assert_eq!(entry.status(), SessionState::Idle);
        assert!(entry.is_hibernating());
        // Only user text wakes: a hibernated entry stays asleep until
        // wake_on_user_text is called (no other event wakes it).
        assert!(entry.wake_on_user_text());
        assert_eq!(entry.status(), SessionState::Running);
        assert!(!entry.wake_on_user_text());
    }

    #[test]
    fn rap_outbox_delivers_without_process() {
        let mut outbox = RapOutbox::new();
        outbox.deliver(RapDelivery {
            thread_id: "t-1".into(),
            group_id: "g1".into(),
            invocation_id: "inv-1".into(),
            text: "echo".into(),
        });
        assert_eq!(outbox.pending_count("t-1"), 1);
        let got = outbox.collect("t-1");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "echo");
        assert_eq!(outbox.pending_count("t-1"), 0);
    }

    #[test]
    fn localhost_policy_refuses_wildcard() {
        assert!(LocalhostPolicy::is_loopback("127.0.0.1"));
        assert!(LocalhostPolicy::is_loopback("127.0.0.2"));
        assert!(LocalhostPolicy::is_loopback("::1"));
        assert!(LocalhostPolicy::is_loopback("localhost"));
        assert!(!LocalhostPolicy::is_loopback("0.0.0.0"));
        assert!(!LocalhostPolicy::is_loopback("192.168.1.10"));
        assert!(LocalhostPolicy::require_loopback("127.0.0.1").is_ok());
        assert!(LocalhostPolicy::require_loopback("0.0.0.0").is_err());
    }

    #[test]
    fn goldfish_schedule_passes_4l_explicitly() {
        let s = GoldfishSchedule::new("holon-1");
        assert_eq!(s.fractal_layer, "4L");
        assert_eq!(s.cap, 75_000);
    }
}
