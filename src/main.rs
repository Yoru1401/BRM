mod command_line;
mod dev;
mod display;
mod game;
mod sdf;

use bevy::{prelude::*, render::diagnostic::RenderDiagnosticsPlugin, window::PresentMode};

fn main() {
    let bench = dev::benchmark::requested();
    let shot = dev::screenshot::requested();
    let display = display::Display::default();

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(match (&bench, &shot) {
            (None, None) => display.window("IDK", PresentMode::AutoNoVsync),
            _ => display.window("IDK bench", PresentMode::Immediate),
        }),
        ..default()
    }))
    .add_plugins((
        sdf::field::FieldPlugin,
        sdf::render::RenderPlugin,
        sdf::light::LightPlugin,
        sdf::dynamic::DynamicPlugin,
        game::input::InputPlugin,
    ));

    if display.render_scale < 1.0 {
        app.add_plugins(display::RenderScalePlugin(display));
    }

    match (bench, shot) {
        (Some(bench), _) => {
            app.add_plugins((
                game::scene::ScenePlugin,
                RenderDiagnosticsPlugin,
                dev::benchmark::BenchmarkPlugin(bench),
            ));
        }

        (None, Some(path)) => {
            app.add_plugins((
                game::scene::ScenePlugin,
                dev::screenshot::ScreenshotPlugin(path),
            ));
            if game::character::playing() {
                app.add_plugins((game::physics::PhysicsPlugin, game::character::CharacterPlugin));
            }
        }
        (None, None) => {
            app.add_plugins((
                game::scene::ScenePlugin,
                game::physics::PhysicsPlugin,
                game::overlay::OverlayPlugin,
            ));
            if game::character::playing() {
                app.add_plugins(game::character::CharacterPlugin);
            }
        }
    }
    app.run();
}
