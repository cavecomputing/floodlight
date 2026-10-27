//! Pinging one host over and over: each round trip, the ones lost, and what they add up to.
//! Nothing in here draws anything, so all of it runs in tests.

pub mod echo;
pub mod pings;
pub mod target;
