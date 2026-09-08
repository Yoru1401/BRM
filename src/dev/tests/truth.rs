use bevy::prelude::*;

use crate::sdf::adf::{self, Adf, Surface, closest_on_triangle};
use crate::sdf::solid::{Shape, Solid};

struct World {
    triangles: Vec<[Vec3; 3]>,
    parts: Vec<u16>,
    materials: Vec<u8>,
    solids: Vec<Solid>,
}

fn hill(x: f32, z: f32) -> f32 {
    (x * 0.21).sin() * (z * 0.17).cos() * 2.4 + (x * 0.09 + z * 0.13).sin() * 1.6
}

fn worldlet() -> World {
    let mut triangles: Vec<[Vec3; 3]> = Vec::new();
    let mut parts: Vec<u16> = Vec::new();

    let side = 24usize;
    let step = 40.0 / side as f32;
    let corner = |row: usize, column: usize| {
        let x = -20.0 + column as f32 * step;
        let z = -20.0 + row as f32 * step;
        Vec3::new(x, hill(x, z), z)
    };
    let floor = -8.0;
    let mut push = |triangle: [Vec3; 3], part: u16| {
        if (triangle[1] - triangle[0])
            .cross(triangle[2] - triangle[0])
            .length_squared()
            > 1e-12
        {
            triangles.push(triangle);
            parts.push(part);
        }
    };

    for row in 0..side {
        for column in 0..side {
            let (a, b, c, d) = (
                corner(row, column),
                corner(row, column + 1),
                corner(row + 1, column + 1),
                corner(row + 1, column),
            );
            push([a, c, b], 0);
            push([a, d, c], 0);
        }
    }
    for slot in 0..side {
        let along = |row: usize, column: usize| corner(row, column);
        let walls = [
            (along(0, slot), along(0, slot + 1)),
            (along(side, slot + 1), along(side, slot)),
            (along(slot + 1, 0), along(slot, 0)),
            (along(slot, side), along(slot + 1, side)),
        ];
        for (one, two) in walls {
            let low_one = Vec3::new(one.x, floor, one.z);
            let low_two = Vec3::new(two.x, floor, two.z);
            push([one, low_one, low_two], 0);
            push([one, low_two, two], 0);
        }
    }
    let base = [
        Vec3::new(-20.0, floor, -20.0),
        Vec3::new(20.0, floor, -20.0),
        Vec3::new(20.0, floor, 20.0),
        Vec3::new(-20.0, floor, 20.0),
    ];
    push([base[0], base[2], base[1]], 0);
    push([base[0], base[3], base[2]], 0);

    let mut solids: Vec<Solid> = Vec::new();
    for slot in 0..12 {
        let turn = slot as f32 / 12.0 * std::f32::consts::TAU;
        let (x, z) = (turn.cos() * 11.0, turn.sin() * 11.0);
        let ground = hill(x, z);
        let shape = match slot % 4 {
            0 => Shape::Box {
                half: Vec3::new(2.0, 2.5, 2.0),
            },
            1 => Shape::Cylinder {
                radius: 1.8,
                half_height: 3.0,
            },
            2 => Shape::Sphere { radius: 2.6 },
            _ => Shape::Frustum {
                top: 0.7,
                bottom: 2.4,
                half_height: 2.5,
            },
        };
        solids.push(Solid::new(
            shape,
            Vec3::new(x, ground + 1.0, z),
            Quat::IDENTITY,
            0,
            slot as u16 + 1,
        ));
    }

    let materials = vec![0u8; triangles.len()];
    World {
        triangles,
        parts,
        materials,
        solids,
    }
}

fn exact(world: &World, point: Vec3) -> f32 {
    let mut best_signed = world
        .solids
        .iter()
        .map(|solid| solid.distance(point))
        .fold(f32::MAX, f32::min);
    let mut at = 0usize;
    let mut order: Vec<usize> = (0..world.triangles.len()).collect();
    order.sort_unstable_by_key(|index| world.parts[*index]);

    while at < order.len() {
        let part = world.parts[order[at]];
        let mut best = f32::MAX;
        let mut nearest = Vec3::ZERO;
        let mut pseudo = Vec3::ZERO;
        while at < order.len() && world.parts[order[at]] == part {
            let triangle = &world.triangles[order[at]];
            at += 1;
            let contact = closest_on_triangle(point, triangle);
            let reach = point.distance_squared(contact);
            let facing = (triangle[1] - triangle[0])
                .cross(triangle[2] - triangle[0])
                .normalize_or_zero();
            if (contact - nearest).length_squared() <= 1e-10 && best < f32::MAX {
                pseudo += facing;
                best = best.min(reach);
            } else if reach < best {
                best = reach;
                nearest = contact;
                pseudo = facing;
            }
        }
        let reach = best.max(0.0).sqrt();
        let signed = match (point - nearest).dot(pseudo) < 0.0 {
            true => -reach,
            false => reach,
        };
        best_signed = best_signed.min(signed);
    }
    best_signed
}

#[test]
#[ignore = "diagnostic: measures the baked field against brute force over every triangle"]
fn how_far_the_bake_strays_from_the_truth() {
    let world = worldlet();
    println!("worldlet: {} triangles, {} solids", world.triangles.len(), world.solids.len());
    let field = adf::bake(
        &Surface::new(&world.triangles, &world.materials, &world.parts)
            .with_solids(&world.solids),
    );
    println!(
        "voxel {:.4} brick {:.4} range {:.4} bricks {:?} used {}",
        field.voxel,
        field.brick_size(),
        field.range(),
        field.bricks,
        field.used
    );

    let mut over = Report::default();
    let mut under = Report::default();
    let (low, high) = field.bounds();
    for step in 0..4000 {
        let unit = Vec3::new(
            ((step * 7919) % 211) as f32 / 210.0,
            ((step * 6151) % 193) as f32 / 192.0,
            ((step * 3571) % 179) as f32 / 178.0,
        );
        let point = low + (high - low) * unit;
        let truth = exact(&world, point);
        if truth.abs() > field.range() {
            continue;
        }
        let baked = field.distance(point);
        over.see(baked - truth, point, &field);
        under.see(truth - baked, point, &field);
    }

    println!("overestimates: {}", over.tell(field.voxel));
    println!("underestimates: {}", under.tell(field.voxel));
    assert!(over.samples > 200, "only {} samples in the band", over.samples);
}

#[derive(Default)]
struct Report {
    samples: usize,
    worst: f32,
    at: Vec3,
    banded: bool,
    total: f32,
}

impl Report {
    fn see(&mut self, error: f32, point: Vec3, field: &Adf) {
        self.samples += 1;
        self.total += error.max(0.0);
        if error > self.worst {
            self.worst = error;
            self.at = point;
            self.banded = field.probe(point).1;
        }
    }

    fn tell(&self, voxel: f32) -> String {
        format!(
            "worst {:.4} m ({:.2} voxels) at {:?}, banded {}; mean {:.4} m over {} samples",
            self.worst,
            self.worst / voxel,
            self.at,
            self.banded,
            self.total / self.samples.max(1) as f32,
            self.samples
        )
    }
}
