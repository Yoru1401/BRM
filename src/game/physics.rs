use bevy::prelude::*;

use crate::command_line;
use crate::sdf::brush::{Albedo, Brush, Modifiers, SphereBody};
use crate::sdf::field::{SdfScene, scene_distance, scene_normal};

pub(crate) struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tuning>()
            .add_systems(
                FixedUpdate,
                (simulate_bodies, resolve_body_pairs, despawn_fallen_bodies)
                    .chain()
                    .run_if(static_field_is_ready),
            )
            .add_systems(Update, draw_body_spin)
            .add_systems(Startup, spawn_bodies);
    }
}

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct Tuning {
    pub(crate) gravity: Vec3,
    pub(crate) friction: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            gravity: command_line::value("--gravity")
                .map_or(GRAVITY, |pull| Vec3::new(0.0, -pull, 0.0)),
            friction: command_line::value("--friction").unwrap_or(FRICTION_COEFFICIENT),
        }
    }
}

const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);

const RESTITUTION: f32 = 0.0;

const SPHERE_INERTIA_FACTOR: f32 = 0.4;

const CONTACT_TANGENT_FACTOR: f32 = 2.0 / 7.0;

pub(crate) const FRICTION_COEFFICIENT: f32 = 0.6;

const ANGULAR_DAMPING_PER_SECOND: f32 = 0.8;

const SLEEP_SPEED: f32 = 0.05;

const SLEEP_SPIN: f32 = 0.15;

const SLEEP_CLEARANCE: f32 = 0.02;

const BODY_ALBEDO: Vec3 = Vec3::new(0.95, 0.85, 0.25);

const KILL_BELOW: f32 = -50.0;

fn static_field_is_ready(scene: Res<SdfScene>) -> bool {
    scene.static_count > 0
}

fn simulate_bodies(
    mut bodies: Query<(&mut SphereBody, &mut Transform)>,
    scene: Res<SdfScene>,
    time: Res<Time<Fixed>>,
    tuning: Res<Tuning>,
) {
    let step = time.delta_secs();
    let statics = scene.static_shapes();

    for (mut body, mut placement) in &mut bodies {
        if body.resting {
            let clearance = scene_distance(statics, placement.translation) - body.radius;
            if clearance <= SLEEP_CLEARANCE {
                continue;
            }
            body.resting = false;
        }

        body.velocity += tuning.gravity * step;
        placement.translation += body.velocity * step;
        if body.angular_velocity != Vec3::ZERO {
            body.orientation = (Quat::from_scaled_axis(body.angular_velocity * step)
                * body.orientation)
                .normalize();
        }

        let penetration = body.radius - scene_distance(statics, placement.translation);
        if penetration <= 0.0 {
            continue;
        }
        let normal = scene_normal(statics, placement.translation);
        placement.translation += normal * penetration;

        let speed_into_surface = body.velocity.dot(normal);
        let normal_impulse = (-speed_into_surface).max(0.0);
        if speed_into_surface < 0.0 {
            body.velocity -= normal * speed_into_surface * (1.0 + RESTITUTION);
        }

        let (velocity_change, spin_change) = contact_friction(
            normal,
            body.velocity,
            body.angular_velocity,
            body.radius,
            normal_impulse,
            tuning.friction,
        );
        body.velocity += velocity_change;
        body.angular_velocity += spin_change;
        body.angular_velocity *= (1.0 - ANGULAR_DAMPING_PER_SECOND * step).max(0.0);

        if body.velocity.length() < SLEEP_SPEED && body.angular_velocity.length() < SLEEP_SPIN {
            body.velocity = Vec3::ZERO;
            body.angular_velocity = Vec3::ZERO;
            body.resting = true;
        }
    }
}

pub(crate) fn contact_friction(
    normal: Vec3,
    velocity: Vec3,
    angular_velocity: Vec3,
    radius: f32,
    normal_impulse: f32,
    coefficient: f32,
) -> (Vec3, Vec3) {
    let arm = -normal * radius;
    let contact_velocity = velocity + angular_velocity.cross(arm);
    let slip = contact_velocity - normal * contact_velocity.dot(normal);
    let Some(slip_direction) = slip.try_normalize() else {
        return (Vec3::ZERO, Vec3::ZERO);
    };

    let to_stop_slipping = CONTACT_TANGENT_FACTOR * slip.length();
    let magnitude = to_stop_slipping.min(coefficient * normal_impulse);
    let impulse = -slip_direction * magnitude;

    let inertia = SPHERE_INERTIA_FACTOR * radius * radius;
    (impulse, arm.cross(impulse) / inertia)
}

pub(crate) fn sphere_pair_correction(
    a_position: Vec3,
    a_radius: f32,
    a_velocity: Vec3,
    b_position: Vec3,
    b_radius: f32,
    b_velocity: Vec3,
) -> Option<(Vec3, Vec3)> {
    let offset = a_position - b_position;
    let gap = offset.length();
    let overlap = a_radius + b_radius - gap;
    if overlap <= 0.0 {
        return None;
    }

    let away_from_b = offset.try_normalize().unwrap_or(Vec3::Y);

    let closing_speed = (a_velocity - b_velocity).dot(away_from_b);
    let velocity_change = if closing_speed < 0.0 {
        away_from_b * (-closing_speed * 0.5)
    } else {
        Vec3::ZERO
    };
    Some((away_from_b * (overlap * 0.5), velocity_change))
}

fn resolve_body_pairs(mut bodies: Query<(&mut SphereBody, &mut Transform)>) {
    let mut handles: Vec<(Mut<SphereBody>, Mut<Transform>)> = bodies.iter_mut().collect();

    for first in 0..handles.len() {
        for second in (first + 1)..handles.len() {
            let (left, right) = handles.split_at_mut(second);
            let (a_body, a_placement) = &mut left[first];
            let (b_body, b_placement) = &mut right[0];

            let Some((separation, velocity_change)) = sphere_pair_correction(
                a_placement.translation,
                a_body.radius,
                a_body.velocity,
                b_placement.translation,
                b_body.radius,
                b_body.velocity,
            ) else {
                continue;
            };

            a_placement.translation += separation;
            b_placement.translation -= separation;
            a_body.velocity += velocity_change;
            b_body.velocity -= velocity_change;

            a_body.resting = false;
            b_body.resting = false;
        }
    }
}

fn draw_body_spin(mut gizmos: Gizmos, bodies: Query<(&SphereBody, &Transform)>) {
    for (body, placement) in &bodies {
        let reach = body.radius * 1.15;
        for (axis, colour) in [
            (Vec3::X, Color::srgb(1.0, 0.2, 0.2)),
            (Vec3::Y, Color::srgb(0.2, 1.0, 0.2)),
            (Vec3::Z, Color::srgb(0.3, 0.4, 1.0)),
        ] {
            let spoke = body.orientation * axis * reach;
            gizmos.line(
                placement.translation - spoke,
                placement.translation + spoke,
                colour,
            );
        }
    }
}

pub(crate) fn despawn_fallen_bodies(
    mut commands: Commands,
    fallen: Query<(Entity, &Transform), With<SphereBody>>,
) {
    for (entity, placement) in &fallen {
        if placement.translation.y < KILL_BELOW {
            commands.entity(entity).despawn();
        }
    }
}

fn spawn_bodies(mut commands: Commands) {
    let drops = [
        (Vec3::new(-3.00, 4.0, 2.00), 0.30),
        (Vec3::new(-2.85, 5.5, 2.10), 0.28),
        (Vec3::new(-3.15, 7.0, 1.90), 0.32),
        (Vec3::new(-3.00, 8.5, 2.15), 0.26),
        (Vec3::new(-2.90, 10.0, 1.85), 0.30),
        (Vec3::new(-3.10, 11.5, 2.00), 0.28),
        (Vec3::new(0.00, 5.0, -3.00), 0.35),
        (Vec3::new(0.15, 6.5, -2.90), 0.30),
        (Vec3::new(-0.10, 8.0, -3.10), 0.38),
        (Vec3::new(0.05, 9.5, -3.00), 0.32),
        (Vec3::new(3.00, 5.0, -3.00), 0.40),
        (Vec3::new(3.20, 6.5, -2.85), 0.35),
        (Vec3::new(2.80, 8.0, -3.20), 0.45),
        (Vec3::new(3.05, 9.5, -3.00), 0.38),
    ];
    for (position, radius) in drops {
        commands.spawn((
            SphereBody {
                radius,
                velocity: Vec3::ZERO,
                angular_velocity: Vec3::ZERO,
                orientation: Quat::IDENTITY,
                resting: false,
            },
            Brush,
            Modifiers {
                round: 1.0,
                ..default()
            },
            Albedo(BODY_ALBEDO),
            Transform::from_translation(position).with_scale(Vec3::splat(radius)),
        ));
    }
}
