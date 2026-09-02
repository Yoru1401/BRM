//! Frame-time measurement, so a claim about performance can be checked instead
//! of argued about.
//!
//! ```sh
//! cargo run --release -- bench grid:20
//! cargo run --release -- bench spread:80
//! cargo run --release -- bench spread:80 --no-cull
//! cargo run --release -- bench spread:80 --omega 1.0
//! cargo run --release -- bench spread:80 --repeat 5
//! cargo run --release -- bench spread:80 --grid 8
//! cargo run --release -- bench spread:80 --no-grid
//! cargo run --release -- bench spread:80 --lights 8
//! cargo run --release -- bench spread:80 --lights 8 --shadows 2
//! cargo run --release -- bench spread:80 --shadows 1 --shadow-steps 12
//! cargo run --release -- bench empty
//! ```
//!
//! A run parks the camera at a fixed pose, spawns a generated scene instead of
//! the authored world, runs no physics and no overlay, throws away the
//! first frames, then prints one tab-separated line and exits.
//!
//! # Why the scenes are generated
//!
//! The count scenes tile a **fixed volume**. Twenty shapes and eighty shapes
//! fill the same slab and cover the same pixels; only the shape count differs.
//! An earlier round measured `cube_80` against `cube_20` by adding shapes
//! beside each other, which grew the screen coverage at the same time - the
//! numbers went up and said nothing about the scan.
//!
//! # Two scene shapes, because they measure different things
//!
//! `grid:` packs everything into one small slab. Every shape is next to every
//! other, so a ray near one is near all of them and the per-shape box reject
//! can almost never fire. It is the reject's worst case, and the honest
//! measurement of the raw scan.
//!
//! `spread:` scatters the same count over a volume a hundred times larger,
//! which is what a level actually looks like. Most shapes are nowhere near any
//! given ray, so this is where the reject should pay - and `--no-cull` is what
//! turns it off, so the two runs can be compared.

use bevy::{
    prelude::*,
    window::{PresentMode, WindowResolution},
};

use crate::game::world::SdfWorld;
use crate::sdf::field::{Albedo, SdfShape};
use crate::sdf::light::{Light, LightKind};
use crate::sdf::render::{Quad, SdfMaterial};

/// Frames to run before recording starts: shader compilation, buffer creation
/// and the first swapchain frames are all one-offs.
const WARMUP_FRAMES: usize = 120;
/// Frames recorded. At a few hundred FPS this is a fraction of a second, which
/// is enough for a median but short enough to run a sweep by hand.
const RECORDED_FRAMES: usize = 600;

/// Where the camera sits for every run. Chosen so the generated slab fills most
/// of the view - a benchmark pointing at empty space measures nothing.
const BENCH_EYE: Vec3 = Vec3::new(0.0, 3.0, 11.0);

/// The volume every `grid:` scene tiles. Fixed on purpose: the screen coverage
/// must not change with the shape count.
pub(crate) const SLAB_HALF_SIZE: Vec3 = Vec3::new(6.0, 1.5, 3.0);

/// The volume a `spread:` scene scatters over - two orders of magnitude more
/// room than the slab, so most shapes are far from any given ray.
pub(crate) const SPREAD_HALF_SIZE: Vec3 = Vec3::new(40.0, 6.0, 40.0);

/// Half size of one `spread:` box. Fixed, so a shape costs the same to evaluate
/// whatever the count is, and small enough that neighbours never touch - two
/// merged boxes would be one blended surface instead of two rejects.
const SPREAD_BOX: Vec3 = Vec3::splat(0.8);

// ----------------------------------------------------------- the command line

/// What a run was asked to measure.
#[derive(Resource, Debug, Clone)]
pub(crate) struct Bench {
    pub(crate) scene: BenchScene,
    /// The per-shape box reject, on unless `--no-cull`. The A/B this exists for
    /// only means anything on a `spread:` scene.
    pub(crate) cull: bool,
    /// March over-relaxation. `--omega 1.0` is plain sphere tracing, which is
    /// the baseline every other value is measured against.
    pub(crate) omega: f32,
    /// Measurement blocks to run before exiting. Repeats inside one process
    /// share a window, a driver state and a thermal state, so the spread
    /// between them is the honest error bar on a single number - and until
    /// 2026-08-31 there was none, which left a 6% result unreadable next to
    /// 18% of drift between sessions.
    pub(crate) repeat: usize,
    /// Cells per axis of the acceleration grid. Fine culls better and makes
    /// empty space cost more steps, so the useful value is measured, not
    /// reasoned about.
    pub(crate) grid: u32,
    pub(crate) use_grid: bool,
    /// Point lights ringing the scene. Diffuse is nearly free; the price of a
    /// light is whether it casts.
    pub(crate) lights: usize,
    /// How many of them cast. **One march per shaded pixel each**, so this is
    /// the number that moves the frame time.
    pub(crate) shadows: usize,
    pub(crate) shadow_steps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BenchScene {
    /// No shapes at all. The quad still runs, so this is the cost of the march
    /// itself against an empty field.
    Empty,
    /// `count` boxes tiling the fixed slab. Dense: the reject's worst case.
    Grid(usize),
    /// `count` separated boxes over a much larger volume. Sparse: what a level
    /// looks like, and where the reject is supposed to earn its keep.
    Spread(usize),
}

/// Reads the command line. `None` means an ordinary run of the game.
///
/// ponytail: hand-parsed, three arguments. A `clap` dependency to read
/// `bench grid:20` would be larger than the thing it parses.
pub(crate) fn requested() -> Option<Bench> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().map(String::as_str) != Some("bench") {
        return None;
    }
    let count_after = |prefix: &str, text: &str| text.strip_prefix(prefix)?.parse().ok();
    let scene = match arguments.get(1).map(String::as_str) {
        Some("empty") | None => BenchScene::Empty,
        Some(other) => {
            if let Some(count) = count_after("grid:", other) {
                BenchScene::Grid(count)
            } else if let Some(count) = count_after("spread:", other) {
                BenchScene::Spread(count)
            } else {
                eprintln!(
                    "bench: unknown scene {other:?}, expected `empty`, `grid:<count>` or `spread:<count>`"
                );
                std::process::exit(2);
            }
        }
    };
    let flag = |name: &str| arguments.iter().any(|argument| argument == name);
    let value_after = |name: &str| {
        let at = arguments.iter().position(|argument| argument == name)?;
        arguments.get(at + 1)?.parse::<f32>().ok()
    };
    Some(Bench {
        scene,
        cull: !flag("--no-cull"),
        omega: value_after("--omega").unwrap_or(crate::sdf::render::OMEGA),
        repeat: value_after("--repeat").map_or(1, |count| (count as usize).max(1)),
        grid: value_after("--grid")
            .map_or(crate::sdf::field::GRID_DEFAULT_RESOLUTION, |n| n as u32),
        use_grid: !flag("--no-grid"),
        lights: value_after("--lights").map_or(1, |count| count as usize),
        shadows: value_after("--shadows").map_or(0, |count| count as usize),
        shadow_steps: value_after("--shadow-steps")
            .map_or(crate::sdf::render::SHADOW_STEPS, |count| count as u32),
    })
}

// -------------------------------------------------------------------- the run

pub(crate) struct BenchPlugin(pub(crate) Bench);

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone())
            .init_resource::<Frames>()
            // PostStartup: the camera and the quad are spawned in Startup, and
            // two systems in one schedule have no order between them.
            .add_systems(Startup, (spawn_bench_scene, spawn_bench_lights))
            .add_systems(PostStartup, apply_switches)
            // PostUpdate, every frame: `fly_camera` is still running, and all
            // 600 recorded frames have to be shot from the same place.
            .add_systems(PostUpdate, park_camera)
            .add_systems(Update, record);
    }
}

/// The window a bench run wants: no vsync, and a size that does not depend on
/// how the developer last left their desktop.
///
/// `Immediate`, not `AutoNoVsync`. Auto falls back to Fifo when the surface
/// prefers it, and Fifo caps the frame at the refresh rate - which turned every
/// measurement faster than 16.67 ms into the same 16.6, and made a plateau look
/// like a bottleneck. A capped run reads `max(true, 16.67)`, so it cannot be
/// compared against anything.
pub(crate) fn bench_window() -> Window {
    Window {
        present_mode: PresentMode::Immediate,
        resolution: WindowResolution::new(1280, 720),
        title: "IDK bench".into(),
        ..default()
    }
}

// ----------------------------------------------------------------- the scenes

/// Boxes filling `SLAB_HALF_SIZE`, laid out so the count changes and the
/// silhouette does not.
fn spawn_bench_scene(mut commands: Commands, bench: Res<Bench>) {
    let (count, layout) = match bench.scene {
        BenchScene::Empty => {
            commands.spawn((SdfWorld, Transform::default()));
            return;
        }
        BenchScene::Grid(count) => (count, grid_layout(count)),
        BenchScene::Spread(count) => (count, spread_layout(count)),
    };

    let per_axis = cells_per_axis(count);
    commands
        .spawn((SdfWorld, Transform::default()))
        .with_children(|world| {
            for (index, placement) in layout.into_iter().enumerate() {
                let (x, y, z) = cell_of(index, per_axis);
                world.spawn((
                    SdfShape::Cube,
                    placement,
                    // A colour per cell, so a wrong layout is visible rather
                    // than silently measured.
                    Albedo(Vec3::new(
                        x as f32 / per_axis as f32,
                        y as f32 / per_axis as f32,
                        z as f32 / per_axis as f32,
                    )),
                ));
            }
        });
}

/// Cells along each axis, as close to a cube as the count allows. The last row
/// is left short rather than resizing everything to fit, so a count that is not
/// a perfect cube still tiles at the same cell size as one that is.
pub(crate) fn cells_per_axis(count: usize) -> usize {
    (count as f32).cbrt().ceil().max(1.0) as usize
}

fn cell_of(index: usize, per_axis: usize) -> (usize, usize, usize) {
    (
        index % per_axis,
        (index / per_axis) % per_axis,
        index / (per_axis * per_axis),
    )
}

/// Where each brush of a count scene goes. Pure, so the layout is testable
/// without a window: every brush must stay inside `SLAB_HALF_SIZE`, and the
/// slab it fills must not change with the count.
pub(crate) fn grid_layout(count: usize) -> Vec<Transform> {
    let per_axis = cells_per_axis(count);
    let cell = SLAB_HALF_SIZE / per_axis as f32;
    (0..count)
        .map(|index| {
            let (x, y, z) = cell_of(index, per_axis);
            let along = |slot: usize, half: f32, size: f32| -half + size + 2.0 * size * slot as f32;
            Transform {
                translation: Vec3::new(
                    along(x, SLAB_HALF_SIZE.x, cell.x),
                    along(y, SLAB_HALF_SIZE.y, cell.y),
                    along(z, SLAB_HALF_SIZE.z, cell.z),
                ),
                scale: cell,
                ..default()
            }
        })
        .collect()
}

/// Where each brush of a `spread:` scene goes: the same lattice, over a much
/// larger volume, with a fixed box size so the shapes never touch.
pub(crate) fn spread_layout(count: usize) -> Vec<Transform> {
    let per_axis = cells_per_axis(count);
    let step = SPREAD_HALF_SIZE * 2.0 / per_axis as f32;
    (0..count)
        .map(|index| {
            let (x, y, z) = cell_of(index, per_axis);
            let slot = Vec3::new(x as f32, y as f32, z as f32);
            Transform {
                translation: -SPREAD_HALF_SIZE + step * 0.5 + step * slot,
                scale: SPREAD_BOX,
                ..default()
            }
        })
        .collect()
}

/// A ring of point lights around the scene, the first `shadows` of them
/// casting.
///
/// `--lights 0` is allowed and useful: the scene renders on ambient alone, so
/// the difference against `--lights 1` is the cost of *running* the light path
/// rather than the cost of the shader containing it.
fn spawn_bench_lights(mut commands: Commands, bench: Res<Bench>) {
    let radius = SPREAD_HALF_SIZE.x * 0.5;
    for index in 0..bench.lights {
        let turn = index as f32 / bench.lights.max(1) as f32 * std::f32::consts::TAU;
        commands.spawn((
            Light {
                kind: LightKind::Point,
                colour: Vec3::ONE,
                intensity: 6.0,
                range: radius,
                shadow: index < bench.shadows,
                ..default()
            },
            Transform::from_xyz(turn.cos() * radius, 6.0, turn.sin() * radius),
        ));
    }
}

fn park_camera(camera: Single<&mut Transform, With<Camera3d>>) {
    *camera.into_inner() = Transform::from_translation(BENCH_EYE).looking_at(Vec3::ZERO, Vec3::Y);
}

fn apply_switches(
    bench: Res<Bench>,
    mut grid: ResMut<crate::sdf::field::GridSettings>,
    quad: Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
) {
    if let Some(mut material) = materials.get_mut(&quad.0) {
        material.render_params.cull = u32::from(bench.cull);
        material.render_params.omega = bench.omega;
        material.render_params.shadow_steps = bench.shadow_steps;
    }
    // The grid is rebuilt from these, so they go to the settings rather than
    // straight into the uniform.
    *grid = crate::sdf::field::GridSettings {
        resolution: bench.grid,
        enabled: bench.use_grid,
    };
}

// ------------------------------------------------------------------ measuring

#[derive(Resource, Default)]
struct Frames {
    times: Vec<f32>,
    done: usize,
}

/// Records frame times, then prints and exits.
///
/// `Time::delta` is the whole frame, which is what a player feels. It is not a
/// GPU timing - a timestamp query would separate the march from everything
/// else, and is the next rung if the march ever stops being the obvious cost.
fn record(
    mut frames: ResMut<Frames>,
    time: Res<Time>,
    bench: Res<Bench>,
    shapes: Query<(), With<SdfShape>>,
    mut exit: MessageWriter<AppExit>,
) {
    frames.times.push(time.delta_secs() * 1000.0);
    // Only the first block pays the warm-up; the ones after it are already hot.
    let block = RECORDED_FRAMES + if frames.done == 0 { WARMUP_FRAMES } else { 0 };
    if frames.times.len() < block {
        return;
    }

    let mut recorded: Vec<f32> = frames.times.split_off(block - RECORDED_FRAMES);
    recorded.sort_by(f32::total_cmp);
    let at = |fraction: f32| recorded[((recorded.len() - 1) as f32 * fraction) as usize];

    let scene = match bench.scene {
        BenchScene::Empty => "empty".to_string(),
        BenchScene::Grid(count) => format!("grid:{count}"),
        BenchScene::Spread(count) => format!("spread:{count}"),
    };
    // 60 Hz to within a hair means the frame was waiting on the display, not
    // on the GPU, and the number is a floor rather than a cost.
    let median = at(0.5);
    if (median - 1000.0 / 60.0).abs() < 0.2 {
        eprintln!("bench: median {median:.3} ms is suspiciously exactly 60 Hz - vsync?");
    }
    // Tab separated, one line: two runs diff cleanly.
    println!(
        "run\t{}\tscene\t{scene}\tshapes\t{}\tcull\t{}\tomega\t{:.2}\tgrid\t{}\tlights\t{}\tshadows\t{}\tsteps\t{}\tmin\t{:.3}\tmedian\t{:.3}\tp95\t{:.3}\tframes\t{}",
        frames.done + 1,
        shapes.iter().count(),
        if bench.cull { "on" } else { "off" },
        bench.omega,
        if bench.use_grid {
            bench.grid.to_string()
        } else {
            "off".to_string()
        },
        bench.lights,
        bench.shadows,
        bench.shadow_steps,
        at(0.0),
        median,
        at(0.95),
        recorded.len(),
    );

    frames.times.clear();
    frames.done += 1;
    if frames.done >= bench.repeat {
        exit.write(AppExit::Success);
    }
}
