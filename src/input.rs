//! Player input, as actions rather than keys.
//!
//! Systems ask "is [`Action::Forward`] held", never "is `KeyW` held". The
//! mapping lives in one [`Bindings`] resource, so rebinding is a resource edit
//! and nothing else moves.
//!
//! The action state is a plain `ButtonInput<Action>` - the same type Bevy uses
//! for keys - so `pressed`, `just_pressed` and `just_released` all come for
//! free, and consumers read it exactly the way they read the keyboard before.

use bevy::{
    input::{InputSystems, mouse::AccumulatedMouseMotion},
    prelude::*,
};

pub(crate) struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bindings>()
            .init_resource::<ButtonInput<Action>>()
            .add_systems(PreUpdate, read_bindings.after(InputSystems))
            .add_systems(Update, fly_camera);
    }
}

const MOVE_SPEED: f32 = 5.0;
const SENSITIVITY: f32 = 0.003;
const PITCH_LIMIT: f32 = 1.55; // just under FRAC_PI_2

/// What the game can be asked to do. Devices are somebody else's problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Action {
    Forward,
    Back,
    Left,
    Right,
    Up,
    Down,
    /// Held while looking around. The rotation itself comes from mouse motion,
    /// which is an axis, not a button.
    Look,
    ToggleDebugView,
    ToggleQuad,
}

/// One physical control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
}

/// Controls to actions, many to one. A flat list, walked once a frame:
/// ponytail: linear scan, fine at ten bindings. Map it when there are hundreds.
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

/// Turns whatever the devices did this frame into action state.
///
/// Runs in `PreUpdate` after Bevy's own input collection, so everything in
/// `Update` sees a state that is already current.
fn read_bindings(
    mut actions: ResMut<ButtonInput<Action>>,
    bindings: Res<Bindings>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    actions.clear(); // just_pressed / just_released are per-frame
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

/// Hold [`Action::Look`] and drag = look. Movement actions fly the camera.
fn fly_camera(
    camera: Single<&mut Transform, With<Camera3d>>,
    actions: Res<ButtonInput<Action>>,
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
) {
    let mut camera = camera.into_inner();

    if actions.pressed(Action::Look) && motion.delta != Vec2::ZERO {
        let (yaw, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
        camera.rotation = Quat::from_euler(
            EulerRot::YXZ,
            yaw - motion.delta.x * SENSITIVITY,
            (pitch - motion.delta.y * SENSITIVITY).clamp(-PITCH_LIMIT, PITCH_LIMIT),
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
        camera.translation += direction.normalize() * MOVE_SPEED * time.delta_secs();
    }
}
