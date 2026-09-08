use bevy::prelude::*;

use crate::game::physics::{SphereBody, contact_friction, swept};
use crate::sdf::adf::{self, Adf, BIAS_VOXELS, Surface};

const STEP: f32 = 1.0 / 256.0;
const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);
const TOP: f32 = 2.0;

fn slab() -> Adf {
    let mut mesh = Mesh::from(Cuboid::new(24.0, 4.0, 24.0));
    mesh.duplicate_vertices();
    let mut triangles = adf::triangles_of(&mesh).0;
    for triangle in triangles.iter_mut() {
        for corner in triangle.iter_mut() {
            corner.y += TOP - 2.0;
        }
    }
    adf::bake(&Surface::new(&triangles, &[], &[]))
}

fn drop_ball(field: &Adf, radius: f32, from: f32, ticks: usize) -> (Vec3, Vec3) {
    let mut body = SphereBody {
        radius,
        ..default()
    };
    let mut position = Vec3::new(0.0, from, 0.0);
    for _ in 0..ticks {
        body.velocity += GRAVITY * STEP;
        position = swept(field, position, body.velocity * STEP);

        let (clearance, banded) = field.probe(position);
        let penetration = radius - clearance;
        if penetration <= 0.0 || !banded {
            continue;
        }
        let normal = field.normal(position);
        position += normal * penetration;
        let into = body.velocity.dot(normal);
        if into < 0.0 {
            body.velocity -= normal * into;
        }
        let (slowing, _) = contact_friction(
            normal,
            body.velocity,
            Vec3::ZERO,
            radius,
            (-into).max(0.0),
            0.6,
        );
        body.velocity += slowing;
    }
    (position, body.velocity)
}

#[test]
fn a_dropped_sphere_rests_one_radius_above_the_slab() {
    let field = slab();
    let radius = field.range() * 0.9;
    let (position, velocity) = drop_ball(&field, radius, TOP + 6.0, 2000);

    let hover = position.y - TOP;
    let bias = field.voxel() * BIAS_VOXELS;
    assert!(
        (hover - radius - bias).abs() < field.voxel(),
        "rested {hover} above the slab; wanted radius {radius} plus the {bias} bake bias"
    );
    assert!(
        velocity.length() < 0.5,
        "still moving at {:?} after eight seconds",
        velocity
    );
}

