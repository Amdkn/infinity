//! Smoke D-N051 F8 pas 3 — hibernation + RAP + MCP + localhost + gel.
//!
//! Pure-adapter checks (no network, no daemon process): the state
//! machine, the RAP outbox, the localhost policy, the gel lineage and
//! the pinned upstream surface. The live MCP/RAP proofs live in
//! `mcp_rap_proof.rs` and in the upstream daemon tests.

use aspace_infinity_adapter::{
    AspaceSessionEntry, DAEMON_READY_LINE, FRACTAL_LAYER, GEL_COMMIT_SHORT, GoldfishSchedule,
    INFINITY_POPULATION_CAP, LocalhostPolicy, RapDelivery, RapOutbox, SessionState,
    WS_DEFAULT_BIND, check_upstream_surface, verify_gel_lineage,
};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bug: adapter must live at the repo root")
        .to_owned()
}

#[test]
fn smoke_hibernation_idle_then_shut_down_wake_on_user_text_only() {
    let mut entry = AspaceSessionEntry::new(PathBuf::from("/tmp"));
    assert_eq!(entry.status(), SessionState::Running);
    entry.mark_idle();
    assert_eq!(entry.status(), SessionState::Idle);
    assert!(!entry.is_hibernating(), "idle is not hibernation yet");
    entry.mark_shut_down();
    // Upstream precedence: `idle` is checked before `shut_down`, so the
    // status still reads Idle — but the session IS hibernating.
    assert_eq!(entry.status(), SessionState::Idle);
    assert!(entry.is_hibernating(), "pure data between turns");
    // Only user text wakes: nothing else calls wake_on_user_text.
    assert!(entry.wake_on_user_text());
    assert_eq!(entry.status(), SessionState::Running);
    assert!(!entry.wake_on_user_text(), "already awake");
    // Archived sessions never wake.
    let mut archived = AspaceSessionEntry::new(PathBuf::from("/tmp"));
    archived.archived = true;
    archived.mark_shut_down();
    assert_eq!(archived.status(), SessionState::Archived);
    assert!(!archived.wake_on_user_text());
}

#[test]
fn smoke_rap_delivers_to_agent_without_process() {
    let mut outbox = RapOutbox::new();
    // No process hosts "t-1": the result still lands, and waits.
    outbox.deliver(RapDelivery {
        thread_id: "t-1".into(),
        group_id: "g1".into(),
        invocation_id: "inv-1".into(),
        text: "tool result: echo".into(),
    });
    assert_eq!(outbox.pending_count("t-1"), 1);
    let mut entry = AspaceSessionEntry::new(PathBuf::from("/tmp"));
    entry.mark_idle();
    entry.mark_shut_down();
    assert!(entry.is_hibernating());
    // Wake on user text, then collect the waiting result.
    assert!(entry.wake_on_user_text());
    let got = outbox.collect("t-1");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].text, "tool result: echo");
    assert_eq!(outbox.pending_count("t-1"), 0);
}

#[test]
fn smoke_localhost_only() {
    for host in ["127.0.0.1", "127.0.0.2", "::1", "localhost"] {
        assert!(
            LocalhostPolicy::is_loopback(host),
            "{host} must be loopback"
        );
        assert!(LocalhostPolicy::require_loopback(host).is_ok());
    }
    for host in ["0.0.0.0", "192.168.1.10", "10.0.0.1", "example.com"] {
        assert!(
            !LocalhostPolicy::is_loopback(host),
            "{host} is not loopback"
        );
        assert!(
            LocalhostPolicy::require_loopback(host).is_err(),
            "{host} must be refused"
        );
    }
    assert_eq!(WS_DEFAULT_BIND, "127.0.0.1");
    assert_eq!(DAEMON_READY_LINE, "infinity-daemon ready");
}

#[test]
fn smoke_cap_is_constant_and_fractal_layer_is_4l() {
    assert_eq!(INFINITY_POPULATION_CAP, 75_000);
    assert_eq!(FRACTAL_LAYER, "4L");
    let s = GoldfishSchedule::new("holon-1");
    assert_eq!(s.fractal_layer, "4L", "never the scheduler 8L default");
    assert_eq!(s.cap, 75_000);
    // The cap is a constant, not a measurement: nothing here spawns 75k agents.
}

#[test]
fn smoke_gel_lineage_adapter_only() {
    let root = repo_root();
    let lineage = verify_gel_lineage(&root).expect("gel lineage must hold");
    assert_eq!(lineage.gel_short(), GEL_COMMIT_SHORT);
    assert!(
        lineage.gel_is_ancestor_of_head,
        "H-M1: gel is HEAD ancestor"
    );
    assert!(
        lineage.non_adapter_files.is_empty(),
        "H-M2: non-adapter files changed: {:?}",
        lineage.non_adapter_files
    );
    assert!(
        lineage
            .changed_files
            .iter()
            .all(|f| f.starts_with("aspace-infinity-adapter/")),
        "diff is adapter-only: {:?}",
        lineage.changed_files
    );
}

#[test]
fn smoke_upstream_surface_pinned_at_gel() {
    let root = repo_root();
    let checks = check_upstream_surface(&root);
    let missing: Vec<&str> = checks
        .iter()
        .filter(|c| !c.found)
        .map(|c| c.name.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "pinned upstream surface moved at the gel: {missing:?} (amend the design, never silently re-pin)"
    );
    assert_eq!(checks.len(), 10, "pin count must not drift silently");
}
