//! Pinging hosts over and over (each round trip, the ones lost, and what they add up to), and
//! tracing the route to one, a hop at a time. Nothing in here draws anything, so all of it runs
//! in tests.

pub mod echo;
pub mod pings;
pub mod target;
pub mod trace;
