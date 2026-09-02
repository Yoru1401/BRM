//! The field and everything that draws it.
//!
//! Nothing in here knows a game exists. [`field`] owns the shapes, the packing
//! and the CPU evaluator; [`render`] puts a ray behind every pixel; [`light`]
//! is what those rays shade against. A `bench` run loads all three and nothing
//! else, which is what makes a measurement mean something.

pub mod field;
pub mod light;
pub mod render;
