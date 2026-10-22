//! Every name in one place, so renaming the tool means editing this file. The binary name in
//! Cargo.toml (`name = "floodlight"`) is the one unavoidable exception; keep it in step with `BIN`.

/// The command people type.
pub const BIN: &str = "floodlight";

/// The folder under the OS's config directory that holds settings.toml.
pub const CONFIG_DIR: &str = "floodlight";
