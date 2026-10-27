//! Every name in one place, so renaming the tool means editing this file. The binary name in
//! Cargo.toml (`name = "floodlight"`) is the one unavoidable exception; keep it in step with `BIN`.

/// The command people type.
pub const BIN: &str = "floodlight";

/// The folder under the OS's data directory that holds the macOS WiFi helper app.
pub const CONFIG_DIR: &str = "floodlight";

/// The macOS WiFi helper app, as it appears under Location Services in System Settings.
pub const WIFI_HELPER: &str = "Floodlight WiFi";

/// The helper app's bundle identifier, which macOS keys the Location permission on.
pub const WIFI_HELPER_ID: &str = "cavecomputing.floodlight.wifi";
