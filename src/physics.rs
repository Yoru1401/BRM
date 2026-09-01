//! Rigidbodies against the field.
//!
//! Bodies query the same packed shapes the renderer draws, so there is no
//! separate collision geometry to keep in step.

use bevy::prelude::*;

use crate::field::{Albedo, SdfScene, SdfShape, SphereBody, scene_distance, scene_normal};

pub(crate) struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (simulate_bodies, resolve_body_pairs, despawn_fallen_bodies)
                .chain()
                .run_if(static_field_is_ready),
        )
        .add_systems(Update, draw_body_spin)
        .add_systems(Startup, spawn_bodies);
    }
}

const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);
/// 0 = no bounce. ponytail: no restitution curve.
const RESTITUTION: f32 = 0.0;
/// Solid sphere: `I = 2/5 m r^2`.
const SPHERE_INERTIA_FACTOR: f32 = 0.4;
/// Effective mass along a contact tangent for a solid sphere is `2/7 m`, so
/// that fraction of the slip is what a perfectly gripping contact removes.
const CONTACT_TANGENT_FACTOR: f32 = 2.0 / 7.0;
/// Coulomb limit. Friction can never exceed this times the normal impulse,
/// which is what lets a ball slide on a steep ramp instead of gripping it.
pub(crate) const FRICTION_COEFFICIENT: f32 = 0.6;
/// Rolling resistance. Nothing else stops a ball rolling forever on flat floor.
const ANGULAR_DAMPING_PER_SECOND: f32 = 0.8;
/// Below this speed a touching body is parked instead of being integrated, which
/// is what stops the endless gravity-push-out jitter at rest.
const SLEEP_SPEED: f32 = 0.05;
/// A body must also be barely spinning before it parks, or a ball would freeze
/// mid-roll.
const SLEEP_SPIN: f32 = 0.15;
/// A parked body wakes once the surface under it has moved this far away.
const SLEEP_CLEARANCE: f32 = 0.02;
/// Physics bodies get their own colour so they read against imported geometry.
const BODY_ALBEDO: Vec3 = Vec3::new(0.95, 0.85, 0.25);
/// A body this far under the world is gone, not falling.
///
/// Load-bearing for the *renderer*, not for the physics. A body that misses the
/// floor never stops accelerating, and `scene_bounds` is one AABB over every
/// shape - so the scene box grows without limit, and the acceleration grid
/// derives its resolution from that box. Measured on 2026-09-01: four minutes
/// of runtime took the bounds to 50270 units tall, collapsed the grid to a
/// single cell across X and Z, and put the frame at 69 ms against 10 with a
/// healthy grid.
const KILL_BELOW: f32 = -50.0;

// ================================================================ physics

/// The scene asset loads asynchronously and the first frames are spent building
/// pipelines. Until statics exist the field reads as empty everywhere, so
/// gravity would run against nothing and drop the bodies through the world.
fn static_field_is_ready(scene: Res<SdfScene>) -> bool {
    scene.static_count > 0
}

/// Gravity, integrate, resolve against the static field.
///
/// Order matters: push out of the surface first, then kill the velocity heading
/// into it, then damp what is left sliding along it. Damping the whole velocity
/// would fight the push-out and leave bodies sinking.
fn simulate_bodies(
    mut bodies: Query<(&mut SphereBody, &mut Transform)>,
    scene: Res<SdfScene>,
    time: Res<Time<Fixed>>,
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

        body.velocity += GRAVITY * step;
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

/// Coulomb friction at a contact, returning the change to linear and angular
/// velocity. Unit mass throughout, matching the pair solver.
///
/// Friction acts at the contact point, not the centre, so it both slows the
/// body and spins it - that coupling is what turns sliding into rolling. The
/// impulse it would take to kill the slip outright is `2/7` of it for a solid
/// sphere; Coulomb caps that against the normal impulse, so a steep enough
/// slope still slides.
pub(crate) fn contact_friction(
    normal: Vec3,
    velocity: Vec3,
    angular_velocity: Vec3,
    radius: f32,
    normal_impulse: f32,
) -> (Vec3, Vec3) {
    let arm = -normal * radius;
    let contact_velocity = velocity + angular_velocity.cross(arm);
    let slip = contact_velocity - normal * contact_velocity.dot(normal);
    let Some(slip_direction) = slip.try_normalize() else {
        return (Vec3::ZERO, Vec3::ZERO);
    };

    let to_stop_slipping = CONTACT_TANGENT_FACTOR * slip.length();
    let magnitude = to_stop_slipping.min(FRICTION_COEFFICIENT * normal_impulse);
    let impulse = -slip_direction * magnitude;

    let inertia = SPHERE_INERTIA_FACTOR * radius * radius;
    (impulse, arm.cross(impulse) / inertia)
}

/// Half of what it takes to separate two overlapping spheres and stop them
/// closing. `a` gets these, `b` gets the negatives. `None` when they are apart.
///
/// ponytail: equal mass, no restitution, no friction between bodies.
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
    // Exactly coincident centres have no direction to push along; any axis will
    // do, and the next step will find a real one.
    let away_from_b = offset.try_normalize().unwrap_or(Vec3::Y);

    let closing_speed = (a_velocity - b_velocity).dot(away_from_b);
    let velocity_change = if closing_speed < 0.0 {
        away_from_b * (-closing_speed * 0.5)
    } else {
        Vec3::ZERO
    };
    Some((away_from_b * (overlap * 0.5), velocity_change))
}

/// Keeps bodies out of each other. Every pair, which is fine at this count.
/// ponytail: O(n^2) and a single pass, so a deep stack stays slightly squashed.
/// Add a broadphase and a few iterations when either becomes visible.
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
            // Something landed on them, so neither is at rest any more.
            a_body.resting = false;
            b_body.resting = false;
        }
    }
}

// ============================================================ debug draw

/// Three short axis lines per body. A sphere in an SDF looks the same however
/// it is turned, so without these there is no way to see whether it rolls.
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

/// A few spheres dropped above the origin, to watch the field push back.
/// Bodies that have left the world, removed. See [`KILL_BELOW`].
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
    // Three tests, dropped onto whatever the loaded scene puts under them:
    // a column into the bowl (do they stack, or sink into each other?),
    // a batch onto the ramp (do they queue single file down the chute?),
    // a batch onto the top step (do they bounce down and wake each other?).
    let drops = [
        (Vec3::new(0.0, 9.0, 0.0), 0.45),
        (Vec3::new(0.3, 11.0, 0.2), 0.40),
        (Vec3::new(-0.2, 13.0, 0.3), 0.50),
        (Vec3::new(0.1, 15.0, -0.3), 0.35),
        (Vec3::new(-0.3, 17.0, -0.1), 0.45),
        (Vec3::new(0.2, 19.0, 0.1), 0.40),
        (Vec3::new(-13.0, 9.0, 0.0), 0.35),
        (Vec3::new(-13.4, 11.0, 0.2), 0.30),
        (Vec3::new(-12.6, 13.0, -0.2), 0.40),
        (Vec3::new(-13.2, 15.0, 0.1), 0.35),
        (Vec3::new(17.0, 8.0, 0.0), 0.40),
        (Vec3::new(16.6, 10.0, 0.3), 0.30),
        (Vec3::new(17.4, 12.0, -0.3), 0.35),
        (Vec3::new(17.0, 14.0, 0.1), 0.45),
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
            SdfShape::Sphere,
            Albedo(BODY_ALBEDO),
            Transform::from_translation(position).with_scale(Vec3::splat(radius)),
        ));
    }
}
