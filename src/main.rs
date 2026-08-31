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
//! ordinary Bevy 3D entities - the holograms here - share the world and occlude
//! correctly.
//!
//! # One field, two evaluators
//!
//! `scene_distance` exists twice: once in `assets/shaders/sdf.wgsl` for
//! rendering, once here for physics. Both read the **same packed `GpuShape`
//! values**, so only the arithmetic can drift, never the data. Change one,
//! change the other in the same commit. The CPU side is covered by closed-form
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


mod bench;
mod field;
mod input;
mod physics;
mod render;
mod ui;
mod world;
#[cfg(test)]
mod tests;

use bevy::{prelude::*, window::PresentMode};

/// Physics bodies and holograms, off for timing runs. The bodies are packed
/// into the same field as everything else, so they are extra shapes in every
/// march step as well as CPU work of their own - a measurement of the renderer
/// has to be able to exclude them.
pub(crate) const SPAWN_EXTRAS: bool = true;

fn main() {
    // `bench <scene>` on the command line measures instead of playing: a
    // generated scene, a parked camera, and none of the extras. See bench.rs.
    let bench = bench::requested();

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(match &bench {
            Some(_) => bench::bench_window(),
            None => Window {
                present_mode: PresentMode::AutoNoVsync, // honest FPS numbers
                ..default()
            },
        }),
        ..default()
    }))
    // Input is always registered: the render toggles read `ButtonInput<Action>`,
    // so the plugin that fills it is not optional. A bench run pins the camera
    // every frame instead of leaving it out.
    .add_plugins((field::FieldPlugin, render::RenderPlugin, input::InputPlugin));

    match bench {
        Some(bench) => {
            app.add_plugins(bench::BenchPlugin(bench));
        }
        None => {
            app.add_plugins((world::WorldPlugin, physics::PhysicsPlugin, ui::UiPlugin));
        }
    }
    app.run();
}
