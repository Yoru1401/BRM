use bevy::{
    diagnostic::{DiagnosticPath, DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    time::common_conditions::on_timer,
};
use core::time::Duration;

use crate::sdf::field::{SdfScene, scene_distance};
use crate::sdf::render::{MainCamera, Quad, SdfMaterial};

pub(crate) struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_overlay)
            .add_systems(
                Update,
                update_stats.run_if(on_timer(Duration::from_millis(250))),
            );
    }
}

#[derive(Component)]
struct Stats;

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Stats,
        Text::default(),
        TextFont {
            font_size: FontSize::Px(20.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(8),
            ..default()
        },
    ));
}

fn update_stats(
    diagnostics: Res<DiagnosticsStore>,
    text: Single<&mut Text, With<Stats>>,
    window: Single<&Window>,
    quad: Single<(&MeshMaterial3d<SdfMaterial>, &Visibility), With<Quad>>,
    materials: Res<Assets<SdfMaterial>>,
    scene: Res<SdfScene>,
    camera: Single<&GlobalTransform, With<MainCamera>>,
) {
    fn avg(store: &DiagnosticsStore, path: &DiagnosticPath) -> f64 {
        store.get(path).and_then(|d| d.average()).unwrap_or(0.0)
    }
    let fps = avg(&diagnostics, &FrameTimeDiagnosticsPlugin::FPS);
    let ms = avg(&diagnostics, &FrameTimeDiagnosticsPlugin::FRAME_TIME);
    let (quad_material, visibility) = *quad;
    let render_params = materials
        .get(&quad_material.0)
        .map(|material| material.render_params.clone())
        .unwrap_or_default();
    let view = if render_params.debug_view == 1 {
        "steps"
    } else {
        "shaded"
    };
    let shown = match visibility {
        Visibility::Hidden => "hidden",
        _ => "shown",
    };
    let (width, height) = (
        window.resolution.physical_width(),
        window.resolution.physical_height(),
    );
    let distance_here = scene_distance(&scene.shapes, camera.translation());

    let span = render_params.bounds_max - render_params.bounds_min;
    let shapes = scene.shapes.len();
    let grid = if render_params.grid == 0 {
        "off".to_string()
    } else {
        let cells = render_params.grid_resolution;
        format!("{}x{}x{} cells", cells.x, cells.y, cells.z)
    };
    text.into_inner().0 = format!(
        "{fps:.1} fps avg\n\
         {ms:.3} ms avg\n\
         {width}x{height}\n\
         grid: {grid}\n\
         quad: {shown}  [V]\n\
         view: {view}  [H]\n\
         cpu sdf here: {distance_here:.3}\n\
         shapes: {shapes}  bounds: {:.0} x {:.0} x {:.0}",
        span.x, span.y, span.z
    );
}
