use bevy::{
    diagnostic::{DiagnosticPath, DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    time::common_conditions::on_timer,
};
use core::time::Duration;

use crate::sdf::adf::Adf;
use crate::sdf::render::{MainCamera, Quad, SdfMaterial};
use crate::sdf::scenes;

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
    field: Res<Adf>,
    camera: Single<&GlobalTransform, With<MainCamera>>,
) {
    fn avg(store: &DiagnosticsStore, path: &DiagnosticPath) -> f64 {
        store.get(path).and_then(|d| d.average()).unwrap_or(0.0)
    }
    let fps = avg(&diagnostics, &FrameTimeDiagnosticsPlugin::FPS);
    let ms = avg(&diagnostics, &FrameTimeDiagnosticsPlugin::FRAME_TIME);
    let (quad_material, visibility) = *quad;
    let view = match materials.get(&quad_material.0) {
        Some(material) if material.render_params.debug_view == 1 => "steps",
        _ => "shaded",
    };
    let shown = match visibility {
        Visibility::Hidden => "hidden",
        _ => "shown",
    };
    let (width, height) = (
        window.resolution.physical_width(),
        window.resolution.physical_height(),
    );
    let here = field.distance(camera.translation());
    let bricks = field.bricks();
    let (low, high) = field.bounds();
    let span = high - low;

    text.into_inner().0 = format!(
        "{fps:.1} fps avg\n\
         {ms:.3} ms avg\n\
         {width}x{height}\n\
         page: {}x{}x{} bricks, {} filled\n\
         voxel: {:.4} m\n\
         quad: {shown}  [V]\n\
         view: {view}  [H]\n\
         cpu sdf here: {here:.3}\n\
         bounds: {:.1} x {:.1} x {:.1}
{}",
        bricks.x,
        bricks.y,
        bricks.z,
        field.used,
        field.voxel(),
        span.x,
        span.y,
        span.z,
        scenes::chosen().legend()
    );
}
