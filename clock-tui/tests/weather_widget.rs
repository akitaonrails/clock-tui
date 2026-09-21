use std::path::PathBuf;
use std::process::Command;

#[test]
fn weather_widget_scenarios() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/weather-widget.sh");
    let status = Command::new("bash")
        .arg(script)
        .status()
        .expect("run weather widget regression tests");

    assert!(status.success(), "weather widget scenarios failed");
}
