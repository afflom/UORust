//! The binary end to end.

use std::process::Command;

/// A cold reading prints `heat`.
#[test]
fn cold_reading_heats() {
    let out = Command::new(env!("CARGO_BIN_EXE_thermo"))
        .arg("10")
        .output()
        .expect("runs");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "heat");
}
