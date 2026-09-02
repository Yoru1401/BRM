//! What is played: the authored world, the bodies in it, the controls, the
//! overlay.
//!
//! These plugins are skipped entirely by a `bench` run - see `main`. Anything
//! that would move, spawn or draw per frame outside the field belongs here, so
//! a measurement never has to subtract it.

pub mod input;
pub mod physics;
pub mod ui;
pub mod world;
