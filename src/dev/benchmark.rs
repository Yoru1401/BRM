use bevy::{diagnostic::DiagnosticsStore, prelude::*};

use crate::command_line;
use crate::sdf::adf::Adf;
use crate::sdf::light::Light;
use crate::sdf::render::{Quad, SdfMaterial};

const WARMUP_FRAMES: usize = 120;

const RECORDED_FRAMES: usize = 600;

#[derive(Resource, Debug, Clone)]
pub(crate) struct Bench {
    pub(crate) repeat: usize,

    pub(crate) lights: usize,

    pub(crate) shadows: usize,
}

pub(crate) fn requested() -> Option<Bench> {
    if command_line::positional(0).as_deref() != Some("bench") {
        return None;
    }
    Some(Bench {
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
            .add_systems(PostStartup, trim_lights)
            .add_systems(Update, record);
    }
}

fn trim_lights(mut commands: Commands, bench: Res<Bench>, lights: Query<(Entity, &mut Light)>) {
    for (index, (entity, mut light)) in lights.into_iter().enumerate() {
        match index < bench.lights {
            true => light.shadow = index < bench.shadows,
            false => commands.entity(entity).despawn(),
        }
    }
}

#[derive(Resource, Default)]
struct Frames {
    times: Vec<f32>,
    drawn: Vec<f32>,
    done: usize,
    settled: Option<std::time::Instant>,
}

const PASS: &str = "main_opaque_pass_3d";

fn drawn_millis(store: &DiagnosticsStore) -> Option<f64> {
    store
        .iter()
        .filter(|entry| {
            let path = entry.path().as_str();
            path.contains(PASS) && path.ends_with("/elapsed_gpu")
        })
        .find_map(|entry| entry.value())
}

fn middle(sorted: &[f32], fraction: f32) -> f32 {
    sorted[((sorted.len() - 1) as f32 * fraction) as usize]
}

#[allow(clippy::too_many_arguments)]
fn record(
    mut frames: ResMut<Frames>,
    time: Res<Time>,
    bench: Res<Bench>,
    field: Res<Adf>,
    quad: Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>,
    materials: Res<Assets<SdfMaterial>>,
    diagnostics: Res<DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
) {
    let settled = frames.settled.get_or_insert_with(std::time::Instant::now);
    if settled.elapsed() < crate::dev::SHADER_WARMUP {
        return;
    }
    frames.times.push(time.delta_secs() * 1000.0);
    if let Some(millis) = drawn_millis(&diagnostics) {
        frames.drawn.push(millis as f32);
    }
    if frames.times.len() < WARMUP_FRAMES + RECORDED_FRAMES {
        return;
    }

    let Some(params) = materials.get(&quad.0).map(|m| m.render_params.clone()) else {
        return;
    };
    let mut recorded: Vec<f32> = frames.times.split_off(WARMUP_FRAMES);
    frames.times.clear();
    recorded.sort_by(f32::total_cmp);

    let mut drawn = std::mem::take(&mut frames.drawn);
    let warmed = WARMUP_FRAMES.min(drawn.len());
    let mut drawn: Vec<f32> = drawn.split_off(warmed);
    drawn.sort_by(f32::total_cmp);

    let gpu = |fraction: f32| match drawn.is_empty() {
        true => f32::NAN,
        false => middle(&drawn, fraction),
    };

    let median = middle(&recorded, 0.5);
    if drawn.is_empty() {
        eprintln!(
            "bench: no GPU timing for {PASS}; the adapter has no timestamp query, so the \
             frame column is all there is and vsync will mask the render"
        );
    } else if (median - 1000.0 / 60.0).abs() < 0.2 {
        eprintln!(
            "bench: frame median {median:.3} ms is the 60 Hz refresh, not the render - \
             read the gpu column"
        );
    }

    println!(
        "run\t{}\tbricks\t{}\tvoxel\t{:.4}\tomega\t{:.2}\tlights\t{}\tshadows\t{}\tsteps\t{}\
         \tgpu_min\t{:.3}\tgpu_median\t{:.3}\tgpu_p95\t{:.3}\
         \tframe_min\t{:.3}\tframe_median\t{:.3}\tframe_p95\t{:.3}\tframes\t{}\tgpu_samples\t{}",
        frames.done + 1,
        field.used,
        field.voxel,
        params.omega,
        params.light_count,
        bench.shadows.min(bench.lights),
        params.shadow_steps,
        gpu(0.0),
        gpu(0.5),
        gpu(0.95),
        middle(&recorded, 0.0),
        median,
        middle(&recorded, 0.95),
        recorded.len(),
        drawn.len(),
    );

    frames.done += 1;
    if frames.done >= bench.repeat {
        exit.write(AppExit::Success);
    }
}
