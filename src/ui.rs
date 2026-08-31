//! Screen-space overlay and in-world holograms.
//!
//! The holograms are ordinary Bevy 3D entities. They share the SDF world and
//! occlude correctly only because the fragment stage writes real depth.

use bevy::{
    diagnostic::{DiagnosticPath, DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    time::common_conditions::on_timer,
};
use core::time::Duration;

use crate::field::{SdfScene, SphereBody, scene_distance};
use crate::render::{Quad, SdfMaterial};
use crate::SPAWN_EXTRAS;

pub(crate) struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_overlay)
            .add_systems(
                Update,
                (
                    face_camera,
                    animate_hologram_bar,
                    draw_hologram_text,
                    update_stats.run_if(on_timer(Duration::from_millis(250))),
                ),
            );
        if SPAWN_EXTRAS {
            app.add_systems(Startup, spawn_holograms);
        }
    }
}

/// The FPS / frame-time readout.
#[derive(Component)]
struct Stats;

/// The stats readout, top left.
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

/// Averaged FPS + frame time. Throttled - re-laying out text every frame costs
/// real CPU at 380 fps.
fn update_stats(
    diagnostics: Res<DiagnosticsStore>,
    text: Single<&mut Text, With<Stats>>,
    window: Single<&Window>,
    quad: Single<(&MeshMaterial3d<SdfMaterial>, &Visibility), With<Quad>>,
    materials: Res<Assets<SdfMaterial>>,
    scene: Res<SdfScene>,
    camera: Single<&GlobalTransform, With<Camera3d>>,
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
    // The culling box is the difference between a sky ray costing nothing and
    // costing the whole step budget, and any single stray shape sets its size.
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

/// A world-space panel: several meshes plus gizmo text, billboarded at the
/// camera. Everything here is stock Bevy - the renderer knows nothing about it,
/// which is the point of writing depth.
#[derive(Component)]
struct Hologram {
    pub(crate) title: &'static str,
}

/// The fill of the panel's gauge. Scaled on X, and shifted so it grows from the
/// left edge instead of the middle.
#[derive(Component)]
struct HologramBar;

const HOLOGRAM_PANEL: Vec2 = Vec2::new(3.0, 1.8);
const HOLOGRAM_BAR: Vec2 = Vec2::new(2.6, 0.16);
const HOLOGRAM_TEXT_SCALE: f32 = 0.22;

fn spawn_hologram_panel(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    position: Vec3,
    title: &'static str,
) {
    let mut glass = |red: f32, green: f32, blue: f32, alpha: f32| {
        materials.add(StandardMaterial {
            base_color: Color::srgba(red, green, blue, alpha),
            emissive: LinearRgba::rgb(red, green, blue) * 2.0,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })
    };
    let frame = glass(0.3, 0.9, 1.0, 0.55);
    let backplate = glass(0.05, 0.25, 0.35, 0.30);
    let track = glass(0.1, 0.4, 0.5, 0.35);
    let fill = glass(0.4, 1.0, 0.8, 0.75);

    commands.spawn((
        Hologram { title },
        Transform::from_translation(position),
        Visibility::default(),
        children![
            (
                Mesh3d(meshes.add(Rectangle::from_size(HOLOGRAM_PANEL + Vec2::splat(0.12)))),
                MeshMaterial3d(frame),
                Transform::from_xyz(0.0, 0.0, -0.02),
            ),
            (
                Mesh3d(meshes.add(Rectangle::from_size(HOLOGRAM_PANEL))),
                MeshMaterial3d(backplate),
                Transform::IDENTITY,
            ),
            (
                Mesh3d(meshes.add(Rectangle::from_size(HOLOGRAM_BAR))),
                MeshMaterial3d(track),
                Transform::from_xyz(0.0, -0.6, 0.01),
            ),
            (
                HologramBar,
                Mesh3d(meshes.add(Rectangle::from_size(HOLOGRAM_BAR))),
                MeshMaterial3d(fill),
                Transform::from_xyz(0.0, -0.6, 0.02),
            ),
        ],
    ));
}

/// Panels always face the camera. Yaw and pitch both, so a panel read from
/// below still squares up.
fn face_camera(
    camera: Single<&GlobalTransform, With<Camera3d>>,
    mut panels: Query<&mut Transform, With<Hologram>>,
) {
    let eye = camera.translation();
    for mut panel in &mut panels {
        let towards_camera = eye - panel.translation;
        if towards_camera.length_squared() > f32::EPSILON {
            panel.look_to(-towards_camera, Vec3::Y);
        }
    }
}

/// Sweeps the gauge so the panel is visibly alive, and anchors it left.
fn animate_hologram_bar(time: Res<Time>, mut bars: Query<&mut Transform, With<HologramBar>>) {
    let filled = 0.5 + 0.5 * ops::sin(time.elapsed_secs());
    for mut bar in &mut bars {
        bar.scale.x = filled.max(0.001);
        bar.translation.x = -HOLOGRAM_BAR.x * 0.5 * (1.0 - filled);
    }
}

/// Title plus a live readout, drawn as 3D text gizmos in the panel's own frame.
/// ponytail: gizmo text is redrawn every frame and has no layout. Fine for a
/// readout, wrong for a paragraph.
fn draw_hologram_text(
    mut gizmos: Gizmos,
    panels: Query<(&Hologram, &GlobalTransform)>,
    bodies: Query<&SphereBody>,
    diagnostics: Res<DiagnosticsStore>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|frames| frames.average())
        .unwrap_or(0.0);
    let asleep = bodies.iter().filter(|body| body.resting).count();
    let readout = format!(
        "{} bodies, {asleep} asleep\n{fps:.0} fps",
        bodies.iter().count()
    );

    for (hologram, placement) in &panels {
        let rotation = placement.rotation();
        let title_at = placement.translation() + rotation * Vec3::new(0.0, 0.55, 0.03);
        let readout_at = placement.translation() + rotation * Vec3::new(0.0, -0.05, 0.03);

        gizmos.text(
            Isometry3d::new(title_at, rotation),
            hologram.title,
            HOLOGRAM_TEXT_SCALE * 1.4,
            Vec2::ZERO,
            Color::srgb(0.6, 1.0, 1.0),
        );
        gizmos.text(
            Isometry3d::new(readout_at, rotation),
            &readout,
            HOLOGRAM_TEXT_SCALE,
            Vec2::ZERO,
            Color::srgb(0.4, 0.9, 0.8),
        );
    }
}

/// One rich panel plus two crude shapes that each test a different occlusion
/// case. No renderer support anywhere - all stock Bevy entities.
fn spawn_holograms(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (commands, meshes, materials) = (&mut commands, &mut *meshes, &mut *materials);
    spawn_hologram_panel(
        commands,
        meshes,
        materials,
        Vec3::new(0.0, 4.5, 0.0),
        "BOWL STATUS",
    );

    let marker = materials.add(StandardMaterial {
        base_color: Color::srgba(0.3, 0.9, 1.0, 0.35),
        emissive: LinearRgba::rgb(0.1, 0.8, 1.0),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });

    // Half buried: the floor must cut it off.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(marker.clone()),
        Transform::from_xyz(-5.0, 0.2, 2.0),
    ));

    // Behind the chute walls: visible through the gap only.
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(0.8))),
        MeshMaterial3d(marker),
        Transform::from_xyz(-8.0, 2.0, 0.0),
    ));
}
