use bevy::prelude::*;

use crate::sdf::adf::{self, Adf, Surface};
use crate::sdf::solid::{Op, Shape, Solid};

const HALF: f32 = 2.0;
const BALL: f32 = 1.4;
const CENTRE: Vec3 = Vec3::new(1.6, 1.6, 0.0);

fn exact_box(point: Vec3) -> f32 {
    let corner = point.abs() - Vec3::splat(HALF);
    corner.max(Vec3::ZERO).length() + corner.max_element().min(0.0)
}

fn exact_union(point: Vec3) -> f32 {
    exact_box(point).min(point.distance(CENTRE) - BALL)
}

fn overlapping() -> (Vec<[Vec3; 3]>, Vec<u16>) {
    let mut cube = Mesh::from(Cuboid::new(HALF * 2.0, HALF * 2.0, HALF * 2.0));
    cube.duplicate_vertices();
    let (mut triangles, _) = adf::triangles_of(&cube);
    let mut parts = vec![0u16; triangles.len()];

    let mut ball = Sphere::new(BALL).mesh().ico(4).unwrap();
    ball.duplicate_vertices();
    let (ball_triangles, _) = adf::triangles_of(&ball);
    for triangle in ball_triangles {
        triangles.push(triangle.map(|corner| corner + CENTRE));
        parts.push(1);
    }
    (triangles, parts)
}

fn probe(field: &Adf, parts: &[u16]) -> (usize, f32) {
    let mut checked = 0;
    let mut worst: f32 = 0.0;
    for step in 0..400000 {
        let unit = Vec3::new(
            ((step * 7919) % 211) as f32 / 210.0,
            ((step * 6151) % 193) as f32 / 192.0,
            ((step * 3571) % 179) as f32 / 178.0,
        );
        let point = Vec3::splat(-HALF - 1.0) + unit * (2.0 * HALF + 2.0 + CENTRE.max_element());
        let exact = exact_union(point);
        if exact.abs() > field.range() {
            continue;
        }
        checked += 1;
        worst = worst.max(field.distance(point) - exact);
    }
    let _ = parts;
    (checked, worst)
}

#[test]
fn a_sphere_poking_out_of_a_box_never_overestimates_the_union() {
    let (triangles, parts) = overlapping();
    let materials = vec![0u8; triangles.len()];
    let field = adf::bake(&Surface::new(&triangles, &materials, &parts));

    let (checked, worst) = probe(&field, &parts);
    assert!(checked > 2000, "only {checked} samples landed inside the band");
    assert!(
        worst < 2.0 * field.voxel(),
        "baked field ran {worst} m above the true union, {} voxels",
        worst / field.voxel()
    );
}

#[test]
fn the_two_parts_must_be_told_apart() {
    let (triangles, parts) = overlapping();
    let materials = vec![0u8; triangles.len()];
    let soup = vec![0u16; triangles.len()];

    let split = adf::bake(&Surface::new(&triangles, &materials, &parts));
    let merged = adf::bake(&Surface::new(&triangles, &materials, &soup));

    let buried = CENTRE * 0.35;
    assert!(
        split.distance(buried) < 0.0,
        "a point inside both shapes read {} with parts",
        split.distance(buried)
    );
    assert!(
        merged.distance(buried) >= split.distance(buried) - 1e-4,
        "treating both shapes as one soup was not worse, so the part split is \
         no longer earning its place"
    );
}

fn exact_rod(point: Vec3) -> f32 {
    let local = point - CENTRE;
    let flat = Vec2::new(local.x, local.z).length() - BALL;
    let along = local.y.abs() - HALF;
    Vec2::new(flat.max(0.0), along.max(0.0)).length() + flat.max(along).min(0.0)
}

fn rod_in_a_box() -> (Vec<[Vec3; 3]>, Vec<u16>) {
    let mut cube = Mesh::from(Cuboid::new(HALF * 2.0, HALF * 2.0, HALF * 2.0));
    cube.duplicate_vertices();
    let (mut triangles, _) = adf::triangles_of(&cube);
    let mut parts = vec![0u16; triangles.len()];

    let mut rod = Cylinder::new(BALL, HALF * 2.0)
        .mesh()
        .resolution(20)
        .build();
    rod.duplicate_vertices();
    let (rod_triangles, _) = adf::triangles_of(&rod);
    for triangle in rod_triangles {
        triangles.push(triangle.map(|corner| corner + CENTRE));
        parts.push(1);
    }
    (triangles, parts)
}

#[test]
fn a_cylinder_poking_out_of_a_box_never_overestimates_the_union() {
    let (triangles, parts) = rod_in_a_box();
    let materials = vec![0u8; triangles.len()];
    let field = adf::bake(&Surface::new(&triangles, &materials, &parts));

    let mut checked = 0;
    let mut worst: f32 = 0.0;
    let mut where_worst = Vec3::ZERO;
    for step in 0..400000 {
        let unit = Vec3::new(
            ((step * 7919) % 211) as f32 / 210.0,
            ((step * 6151) % 193) as f32 / 192.0,
            ((step * 3571) % 179) as f32 / 178.0,
        );
        let point = Vec3::splat(-HALF - 1.0) + unit * (2.0 * HALF + 2.0 + CENTRE.max_element());
        let exact = exact_box(point).min(exact_rod(point));
        if exact.abs() > field.range() {
            continue;
        }
        checked += 1;
        let over = field.distance(point) - exact;
        if over > worst {
            worst = over;
            where_worst = point;
        }
    }
    assert!(checked > 2000, "only {checked} samples landed inside the band");
    assert!(
        worst < 2.0 * field.voxel(),
        "baked field ran {worst} m above the true union at {where_worst:?}, {} voxels",
        worst / field.voxel()
    );
}

#[test]
fn a_sphere_half_buried_in_a_box_survives_a_coarse_bake() {
    let (triangles, parts) = overlapping();
    let materials = vec![0u8; triangles.len()];

    for coarse in [HALF / 24.0, HALF / 12.0, HALF / 6.0] {
        let field = adf::bake_at(&Surface::new(&triangles, &materials, &parts), Some(coarse));
        let mut checked = 0;
        let mut worst: f32 = 0.0;
        let mut where_worst = Vec3::ZERO;
        for step in 0..200000 {
            let unit = Vec3::new(
                ((step * 7919) % 211) as f32 / 210.0,
                ((step * 6151) % 193) as f32 / 192.0,
                ((step * 3571) % 179) as f32 / 178.0,
            );
            let point = Vec3::splat(-HALF - 1.0) + unit * (2.0 * HALF + 2.0 + CENTRE.max_element());
            let truth = exact_union(point);
            if truth < 0.0 {
                assert!(
                    field.distance(point) < 0.0,
                    "at {:.4} m voxels a point {truth} inside the union read {} at {point:?}",
                    field.voxel(),
                    field.distance(point)
                );
                continue;
            }
            if truth > field.finest().brick_size() {
                continue;
            }
            checked += 1;
            let over = field.distance(point) - truth;
            if over > worst {
                worst = over;
                where_worst = point;
            }
        }
        println!(
            "voxel {:.4} bricks {:?} used {} checked {checked} worst {:.4} ({:.2} voxels) at {where_worst:?}",
            field.voxel(),
            field.bricks(),
            field.used,
            worst,
            worst / field.voxel()
        );
        assert!(
            worst < 2.0 * field.voxel(),
            "at {:.4} m voxels the union ran {worst} m above the truth at {where_worst:?}",
            field.voxel()
        );
    }
}

fn solid_box_and(shape: Shape) -> [Solid; 2] {
    [
        Solid::new(
            Shape::Box {
                half: Vec3::splat(HALF),
            },
            Vec3::ZERO,
            Quat::IDENTITY,
            0,
            0,
        ),
        Solid::new(shape, CENTRE, Quat::IDENTITY, 0, 1),
    ]
}

fn probe_solids(solids: &[Solid], exact: impl Fn(Vec3) -> f32) -> (usize, f32, f32) {
    let field = adf::bake(&Surface::new(&[], &[], &[]).with_solids(solids));
    let mut checked = 0;
    let mut worst: f32 = 0.0;
    for step in 0..400000 {
        let unit = Vec3::new(
            ((step * 7919) % 211) as f32 / 210.0,
            ((step * 6151) % 193) as f32 / 192.0,
            ((step * 3571) % 179) as f32 / 178.0,
        );
        let point = Vec3::splat(-HALF - 1.0) + unit * (2.0 * HALF + 2.0 + CENTRE.max_element());
        let truth = exact(point);
        if truth.abs() > field.range() {
            continue;
        }
        checked += 1;
        worst = worst.max(field.distance(point) - truth);
    }
    (checked, worst, field.voxel())
}

#[test]
fn a_solid_sphere_poking_out_of_a_solid_box_never_overestimates_the_union() {
    let solids = solid_box_and(Shape::Sphere { radius: BALL });
    let (checked, worst, voxel) = probe_solids(&solids, exact_union);
    assert!(checked > 2000, "only {checked} samples landed inside the band");
    assert!(
        worst < 2.0 * voxel,
        "baked solids ran {worst} m above the true union, {} voxels",
        worst / voxel
    );
}

#[test]
fn a_solid_cylinder_cap_edge_never_overestimates_the_union() {
    let solids = solid_box_and(Shape::Cylinder {
        radius: BALL,
        half_height: HALF,
    });
    let (checked, worst, voxel) =
        probe_solids(&solids, |point| exact_box(point).min(exact_rod(point)));
    assert!(checked > 2000, "only {checked} samples landed inside the band");
    assert!(
        worst < 2.0 * voxel,
        "baked solids ran {worst} m above the true union at a cap edge, {} voxels",
        worst / voxel
    );
}

#[test]
fn a_clipmap_of_solids_never_overestimates_at_any_level() {
    let solids = solid_box_and(Shape::Cylinder {
        radius: BALL,
        half_height: HALF,
    });
    let field = adf::bake_levels(
        &Surface::new(&[], &[], &[]).with_solids(&solids),
        None,
        3,
    );
    assert_eq!(field.levels.len(), 3, "three levels were asked for");

    let mut checked = 0;
    let mut worst: f32 = 0.0;
    for step in 0..400000 {
        let unit = Vec3::new(
            ((step * 7919) % 211) as f32 / 210.0,
            ((step * 6151) % 193) as f32 / 192.0,
            ((step * 3571) % 179) as f32 / 178.0,
        );
        let point = Vec3::splat(-HALF - 1.0) + unit * (2.0 * HALF + 2.0 + CENTRE.max_element());
        let truth = exact_box(point).min(exact_rod(point));
        if truth.abs() > field.coarsest().range() {
            continue;
        }
        checked += 1;
        worst = worst.max(field.distance(point) - truth);
    }
    assert!(checked > 2000, "only {checked} samples landed inside the band");
    assert!(
        worst < 2.0 * field.coarsest().voxel,
        "the clipmap ran {worst} m above the truth, {} coarse voxels",
        worst / field.coarsest().voxel
    );
}

#[test]
fn carving_a_sphere_out_of_a_box_leaves_the_difference() {
    let solids = [
        Solid::new(
            Shape::Box {
                half: Vec3::splat(HALF),
            },
            Vec3::ZERO,
            Quat::IDENTITY,
            0,
            0,
        ),
        Solid::new(Shape::Sphere { radius: BALL }, CENTRE, Quat::IDENTITY, 0, 1)
            .with_op(Op::Subtract),
    ];
    let carved = |point: Vec3| exact_box(point).max(-(point.distance(CENTRE) - BALL));

    let (checked, worst, voxel) = probe_solids(&solids, carved);
    assert!(checked > 2000, "only {checked} samples landed inside the band");
    assert!(
        worst < 2.0 * voxel,
        "the carved field ran {worst} m above the difference, {} voxels",
        worst / voxel
    );

    let field = adf::bake(&Surface::new(&[], &[], &[]).with_solids(&solids));
    let bitten = CENTRE - Vec3::splat(BALL * 0.3);
    assert!(
        field.distance(bitten) > 0.0,
        "the carved-out region still reads solid: {}",
        field.distance(bitten)
    );
    let kept = Vec3::new(-HALF * 0.6, -HALF * 0.6, 0.0);
    assert!(
        field.distance(kept) < 0.0,
        "the far corner of the box was carved away too: {}",
        field.distance(kept)
    );
}
