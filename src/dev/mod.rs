use std::time::Duration;

pub mod benchmark;
pub mod screenshot;

#[cfg(test)]
mod tests;

pub(crate) const SHADER_WARMUP: Duration = Duration::from_secs(3);
