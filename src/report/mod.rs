//! The PDF reports for `--report`: one sweep, survey, speed test, LAN speed test, set of pings,
//! trace or lookup, written as a document to hand a client or a manager. Nothing in here draws on the screen.

pub mod dns;
pub mod lanspeed;
pub mod pdf;
pub mod ping;
pub mod sheet;
pub mod speedtest;
pub mod sweep;
pub mod trace;
pub mod wireless;
