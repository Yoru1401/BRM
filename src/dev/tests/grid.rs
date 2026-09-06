use bevy::prelude::*;

use super::helpers::*;
use crate::sdf::bounds::*;
use crate::sdf::brush::*;
use crate::sdf::field::*;
use crate::sdf::grid::*;

#[test]
fn over_relaxation_never_marches_past_a_surface() {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / 16777216.0
    };
    macro_rules! spread {
        ($scale:expr) => {
            (next() - 0.5) * 2.0 * $scale
        };
    }

    let mut relaxed_steps = 0u32;
    let mut plain_steps = 0u32;
    let mut compared = 0u32;

    for _ in 0..60 {
        let shapes: Vec<GpuShape> = (0..2 + (next() * 6.0) as usize)
            .map(|_| {
                let placement = Transform {
                    translation: Vec3::new(spread!(6.0), spread!(6.0), spread!(6.0)),
                    rotation: Quat::from_euler(
                        EulerRot::XYZ,
                        spread!(3.14),
                        spread!(3.14),
                        spread!(3.14),
                    ),
                    scale: Vec3::new(0.3 + next() * 1.5, 0.3 + next() * 1.5, 0.3 + next() * 1.5),
                };
                let operation = CsgOperation {
                    radius: next() * 0.5,
                    ..default()
                };
                pack_brush(
                    &GlobalTransform::from(placement),
                    Some(&Modifiers::default()),
                    Some(&operation),
                    None,
                )
            })
            .collect();

        for _ in 0..40 {
            let origin = Vec3::new(spread!(14.0), spread!(14.0), spread!(14.0));
            let direction = (Vec3::new(spread!(1.0), spread!(1.0), spread!(1.0))
                - origin.normalize_or_zero() * 0.0)
                .normalize_or_zero();
            if direction == Vec3::ZERO || scene_distance(&shapes, origin) < 0.0 {
                continue;
            }

            let evaluate = |point| scene_distance(&shapes, point);
            let (plain, plain_cost) =
                march(&evaluate, &evaluate, origin, direction, 1.0, 0.001, 512);
            let (relaxed, relaxed_cost) =
                march(&evaluate, &evaluate, origin, direction, 1.2, 0.001, 512);

            assert!(
                relaxed <= plain + 0.05,
                "relaxed march ran {relaxed} past the plain hit at {plain}"
            );
            plain_steps += plain_cost;
            relaxed_steps += relaxed_cost;
            compared += 1;
        }
    }

    println!("plain {plain_steps} steps, relaxed {relaxed_steps} over {compared} rays");
    assert!(compared > 500, "only {compared} rays actually ran");

    assert!(
        relaxed_steps < plain_steps,
        "relaxed spent {relaxed_steps} steps against plain's {plain_steps}"
    );
}

#[test]
fn the_grid_never_reports_more_than_the_exact_field() {
    let mut state = 0xD1B5_4A32_D192_ED03u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / 16777216.0
    };
    macro_rules! spread {
        ($scale:expr) => {
            (next() - 0.5) * 2.0 * $scale
        };
    }

    for resolution in [1, 4, 16] {
        for _ in 0..40 {
            let shapes: Vec<GpuShape> = (0..2 + (next() * 8.0) as usize)
                .map(|_| {
                    let placement = Transform {
                        translation: Vec3::new(spread!(8.0), spread!(8.0), spread!(8.0)),
                        rotation: Quat::from_euler(
                            EulerRot::XYZ,
                            spread!(3.14),
                            spread!(3.14),
                            spread!(3.14),
                        ),
                        scale: Vec3::new(
                            0.3 + next() * 2.0,
                            0.3 + next() * 2.0,
                            0.3 + next() * 2.0,
                        ),
                    };
                    let operation = CsgOperation {
                        mode: (next() * 9.0) as u32,
                        chamfer: next() < 0.5,
                        radius: next() * 0.6,
                        strength: next() * 0.5,
                    };
                    pack_brush(
                        &GlobalTransform::from(placement),
                        Some(&Modifiers::default()),
                        Some(&operation),
                        None,
                    )
                })
                .collect();

            let (bounds_min, bounds_max) = scene_bounds(&shapes);
            let grid = build_grid(
                &shapes,
                GridWindow::covering(bounds_min, bounds_max, resolution),
            );

            for _ in 0..60 {
                let point = Vec3::new(spread!(12.0), spread!(12.0), spread!(12.0));
                let exact = scene_distance(&shapes, point);
                let gridded =
                    scene_distance_gridded(&shapes, &grid, &SkipPyramid::default(), point);
                assert!(
                    gridded <= exact + 1e-4,
                    "grid at resolution {resolution} reported {gridded} where the field is \
                     {exact}, at {point:?}"
                );
            }
        }
    }
}

#[test]
fn a_gridded_march_hits_what_the_exact_one_hits() {
    let mut state = 0x1234_5678_9ABC_DEF1u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / 16777216.0
    };
    macro_rules! spread {
        ($scale:expr) => {
            (next() - 0.5) * 2.0 * $scale
        };
    }

    let mut compared = 0;
    let mut exact_steps = 0u32;
    let mut grid_steps = 0u32;

    for _ in 0..40 {
        let shapes: Vec<GpuShape> = (0..3 + (next() * 8.0) as usize)
            .map(|_| {
                let placement = Transform {
                    translation: Vec3::new(spread!(9.0), spread!(9.0), spread!(9.0)),
                    rotation: Quat::from_euler(
                        EulerRot::XYZ,
                        spread!(3.14),
                        spread!(3.14),
                        spread!(3.14),
                    ),
                    scale: Vec3::new(0.4 + next() * 1.6, 0.4 + next() * 1.6, 0.4 + next() * 1.6),
                };
                let operation = CsgOperation {
                    radius: next() * 0.5,
                    ..default()
                };
                pack_brush(
                    &GlobalTransform::from(placement),
                    Some(&Modifiers::default()),
                    Some(&operation),
                    None,
                )
            })
            .collect();

        let (bounds_min, bounds_max) = scene_bounds(&shapes);
        let grid = build_grid(&shapes, GridWindow::covering(bounds_min, bounds_max, 16));
        let exact = |point| scene_distance(&shapes, point);
        let gridded =
            |point| scene_distance_gridded(&shapes, &grid, &SkipPyramid::default(), point);

        for _ in 0..40 {
            let origin = Vec3::new(spread!(16.0), spread!(16.0), spread!(16.0));
            let direction = Vec3::new(spread!(1.0), spread!(1.0), spread!(1.0)).normalize_or_zero();
            if direction == Vec3::ZERO || exact(origin) < 0.0 {
                continue;
            }

            let (hit, cost) = march(&exact, &exact, origin, direction, 1.2, 0.05, 512);
            let (grid_hit, grid_cost) = march(&gridded, &exact, origin, direction, 1.2, 0.05, 512);
            assert!(
                (hit - grid_hit).abs() < 0.05,
                "gridded march stopped at {grid_hit}, exact at {hit}"
            );
            exact_steps += cost;
            grid_steps += grid_cost;
            compared += 1;
        }
    }

    assert!(compared > 300, "only {compared} rays actually ran");
    println!("exact {exact_steps} steps, gridded {grid_steps} over {compared} rays");
}

#[test]
fn a_long_ray_inside_the_grid_still_arrives() {
    const SHADER_BUDGET: u32 = 128;

    let cube_at = |position: Vec3| {
        shaped(
            Transform {
                translation: position,
                scale: Vec3::splat(0.8),
                ..default()
            },
            Modifiers::default(),
        )
    };

    let anchors = [
        cube_at(Vec3::new(-20.0, 0.0, -20.0)),
        cube_at(Vec3::new(20.0, 0.0, 20.0)),
    ];
    let (bounds_min, bounds_max) = scene_bounds(&anchors);
    let planes = build_grid(&anchors, GridWindow::covering(bounds_min, bounds_max, 16));

    for step in 2..planes.resolution.x - 2 {
        for nudge in [0.0f32, 0.002, -0.002] {
            let x = planes.origin.x + planes.cell_size.x * step as f32 + nudge;
            let mut shapes = anchors.to_vec();
            shapes.push(cube_at(Vec3::new(x, 0.0, -15.0)));

            let grid = build_grid(&shapes, GridWindow::covering(bounds_min, bounds_max, 16));
            let exact = |point| scene_distance(&shapes, point);
            let gridded =
                |point| scene_distance_gridded(&shapes, &grid, &SkipPyramid::default(), point);

            let origin = Vec3::new(x, 0.0, 18.0);
            let direction = Vec3::new(0.0, 0.0, -1.0);
            let (hit, _) = march(&exact, &exact, origin, direction, 1.2, 0.01, SHADER_BUDGET);
            let (grid_hit, cost) = march(
                &gridded,
                &exact,
                origin,
                direction,
                1.2,
                0.01,
                SHADER_BUDGET,
            );

            assert!(hit < 40.0, "the exact march missed its own target at x {x}");
            assert!(
                (hit - grid_hit).abs() < 0.1,
                "at x {x}: exact stopped at {hit}, gridded at {grid_hit} after {cost} steps"
            );
        }
    }
}

fn body_at(position: Vec3, radius: f32) -> GpuShape {
    shaped(
        Transform::from_translation(position).with_scale(Vec3::splat(radius)),
        Modifiers {
            round: 1.0,
            ..default()
        },
    )
}

fn a_room() -> Vec<GpuShape> {
    [
        Vec3::new(0.0, -2.0, 0.0),
        Vec3::new(-6.0, 1.0, 0.0),
        Vec3::new(6.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, -6.0),
    ]
    .into_iter()
    .map(|position| {
        shaped(
            Transform::from_translation(position).with_scale(Vec3::new(6.0, 1.0, 6.0)),
            Modifiers::default(),
        )
    })
    .collect()
}

#[test]
fn a_body_outside_the_grid_still_lowers_the_field() {
    let statics = a_room();
    let mut shapes = statics.clone();
    shapes.push(body_at(Vec3::new(0.0, 3.0, 0.0), 0.8));

    let (bounds_min, bounds_max) = scene_bounds(&statics);
    let grid = build_grid(&statics, GridWindow::covering(bounds_min, bounds_max, 16));
    assert_eq!(grid.indexed, statics.len());

    let beside_the_body = Vec3::new(0.0, 4.4, 0.0);
    let exact = scene_distance(&shapes, beside_the_body);
    let gridded = scene_distance_gridded(&shapes, &grid, &SkipPyramid::default(), beside_the_body);

    assert!(
        exact < 0.7,
        "the probe point should be near the body, field is {exact}"
    );
    assert!(
        gridded <= exact + 1e-4,
        "grid reported {gridded} where the field is {exact}: the body was never folded",
    );
}

#[test]
fn a_moving_body_leaves_the_static_grid_untouched() {
    let statics = a_room();
    let (bounds_min, bounds_max) = scene_bounds(&statics);
    let before = build_grid(&statics, GridWindow::covering(bounds_min, bounds_max, 16));

    for height in [3.0, 2.0, 1.0, 0.5] {
        let mut shapes = statics.clone();
        shapes.push(body_at(Vec3::new(0.3, height, -0.4), 0.8));
        let after = build_grid(
            &shapes[..statics.len()],
            GridWindow::covering(bounds_min, bounds_max, 16),
        );
        assert_eq!(before, after, "the grid moved when only a body did");
    }
}

#[test]
fn bodies_outside_the_grid_never_report_more_than_the_exact_field() {
    let mut state = 0x51ED_2701_A93C_0F17u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / 16777216.0
    };
    macro_rules! spread {
        ($scale:expr) => {
            (next() - 0.5) * 2.0 * $scale
        };
    }

    let mut near_a_body = 0;
    for _ in 0..40 {
        let statics = a_room();
        let mut shapes = statics.clone();
        for _ in 0..1 + (next() * 4.0) as usize {
            shapes.push(body_at(
                Vec3::new(spread!(5.0), 0.5 + next() * 4.0, spread!(5.0)),
                0.3 + next() * 0.6,
            ));
        }

        let (bounds_min, bounds_max) = scene_bounds(&statics);
        let grid = build_grid(&statics, GridWindow::covering(bounds_min, bounds_max, 16));

        for _ in 0..80 {
            let point = Vec3::new(spread!(5.0), 0.5 + next() * 4.5, spread!(5.0));
            let exact = scene_distance(&shapes, point);
            let gridded = scene_distance_gridded(&shapes, &grid, &SkipPyramid::default(), point);
            assert!(
                gridded <= exact + 1e-4,
                "grid reported {gridded} where the field is {exact}, at {point:?}",
            );
            if exact < scene_distance(&statics, point) - 1e-3 {
                near_a_body += 1;
            }
        }
    }

    assert!(
        near_a_body > 200,
        "only {near_a_body} samples were nearer a body than the statics, so the assertion \
         above could pass without the bodies ever mattering",
    );
}

#[test]
fn a_march_past_bodies_outside_the_grid_hits_what_the_exact_one_hits() {
    let statics = a_room();
    let mut shapes = statics.clone();
    shapes.push(body_at(Vec3::new(0.0, 2.5, 0.0), 1.0));
    shapes.push(body_at(Vec3::new(-2.5, 2.0, 1.5), 0.7));

    let (bounds_min, bounds_max) = scene_bounds(&statics);
    let grid = build_grid(&statics, GridWindow::covering(bounds_min, bounds_max, 16));
    let exact = |point| scene_distance(&shapes, point);
    let gridded = |point| scene_distance_gridded(&shapes, &grid, &SkipPyramid::default(), point);

    let mut compared = 0;
    for step in 0..24 {
        let angle = step as f32 * std::f32::consts::TAU / 24.0;
        let origin = Vec3::new(angle.cos() * 14.0, 2.4, angle.sin() * 14.0);
        let direction = (Vec3::new(0.0, 2.4, 0.0) - origin).normalize();
        if exact(origin) < 0.0 {
            continue;
        }
        let (hit, _) = march(&exact, &exact, origin, direction, 1.2, 0.01, 128);
        let (grid_hit, _) = march(&gridded, &exact, origin, direction, 1.2, 0.01, 128);
        assert!(
            (hit - grid_hit).abs() < 0.05,
            "at angle {angle}: exact stopped at {hit}, gridded at {grid_hit}",
        );
        compared += 1;
    }
    assert!(compared > 20, "only {compared} rays ran");
}
