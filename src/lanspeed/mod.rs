//! The LAN speed test: this tool on one machine serving, another connecting, and the throughput
//! between them over TCP or UDP, iperf-style. Nothing in here draws anything, so all of it runs
//! in tests.

pub mod runs;
pub mod server;
pub mod transfer;
pub mod wire;

/// The port the server listens on, TCP and UDP both: iperf's, so a firewall rule made for it
/// serves this too.
pub const PORT: u16 = 5201;
