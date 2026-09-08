use bevy::prelude::*;

use crate::sdf::adf::{self, Adf, Surface};

fn cube(half: f32) -> Vec<[Vec3; 3]> {
    let mut mesh = Mesh::from(Cuboid::from_length(half * 2.0));
    mesh.duplicate_vertices();
    adf::triangles_of(&mesh).0
}

fn exact_box(point: Vec3, half: f32) -> f32 {
    let corner = point.abs() - Vec3::splat(half);
    corner.max(Vec3::ZERO).length() + corner.max_element().min(0.0)
}

fn baked(half: f32) -> Adf {
    let baked = adf::bake(&Surface::new(&cube(half), &[], &[]));
    assert!(baked.used > 0, "a cube baked into no bricks at all");
    baked
}

#[test]
fn the_bake_never_overestimates_the_distance() {
    const HALF: f32 = 1.0;
    let field = baked(HALF);
    let (low, high) = field.bounds();

    let mut agreed = 0;
    for step in 0..4096 {
        let unit = Vec3::new(
            ((step * 7919) % 97) as f32 / 96.0,
            ((step * 6151) % 89) as f32 / 88.0,
            ((step * 3571) % 83) as f32 / 82.0,
        );
        let point = low + (high - low) * unit;
        let exact = exact_box(point, HALF);
        if exact < -field.range() {
            continue;
        }
        let sampled = field.distance(point);

        assert!(
            sampled <= exact + 1e-4,
            "overestimated at {point:?}: {sampled} against {exact}"
        );
        if exact < field.range() && (sampled - exact).abs() < 2.0 * field.voxel {
            agreed += 1;
        }
    }
    assert!(agreed > 200, "only {agreed} samples tracked the true field");
}

#[test]
fn the_inside_of_a_cube_reads_negative() {
    let field = baked(1.0);
    for point in [Vec3::ZERO, Vec3::splat(0.4), Vec3::new(-0.6, 0.2, 0.5)] {
        assert!(
            field.distance(point) < 0.0,
            "{point:?} is inside the cube but read {}",
            field.distance(point)
        );
    }
    for point in [Vec3::new(2.0, 0.0, 0.0), Vec3::splat(1.6)] {
        assert!(
            field.distance(point) > 0.0,
            "{point:?} is outside the cube but read {}",
            field.distance(point)
        );
    }
}

#[test]
fn the_normal_points_out_of_the_nearest_face() {
    let field = baked(1.0);
    let normal = field.normal(Vec3::new(0.0, 1.0 + field.voxel, 0.0));
    assert!(
        normal.dot(Vec3::Y) > 0.9,
        "normal above the top face was {normal:?}"
    );
}

#[test]
fn empty_bricks_still_bound_the_step() {
    let field = baked(1.0);
    let (low, _) = field.bounds();
    let corner = low + Vec3::splat(field.voxel * 0.5);
    let sampled = field.distance(corner);
    assert!(
        sampled > 0.0 && sampled <= exact_box(corner, 1.0) + 1e-4,
        "corner of the volume read {sampled}"
    );
}

fn torus(major: f32, minor: f32, rings: usize) -> Vec<[Vec3; 3]> {
    let mut mesh = Torus::new(major - minor, major + minor)
        .mesh()
        .major_resolution(rings * 2)
        .minor_resolution(rings)
        .build();
    mesh.duplicate_vertices();
    adf::triangles_of(&mesh).0
}

fn exact_torus(point: Vec3, major: f32, minor: f32) -> f32 {
    Vec2::new(Vec2::new(point.x, point.z).length() - major, point.y).length() - minor
}

fn exact_torus_normal(point: Vec3, major: f32, minor: f32) -> Vec3 {
    let step = 1e-4;
    Vec3::new(
        exact_torus(point + Vec3::X * step, major, minor)
            - exact_torus(point - Vec3::X * step, major, minor),
        exact_torus(point + Vec3::Y * step, major, minor)
            - exact_torus(point - Vec3::Y * step, major, minor),
        exact_torus(point + Vec3::Z * step, major, minor)
            - exact_torus(point - Vec3::Z * step, major, minor),
    )
    .normalize()
}

#[test]
#[ignore = "diagnostic: prints how much of the normal error is the mesh's own faceting"]
fn how_much_of_the_normal_error_is_the_mesh() {
    const MAJOR: f32 = 4.0 / 3.0;
    const MINOR: f32 = 2.0 / 3.0;

    let surface: Vec<Vec3> = (0..2000)
        .map(|step| {
            let around = step as f32 * 2.399963;
            let round = step as f32 * 1.234567;
            let ring = Vec3::new(around.cos(), 0.0, around.sin());
            ring * (MAJOR + MINOR * round.cos()) + Vec3::Y * MINOR * round.sin()
        })
        .collect();

    for rings in [24usize, 48, 96] {
        let started = std::time::Instant::now();
        let field = adf::bake(&Surface::new(&torus(MAJOR, MINOR, rings), &[], &[]));
        let baked = started.elapsed().as_secs_f32();
        let mut worst: f32 = 0.0;
        let mut total = 0.0;
        for point in &surface {
            let step = field.voxel;
            let read = TETRAHEDRON
                .iter()
                .map(|corner| *corner * field.distance(*point + *corner * step))
                .sum::<Vec3>()
                .normalize_or_zero();
            let angle = read
                .dot(exact_torus_normal(*point, MAJOR, MINOR))
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees();
            worst = worst.max(angle);
            total += angle;
        }
        println!(
            "{rings} rings: mean {:.2} deg, worst {worst:.2} deg, {} bricks, baked in {baked:.2} s",
            total / surface.len() as f32,
            field.used
        );
    }
}

const TETRAHEDRON: [Vec3; 4] = [
    Vec3::new(1.0, -1.0, -1.0),
    Vec3::new(-1.0, -1.0, 1.0),
    Vec3::new(-1.0, 1.0, -1.0),
    Vec3::new(1.0, 1.0, 1.0),
];
