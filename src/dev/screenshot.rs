use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use std::path::PathBuf;

use crate::command_line;
use crate::dev::SHADER_WARMUP;

const SETTLE_FRAMES: usize = 240;

const DRAIN_FRAMES: usize = 60;

pub(crate) fn requested() -> Option<PathBuf> {
    if command_line::positional(0).as_deref() != Some("shot") {
        return None;
    }
    Some(PathBuf::from(
        command_line::positional(1).unwrap_or_else(|| "shot.png".into()),
    ))
}

pub(crate) struct ScreenshotPlugin(pub(crate) PathBuf);

impl Plugin for ScreenshotPlugin {
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
    time: Res<Time>,
    mut settled: Local<usize>,
    mut draining: Local<Option<usize>>,
    mut exit: MessageWriter<AppExit>,
) {
    if let Some(left) = draining.as_mut() {
        if *left == 0 {
            exit.write(AppExit::Success);
        } else {
            *left -= 1;
        }
        return;
    }

    *settled += 1;
    if *settled < SETTLE_FRAMES || time.elapsed() < SHADER_WARMUP {
        return;
    }
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path.0.clone()));
    *draining = Some(DRAIN_FRAMES);
}
