//! An SDF template: one signed distance field, ray marched on the GPU against
//! an acceleration grid, and queried by the physics on the CPU.
//!
//! # How a frame fits together
//!
//! Geometry is authored in `bsn!`, as one child entity per brush under the
//! `SdfWorld` root, each carrying `SdfShape`, `Transform`, `Modifiers`,
//! `CsgOperation` and `Albedo`. `sync_shapes_to_gpu` walks the root's
//! `Children` in order and packs them into `GpuShape` for a fixed-size
//! storage buffer.
//!
//! Order is not decoration: each shape blends against everything before it, so
//! a subtract that runs early carves nothing. `Children` preserves the order
//! they were written in, which a plain `Query` does not - entities land in
//! archetypes by component set, not by spawn order.
//!
//! Rendering happens on a single quad parented to the camera and fitted to the
//! frustum, one ray per pixel in the fragment stage. It writes real depth, so
//! an ordinary Bevy 3D entity would share the world and occlude correctly -
//! but nothing in the world is one. Everything here is the field.
//!
//! # Where things live
//!
//! | folder | what is in it |
//! |--------|---------------|
//! | `sdf/` | the field, the ray march, the lights - no game in it |
//! | `game/` | the authored world, bodies, controls, overlay |
//! | `dev/` | measurement and tests, never reached by a player |
//!
//! A `bench` run adds `sdf/` and nothing from `game/`, which is what makes a
//! frame time attributable to the renderer.
//!
//! `args` sits outside all three. It is the command line, and every module that
//! owns a tunable reads its own flag from it - see [`args`] for the list.
//!
//! # One field, two evaluators
//!
//! `scene_distance` exists twice: once in `assets/shaders/sdf.wgsl` for
//! rendering, once in `sdf/field.rs` for physics. Both read the **same packed
//! `GpuShape` values**, so only the arithmetic can drift, never the data.
//! Change one, change the other in the same commit. The CPU side is covered by closed-form
//! tests, and the overlay's `cpu sdf here` readout is the live alarm: it must
//! reach zero exactly as the camera touches what the GPU drew.
//!
//! The renderer's `ray_march` is the one deliberate difference - it relaxes its
//! hit threshold with distance for speed, while the CPU field stays exact.
//!
//! # Debug keys
//!
//! | key | effect |
//! |-----|--------|
//! | `V` | hide the quad, revealing Bevy's own per-frame cost |
//! | `H` | shaded / march-step heatmap |

mod args;
mod dev;
mod game;
mod sdf;

use bevy::{prelude::*, window::PresentMode};

fn main() {
    // Two command-line modes measure or photograph instead of playing, and
    // both drop whatever would drift between two runs. See dev::bench and
    // dev::shot.
    let bench = dev::bench::requested();
    let shot = dev::shot::requested();

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(match (&bench, &shot) {
            (None, None) => Window {
                present_mode: PresentMode::AutoNoVsync, // honest FPS numbers
                ..default()
            },
            _ => dev::bench::bench_window(),
        }),
        ..default()
    }))
    // Input is always registered: the render toggles read `ButtonInput<Action>`,
    // so the plugin that fills it is not optional. A bench run pins the camera
    // every frame instead of leaving it out.
    .add_plugins((
        sdf::field::FieldPlugin,
        sdf::render::RenderPlugin,
        sdf::light::LightPlugin,
        game::input::InputPlugin,
    ));

    match (bench, shot) {
        // A bench run generates its own scene, so it takes nothing from `game`.
        (Some(bench), _) => {
            app.add_plugins(dev::bench::BenchPlugin(bench));
        }
        // A shot run wants the authored world, but not the bodies falling
        // through it or the overlay on top: both would differ between two runs
        // that were meant to differ only in the shader.
        (None, Some(path)) => {
            app.add_plugins((game::world::WorldPlugin, dev::shot::ShotPlugin(path)));
        }
        (None, None) => {
            app.add_plugins((
                game::world::WorldPlugin,
                game::physics::PhysicsPlugin,
                game::ui::UiPlugin,
            ));
        }
    }
    app.run();
}
