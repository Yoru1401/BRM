//! Tools that never reach a player: measurement and tests.
//!
//! [`bench`] and [`shot`] are both compiled in - each is reached by a
//! command-line argument, not a feature flag, so the thing measured and the
//! thing photographed are the thing shipped.

pub mod bench;
pub mod shot;

#[cfg(test)]
mod tests;
