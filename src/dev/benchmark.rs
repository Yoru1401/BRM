use bevy::{
    prelude::*,
    window::{PresentMode, WindowResolution},
};

use crate::command_line;
use crate::game::world::SdfWorld;
use crate::sdf::field::{Albedo, Brush, GridSettings};
use crate::sdf::light::{Light, LightKind};
use crate::sdf::render::{Quad, SdfMaterial};

const WARMUP_FRAMES: usize = 120;

const RECORDED_FRAMES: usize = 600;

const BENCH_EYE: Vec3 = Vec3::new(0.0, 3.0, 11.0);

pub(crate) const SLAB_HALF_SIZE: Vec3 = Vec3::new(6.0, 1.5, 3.0);

pub(crate) const SPREAD_HALF_SIZE: Vec3 = Vec3::new(40.0, 6.0, 40.0);

const SPREAD_BOX: Vec3 = Vec3::splat(0.8);

#[derive(Resource, Debug, Clone)]
pub(crate) struct Bench {
    pub(crate) scene: BenchScene,

    pub(crate) repeat: usize,

    pub(crate) lights: usize,

    pub(crate) shadows: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BenchScene {
    Empty,

    Grid(usize),

    Spread(usize),
}

pub(crate) fn requested() -> Option<Bench> {
    if command_line::positional(0).as_deref() != Some("bench") {
        return None;
    }
    let count_after = |prefix: &str, text: &str| text.strip_prefix(prefix)?.parse().ok();
    let scene = match command_line::positional(1).as_deref() {
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
    Some(Bench {
        scene,
        repeat: command_line::value("--repeat").map_or(1, |count| (count as usize).max(1)),
        lights: command_line::value("--lights").map_or(1, |count| count as usize),
        shadows: command_line::value("--shadows").map_or(0, |count| count as usize),
    })
}

pub(crate) struct BenchmarkPlugin(pub(crate) Bench);

impl Plugin for BenchmarkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone())
            .init_resource::<Frames>()
            .add_systems(Startup, (spawn_bench_scene, spawn_bench_lights))
            .add_systems(PostUpdate, park_camera)
            .add_systems(Update, record);
    }
}

pub(crate) fn bench_window() -> Window {
    Window {
        present_mode: PresentMode::Immediate,
        resolution: WindowResolution::new(1280, 720),
        title: "IDK bench".into(),
        ..default()
    }
}

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
                    Brush,
                    placement,
                    Albedo(Vec3::new(
                        x as f32 / per_axis as f32,
                        y as f32 / per_axis as f32,
                        z as f32 / per_axis as f32,
                    )),
                ));
            }
        });
}

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

#[derive(Resource, Default)]
struct Frames {
    times: Vec<f32>,
    done: usize,
}

#[allow(clippy::too_many_arguments)]
fn record(
    mut frames: ResMut<Frames>,
    time: Res<Time>,
    bench: Res<Bench>,
    brushes: Query<(), With<Brush>>,
    grid: Res<GridSettings>,
    quad: Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>,
    materials: Res<Assets<SdfMaterial>>,
    mut exit: MessageWriter<AppExit>,
) {
    frames.times.push(time.delta_secs() * 1000.0);

    let block = RECORDED_FRAMES + if frames.done == 0 { WARMUP_FRAMES } else { 0 };
    if frames.times.len() < block {
        return;
    }

    let params = materials
        .get(&quad.0)
        .map(|material| material.render_params.clone())
        .unwrap_or_default();

    let mut recorded: Vec<f32> = frames.times.split_off(block - RECORDED_FRAMES);
    recorded.sort_by(f32::total_cmp);
    let at = |fraction: f32| recorded[((recorded.len() - 1) as f32 * fraction) as usize];

    let scene = match bench.scene {
        BenchScene::Empty => "empty".to_string(),
        BenchScene::Grid(count) => format!("grid:{count}"),
        BenchScene::Spread(count) => format!("spread:{count}"),
    };

    let median = at(0.5);
    if (median - 1000.0 / 60.0).abs() < 0.2 {
        eprintln!("bench: median {median:.3} ms is suspiciously exactly 60 Hz - vsync?");
    }

    println!(
        "run\t{}\tscene\t{scene}\tshapes\t{}\tcull\t{}\tomega\t{:.2}\tgrid\t{}\tlights\t{}\tshadows\t{}\tsteps\t{}\tmin\t{:.3}\tmedian\t{:.3}\tp95\t{:.3}\tframes\t{}",
        frames.done + 1,
        brushes.iter().count(),
        if params.cull == 1 { "on" } else { "off" },
        params.omega,
        if grid.enabled {
            grid.resolution.to_string()
        } else {
            "off".to_string()
        },
        bench.lights,
        bench.shadows,
        params.shadow_steps,
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
