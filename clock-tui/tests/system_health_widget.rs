use std::path::PathBuf;
use std::process::Command;

#[test]
fn system_health_widget_scenarios() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/system-health-widget.sh");
    let status = Command::new("bash")
        .arg(script)
        .status()
        .expect("run system-health widget regression tests");

    assert!(status.success(), "system-health widget scenarios failed");
}
