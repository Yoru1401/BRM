use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
};

use crate::command_line;
use crate::game::input::{Action, Fly};
use crate::game::physics::{Tuning, swept};
use crate::sdf::adf::Adf;
use crate::sdf::dynamic::Dynamic;
use crate::sdf::field::MATERIAL_PLAYER;
use crate::sdf::render::MainCamera;

pub(crate) fn playing() -> bool {
    command_line::flag("--play")
}

pub(crate) struct CharacterPlugin;

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        if command_line::triple("--eye").is_some() {
            app.init_resource::<Orbit>();
            return;
        }
        app.init_resource::<Orbit>()
            .add_systems(
                Update,
                spawn_character
                    .run_if(resource_changed::<Adf>)
                    .run_if(|field: Res<Adf>| field.used > 0),
            )
            .add_systems(FixedUpdate, drive_character)
            .add_systems(Update, follow_camera);
    }
}

const RADIUS: f32 = 0.5;
const HEIGHT: f32 = 1.8;

const RIDE_HEIGHT: f32 = 1.0;
const PROBE_REACH: f32 = 2.5;
const RIDE_STRENGTH: f32 = 50.0;
const RIDE_DAMPER: f32 = 5.0;

const UPRIGHT_STRENGTH: f32 = 40.0;
const UPRIGHT_DAMPER: f32 = 5.0;
const LEAN: f32 = 0.25;
const LEAN_LIMIT: f32 = 0.45;

const MAX_SPEED: f32 = 8.0;
const ACCELERATION: f32 = 200.0;
const MAX_ACCEL_FORCE: f32 = 150.0;
const TURN_FACTOR: f32 = 2.0;
const ALIGNED_FACTOR: f32 = 1.0;

const JUMP_IMPULSE: f32 = 10.0;
const RISE_GRAVITY: f32 = 5.0;
const FALL_GRAVITY: f32 = 10.0;
const LOW_JUMP: f32 = 2.5;
const JUMP_BUFFER: f32 = 0.15;
const COYOTE_TIME: f32 = 0.25;
const GROUNDED_SLACK: f32 = 1.3;

const ORBIT_DISTANCE: f32 = 7.0;
const ORBIT_HEIGHT: f32 = 1.2;
const ORBIT_MARGIN: f32 = 0.4;
const ORBIT_NEAR: f32 = 1.5;
const PITCH_LIMIT: f32 = 1.4;

#[derive(Component)]
pub(crate) struct Character {
    pub(crate) velocity: Vec3,
    pub(crate) grounded: bool,
    goal_velocity: Vec3,
    orientation: Quat,
    angular_velocity: Vec3,
    facing: Quat,
    hold_height: bool,
    was_held: bool,
    jumping: bool,
    jump_ready: bool,
    since_ungrounded: f32,
    since_jump_pressed: f32,
}

impl Default for Character {
    fn default() -> Self {
        Character {
            velocity: Vec3::ZERO,
            grounded: false,
            goal_velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            angular_velocity: Vec3::ZERO,
            facing: Quat::IDENTITY,
            hold_height: true,
            was_held: false,
            jumping: false,
            jump_ready: true,
            since_ungrounded: COYOTE_TIME,
            since_jump_pressed: JUMP_BUFFER,
        }
    }
}

#[derive(Resource)]
pub(crate) struct Orbit {
    yaw: f32,
    pitch: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Orbit {
            yaw: 0.0,
            pitch: -0.35,
        }
    }
}

fn spawn_character(mut commands: Commands, field: Res<Adf>) {
    let (low, high) = field.bounds();
    let centre = (low + high) * 0.5;
    let above = Vec3::new(centre.x, high.y, centre.z);
    let stand = match ground_below(&field, above, high.y - low.y) {
        Some(drop) => above.y - drop + RIDE_HEIGHT,
        None => high.y - RADIUS,
    };
    info!("character: standing at {:?}", Vec3::new(above.x, stand, above.z));
    commands.spawn((
        Character::default(),
        Dynamic::ball(Vec3::new(above.x, stand, above.z), RADIUS, MATERIAL_PLAYER),
        Transform::from_xyz(above.x, stand, above.z),
    ));
}

pub(crate) fn ground_below(field: &Adf, from: Vec3, reach: f32) -> Option<f32> {
    let step_floor = field.voxel * 0.05;
    let mut travelled = 0.0;
    for _ in 0..64 {
        let clearance = field.distance(from + Vec3::NEG_Y * travelled);
        if clearance < step_floor {
            return Some(travelled);
        }
        travelled += clearance.max(step_floor);
        if travelled > reach {
            return None;
        }
    }
    None
}

fn move_towards(from: Vec3, to: Vec3, most: f32) -> Vec3 {
    let delta = to - from;
    let reach = delta.length();
    match reach <= most || reach < 1e-6 {
        true => to,
        false => from + delta * (most / reach),
    }
}

fn dot_factor(alignment: f32) -> f32 {
    TURN_FACTOR + (ALIGNED_FACTOR - TURN_FACTOR) * (alignment + 1.0) * 0.5
}

fn drive_character(
    body: Single<(&mut Character, &mut Transform, &mut Dynamic)>,
    field: Res<Adf>,
    tuning: Res<Tuning>,
    actions: Res<ButtonInput<Action>>,
    orbit: Res<Orbit>,
    time: Res<Time<Fixed>>,
) {
    let step = time.delta_secs();
    if step <= 0.0 {
        return;
    }
    let (mut character, mut placement, mut shape) = body.into_inner();

    let mut wish = Vec3::ZERO;
    if actions.pressed(Action::Forward) {
        wish.z -= 1.0;
    }
    if actions.pressed(Action::Back) {
        wish.z += 1.0;
    }
    if actions.pressed(Action::Left) {
        wish.x -= 1.0;
    }
    if actions.pressed(Action::Right) {
        wish.x += 1.0;
    }

    let mut position = placement.translation;
    step_character(
        &mut character,
        &mut position,
        &field,
        tuning.gravity,
        (Quat::from_rotation_y(orbit.yaw) * wish).normalize_or_zero(),
        actions.pressed(Action::Up),
        step,
    );
    placement.translation = position;
    placement.rotation = character.orientation;

    let half = (HEIGHT - 2.0 * RADIUS) * 0.5;
    let up = character.orientation * Vec3::Y * half;
    *shape = Dynamic::rod(position - up, position + up, RADIUS, MATERIAL_PLAYER);
}

pub(crate) fn step_character(
    character: &mut Character,
    position: &mut Vec3,
    field: &Adf,
    gravity: Vec3,
    goal_direction: Vec3,
    held: bool,
    step: f32,
) {
    let hit = ground_below(field, *position, PROBE_REACH);
    character.grounded = hit.is_some_and(|reach| reach <= RIDE_HEIGHT * GROUNDED_SLACK);
    character.since_ungrounded = match character.grounded {
        true => 0.0,
        false => character.since_ungrounded + step,
    };

    character.since_jump_pressed = match held && !character.was_held {
        true => 0.0,
        false => character.since_jump_pressed + step,
    };
    character.was_held = held;

    let mut force = gravity;

    if character.velocity.y < 0.0 {
        character.hold_height = true;
        character.jump_ready = true;
        character.jumping = false;
        if !character.grounded {
            force += gravity * (FALL_GRAVITY - 1.0);
        }
    } else if character.velocity.y > 0.0 && !character.grounded {
        if character.jumping {
            force += gravity * (RISE_GRAVITY - 1.0);
        }
        if !held {
            force += gravity * (LOW_JUMP - 1.0);
        }
    }

    if character.since_jump_pressed < JUMP_BUFFER
        && character.since_ungrounded < COYOTE_TIME
        && character.jump_ready
    {
        character.jump_ready = false;
        character.hold_height = false;
        character.jumping = true;
        character.velocity.y = 0.0;
        if let Some(reach) = hit {
            position.y -= reach - RIDE_HEIGHT;
        }
        character.velocity.y += JUMP_IMPULSE;
        character.since_jump_pressed = JUMP_BUFFER;
    }

    if character.hold_height && let Some(reach) = hit {
        let offset = reach - RIDE_HEIGHT;
        let closing = -character.velocity.y;
        let spring = offset * RIDE_STRENGTH - closing * RIDE_DAMPER;
        force += -gravity + Vec3::NEG_Y * spring;
    }

    let alignment = goal_direction.dot(character.goal_velocity.normalize_or_zero());
    let quickness = ACCELERATION * dot_factor(alignment);
    character.goal_velocity = move_towards(
        character.goal_velocity,
        goal_direction * MAX_SPEED,
        quickness * step,
    );

    let mut needed = (character.goal_velocity - character.velocity) / step;
    needed.y = 0.0;
    let needed = needed.clamp_length_max(MAX_ACCEL_FORCE * dot_factor(alignment));
    force += needed;

    character.velocity += force * step;
    *position = swept(field, *position, character.velocity * step);

    let (clearance, banded) = field.probe(*position);
    if banded && clearance < RADIUS {
        let away = field.normal(*position);
        *position += away * (RADIUS - clearance);
        let into = character.velocity.dot(away);
        if into < 0.0 {
            character.velocity -= away * into;
        }
    }

    if goal_direction != Vec3::ZERO {
        character.facing = Quat::from_rotation_y(-goal_direction.x.atan2(-goal_direction.z));
    }
    let lean_axis = Vec3::Y.cross(needed).normalize_or_zero();
    let lean = (needed.length() / gravity.length().max(1e-3) * LEAN).min(LEAN_LIMIT);
    let target = Quat::from_axis_angle(lean_axis, lean) * character.facing;

    let mut to_goal = target * character.orientation.inverse();
    if to_goal.w < 0.0 {
        to_goal = -to_goal;
    }
    let (axis, angle) = to_goal.to_axis_angle();
    let torque = axis * (angle * UPRIGHT_STRENGTH) - character.angular_velocity * UPRIGHT_DAMPER;
    character.angular_velocity += torque * step;
    character.orientation =
        (Quat::from_scaled_axis(character.angular_velocity * step) * character.orientation)
            .normalize();
}

fn follow_camera(
    camera: Single<&mut Transform, (With<MainCamera>, Without<Character>)>,
    character: Single<&Transform, With<Character>>,
    field: Res<Adf>,
    actions: Res<ButtonInput<Action>>,
    motion: Res<AccumulatedMouseMotion>,
    fly: Res<Fly>,
    mut orbit: ResMut<Orbit>,
) {
    if actions.pressed(Action::Look) && motion.delta != Vec2::ZERO {
        orbit.yaw -= motion.delta.x * fly.sensitivity;
        orbit.pitch = (orbit.pitch - motion.delta.y * fly.sensitivity).clamp(-PITCH_LIMIT, 0.6);
    }
    let target = character.translation + Vec3::Y * ORBIT_HEIGHT;
    let swing = Quat::from_euler(EulerRot::YXZ, orbit.yaw, orbit.pitch, 0.0);
    let away = swing * Vec3::Z;
    let reached = (swept(&field, target, away * ORBIT_DISTANCE) - target).length();
    let back = (reached - ORBIT_MARGIN).clamp(ORBIT_NEAR, ORBIT_DISTANCE);
    *camera.into_inner() = Transform::from_translation(target + away * back).looking_at(target, Vec3::Y);
}
