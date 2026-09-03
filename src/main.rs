mod command_line;
mod dev;
mod game;
mod sdf;

use bevy::{prelude::*, window::PresentMode};

fn main() {
    let bench = dev::benchmark::requested();
    let shot = dev::screenshot::requested();

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(match (&bench, &shot) {
            (None, None) => Window {
                present_mode: PresentMode::AutoNoVsync,
                ..default()
            },
            _ => dev::benchmark::bench_window(),
        }),
        ..default()
    }))
    .add_plugins((
        sdf::field::FieldPlugin,
        sdf::render::RenderPlugin,
        sdf::light::LightPlugin,
        game::input::InputPlugin,
    ));

    match (bench, shot) {
        (Some(bench), _) => {
            app.add_plugins(dev::benchmark::BenchmarkPlugin(bench));
        }

        (None, Some(path)) => {
            app.add_plugins((
                game::world::WorldPlugin,
                dev::screenshot::ScreenshotPlugin(path),
            ));
        }
        (None, None) => {
            app.add_plugins((
                game::world::WorldPlugin,
                game::physics::PhysicsPlugin,
                game::overlay::OverlayPlugin,
            ));
        }
    }
    app.run();
}
