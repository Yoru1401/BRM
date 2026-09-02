//! The command line, read by whichever module owns the knob.
//!
//! Every tunable here used to be a `const`, which meant a compile to try a
//! value. A flag is a restart. That is the whole ambition - not a settings
//! system, not a file, not a UI, and nothing that has to be kept in sync with
//! the code it configures: the module that owns a value is the module that
//! reads its flag, and the default stays a `const` right next to it.
//!
//! ```sh
//! cargo run --release -- --speed 12 --sensitivity 0.001
//! cargo run --release -- --omega 1.0 --no-grid
//! cargo run --release -- bench spread:80 --shadow-steps 12
//! ```
//!
//! Flags apply to an ordinary run, a `bench` run and a `shot` run alike, since
//! nothing here looks at which one it is.
//!
//! ponytail: re-reads `std::env::args` per lookup. There are a dozen lookups,
//! all at startup, and a `OnceLock` would be more machinery than the thing it
//! saves.

/// Whether a bare flag like `--no-grid` is present.
pub(crate) fn flag(name: &str) -> bool {
    std::env::args().any(|argument| argument == name)
}

/// The number after `name`, if both are there.
pub(crate) fn value(name: &str) -> Option<f32> {
    value_in(&std::env::args().collect::<Vec<_>>(), name)
}

/// The half of [`value`] that does not read the process, so it can be tested.
pub(crate) fn value_in(arguments: &[String], name: &str) -> Option<f32> {
    let at = arguments.iter().position(|argument| argument == name)?;
    arguments.get(at + 1)?.parse().ok()
}

/// The positional argument at `index`, counting past the executable. Index 0 is
/// the mode word - `bench` or `shot` - and index 1 is its argument.
pub(crate) fn positional(index: usize) -> Option<String> {
    std::env::args().nth(index + 1)
}
