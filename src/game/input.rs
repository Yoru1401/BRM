use bevy::{
    input::{InputSystems, mouse::AccumulatedMouseMotion},
    prelude::*,
};

use crate::command_line;
use crate::sdf::render::MainCamera;

pub(crate) struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bindings>()
            .init_resource::<Fly>()
            .init_resource::<ButtonInput<Action>>()
            .add_systems(PreUpdate, read_bindings.after(InputSystems))
            .add_systems(Update, fly_camera);
    }
}

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct Fly {
    pub(crate) speed: f32,
    pub(crate) sensitivity: f32,
}

impl Default for Fly {
    fn default() -> Self {
        Fly {
            speed: command_line::value("--speed").unwrap_or(MOVE_SPEED),
            sensitivity: command_line::value("--sensitivity").unwrap_or(SENSITIVITY),
        }
    }
}

const MOVE_SPEED: f32 = 5.0;
const SENSITIVITY: f32 = 0.003;
const PITCH_LIMIT: f32 = 1.55;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Action {
    Forward,
    Back,
    Left,
    Right,
    Up,
    Down,

    Look,
    ToggleDebugView,
    ToggleQuad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
}

#[derive(Resource)]
pub(crate) struct Bindings(pub(crate) Vec<(Binding, Action)>);

impl Default for Bindings {
    fn default() -> Self {
        use Action::*;
        use Binding::{Key, Mouse};
        Bindings(vec![
            (Key(KeyCode::KeyW), Forward),
            (Key(KeyCode::KeyS), Back),
            (Key(KeyCode::KeyA), Left),
            (Key(KeyCode::KeyD), Right),
            (Key(KeyCode::Space), Up),
            (Key(KeyCode::ShiftLeft), Down),
            (Mouse(MouseButton::Right), Look),
            (Key(KeyCode::KeyH), ToggleDebugView),
            (Key(KeyCode::KeyV), ToggleQuad),
        ])
    }
}

fn read_bindings(
    mut actions: ResMut<ButtonInput<Action>>,
    bindings: Res<Bindings>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    actions.clear();
    for &(binding, action) in &bindings.0 {
        let (just_pressed, just_released) = match binding {
            Binding::Key(key) => (keys.just_pressed(key), keys.just_released(key)),
            Binding::Mouse(button) => (mouse.just_pressed(button), mouse.just_released(button)),
        };
        if just_pressed {
            actions.press(action);
        }
        if just_released {
            actions.release(action);
        }
    }
}

fn fly_camera(
    camera: Single<&mut Transform, With<MainCamera>>,
    actions: Res<ButtonInput<Action>>,
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    fly: Res<Fly>,
) {
    let mut camera = camera.into_inner();

    if actions.pressed(Action::Look) && motion.delta != Vec2::ZERO {
        let (yaw, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
        camera.rotation = Quat::from_euler(
            EulerRot::YXZ,
            yaw - motion.delta.x * fly.sensitivity,
            (pitch - motion.delta.y * fly.sensitivity).clamp(-PITCH_LIMIT, PITCH_LIMIT),
            0.0,
        );
    }

    let mut direction = Vec3::ZERO;
    if actions.pressed(Action::Forward) {
        direction += *camera.forward();
    }
    if actions.pressed(Action::Back) {
        direction += *camera.back();
    }
    if actions.pressed(Action::Left) {
        direction += *camera.left();
    }
    if actions.pressed(Action::Right) {
        direction += *camera.right();
    }
    if actions.pressed(Action::Up) {
        direction.y += 1.0;
    }
    if actions.pressed(Action::Down) {
        direction.y -= 1.0;
    }
    if direction != Vec3::ZERO {
        camera.translation += direction.normalize() * fly.speed * time.delta_secs();
    }
}
