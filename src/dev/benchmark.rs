use bevy::prelude::*;

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
    done: usize,
    settled: Option<std::time::Instant>,
}

fn record(
    mut frames: ResMut<Frames>,
    time: Res<Time>,
    bench: Res<Bench>,
    field: Res<Adf>,
    quad: Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>,
    materials: Res<Assets<SdfMaterial>>,
    mut exit: MessageWriter<AppExit>,
) {
    let settled = frames.settled.get_or_insert_with(std::time::Instant::now);
    if settled.elapsed() < crate::dev::SHADER_WARMUP {
        return;
    }
    frames.times.push(time.delta_secs() * 1000.0);
    if frames.times.len() < WARMUP_FRAMES + RECORDED_FRAMES {
        return;
    }

    let Some(params) = materials.get(&quad.0).map(|m| m.render_params.clone()) else {
        return;
    };
    let mut recorded: Vec<f32> = frames.times.split_off(WARMUP_FRAMES);
    frames.times.clear();
    recorded.sort_by(f32::total_cmp);
    let at = |fraction: f32| recorded[((recorded.len() - 1) as f32 * fraction) as usize];

    let median = at(0.5);
    if (median - 1000.0 / 60.0).abs() < 0.2 {
        eprintln!("bench: median {median:.3} ms is suspiciously exactly 60 Hz - vsync?");
    }

    println!(
        "run\t{}\tbricks\t{}\tvoxel\t{:.4}\tomega\t{:.2}\tlights\t{}\tshadows\t{}\tsteps\t{}\tmin\t{:.3}\tmedian\t{:.3}\tp95\t{:.3}\tframes\t{}",
        frames.done + 1,
        field.used,
        field.voxel,
        params.omega,
        params.light_count,
        bench.shadows.min(bench.lights),
        params.shadow_steps,
        at(0.0),
        median,
        at(0.95),
        recorded.len(),
    );

    frames.done += 1;
    if frames.done >= bench.repeat {
        exit.write(AppExit::Success);
    }
}
