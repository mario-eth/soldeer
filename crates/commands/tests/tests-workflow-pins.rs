//! Ensure the Rust CI workflow only references pinned, immutable actions and toolchains.

#[test]
fn test_rust_workflow_uses_pinned_actions_and_toolchain() {
    let workflow = include_str!("../../../.github/workflows/rust.yml");
    let uses = workflow.lines().filter_map(|line| {
        line.trim().strip_prefix("- uses: ").or_else(|| line.trim().strip_prefix("uses: "))
    });
    let mut count = 0;
    for action in uses {
        count += 1;
        let reference = action.rsplit_once('@').expect("action should have a version ref").1;
        assert_eq!(reference.len(), 40, "workflow action is not pinned to a commit: {action}");
        assert!(
            reference.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "workflow action is not pinned to a commit: {action}"
        );
    }
    assert!(count > 0, "no action references found in workflow");
    assert!(!workflow.contains("@stable"), "workflow uses a floating stable toolchain");
    assert!(!workflow.contains("@nightly"), "workflow uses a floating nightly toolchain");
}
