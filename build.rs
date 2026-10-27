//! On macOS, compiles the WiFi helper (src/wifi/helper.swift) with swiftc from Xcode's command
//! line tools, for src/wifi/survey.rs to carry inside the binary. Other OSes build nothing here.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/wifi/helper.swift");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("wifi-helper");
    let status = Command::new("swiftc")
        .args(["-O", "src/wifi/helper.swift", "-o"])
        .arg(&out)
        .status()
        .expect("swiftc runs: install Xcode's command line tools (xcode-select --install)");
    assert!(
        status.success(),
        "swiftc couldn't build src/wifi/helper.swift"
    );
}
