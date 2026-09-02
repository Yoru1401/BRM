//! One frame of the authored world, written to a file.
//!
//! ```sh
//! cargo run --release -- shot before.png
//! ```
//!
//! Exists because the only open question about the shadow proxy was one no
//! number could answer: whether inflated boxes look acceptable as shadows. Two
//! builds, two files, one `git diff` of the shader between them.
//!
//! Physics is left out and the camera never moves, so two shots differ only
//! where the shader does. Anything that drifts frame to frame - falling bodies,
//! the overlay - would show up as a difference that is not the change.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use std::path::PathBuf;

/// Frames to run before capturing. The shader and the storage buffers are
/// assets: until they land the quad draws an empty field, which is a perfectly
/// plausible-looking picture of nothing.
const SETTLE_FRAMES: usize = 240;

/// Frames to keep running after asking. The capture is asynchronous - the
/// observer fires once the render world hands the image back, and exiting
/// before that writes no file at all.
const DRAIN_FRAMES: usize = 60;

/// Reads the command line. `None` means this is not a shot run.
pub(crate) fn requested() -> Option<PathBuf> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) != Some("shot") {
        return None;
    }
    Some(PathBuf::from(
        arguments.get(1).map_or("shot.png", String::as_str),
    ))
}

pub(crate) struct ShotPlugin(pub(crate) PathBuf);

impl Plugin for ShotPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ShotPath(self.0.clone()))
            .add_systems(Update, capture);
    }
}

#[derive(Resource)]
struct ShotPath(PathBuf);

fn capture(
    mut commands: Commands,
    path: Res<ShotPath>,
    mut frame: Local<usize>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame += 1;
    if *frame == SETTLE_FRAMES {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.0.clone()));
    }
    if *frame >= SETTLE_FRAMES + DRAIN_FRAMES {
        exit.write(AppExit::Success);
    }
}
