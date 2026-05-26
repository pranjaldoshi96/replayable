//! Integration smoke tests for replayable-proxy.

#[test]
fn version_is_nonempty() {
    let v = replayable_proxy::version();
    assert!(!v.is_empty(), "version must not be empty");
}

#[test]
fn version_matches_cargo() {
    assert_eq!(replayable_proxy::version(), env!("CARGO_PKG_VERSION"));
}
