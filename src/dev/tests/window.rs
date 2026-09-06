use bevy::prelude::*;

use super::helpers::*;
use crate::sdf::brush::Modifiers;
use crate::sdf::grid::{GRID_MAX_RESOLUTION, GridWindow, SkipPyramid, build_grid};

fn settings(cell_size: f32, resolution: u32) -> (f32, u32) {
    (cell_size, resolution)
}

#[test]
fn a_window_holds_its_own_centre_and_spans_cell_size_times_resolution() {
    let window = GridWindow::around(Vec3::new(31.0, -4.0, 117.5), 2.0, 16);
    let span = window.cell_size * window.resolution.as_vec3();

    assert_eq!(span, Vec3::splat(32.0), "16 cells of 2 m must span 32 m");
    for axis in 0..3 {
        let low = window.origin[axis];
        let centre = [31.0f32, -4.0, 117.5][axis];
        assert!(
            low <= centre && centre <= low + span[axis],
            "axis {axis}: {centre} is outside {low}..{}",
            low + span[axis],
        );
    }
}

#[test]
fn a_window_snaps_so_small_camera_moves_do_not_rebuild_it() {
    let settings = settings(2.0, 16);
    let still = GridWindow::around(Vec3::new(10.0, 0.0, 0.0), settings.0, settings.1);

    for nudge in [0.01f32, 0.4, 0.9, 1.9] {
        assert_eq!(
            GridWindow::around(Vec3::new(10.0 + nudge, 0.0, 0.0), settings.0, settings.1),
            still,
            "a {nudge} m step inside one cell must not move the window",
        );
    }
    assert_ne!(
        GridWindow::around(Vec3::new(12.5, 0.0, 0.0), settings.0, settings.1),
        still,
        "crossing a cell boundary must move the window",
    );
}

#[test]
fn a_window_never_depends_on_how_far_away_a_stray_shape_sits() {
    let settings = settings(2.0, 16);
    let near = GridWindow::around(Vec3::ZERO, settings.0, settings.1);
    assert_eq!(near.cell_size, Vec3::splat(2.0));

    let mut shapes = vec![shaped(
        Transform::from_xyz(0.0, 0.0, 0.0),
        Modifiers::default(),
    )];
    let close = build_grid(&shapes, near);
    shapes.push(shaped(
        Transform::from_xyz(4000.0, 0.0, 0.0),
        Modifiers::default(),
    ));
    let with_stray = build_grid(
        &shapes,
        GridWindow::around(Vec3::ZERO, settings.0, settings.1),
    );

    assert_eq!(
        close.cell_size, with_stray.cell_size,
        "a shape 4 km away must not change the cell size",
    );
    assert_eq!(close.resolution, with_stray.resolution);
}

#[test]
fn resolution_is_clamped_and_a_degenerate_cell_never_reaches_the_grid() {
    let huge = GridWindow::around(Vec3::ZERO, 2.0, 9999);
    assert_eq!(huge.resolution, UVec3::splat(GRID_MAX_RESOLUTION));

    let flat = GridWindow::around(Vec3::ZERO, 0.0, 16);
    assert!(
        flat.cell_size.min_element() > 0.0,
        "a zero cell size would divide by zero in every lookup",
    );
}

#[test]
fn a_window_that_excludes_geometry_still_never_overshoots() {
    let mut state = 0x7A11_0C41_BEEF_1234u64;
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

    let shapes: Vec<crate::sdf::brush::GpuShape> = (0..24)
        .map(|_| {
            shaped(
                Transform {
                    translation: Vec3::new(spread!(40.0), spread!(6.0), spread!(40.0)),
                    scale: Vec3::splat(0.4 + next() * 1.6),
                    ..default()
                },
                Modifiers::default(),
            )
        })
        .collect();

    let settings = settings(2.0, 16);
    let mut outside_the_window = 0;
    for eye in [
        Vec3::new(0.0, 3.0, 11.0),
        Vec3::new(-20.0, 2.0, -20.0),
        Vec3::new(35.0, 5.0, 5.0),
    ] {
        let window = GridWindow::around(eye, settings.0, settings.1);
        let grid = build_grid(&shapes, window);

        for _ in 0..400 {
            let point = eye + Vec3::new(spread!(18.0), spread!(18.0), spread!(18.0));
            let exact = crate::sdf::field::scene_distance(&shapes, point);
            let gridded = crate::sdf::grid::scene_distance_gridded(
                &shapes,
                &grid,
                &SkipPyramid::default(),
                point,
            );
            assert!(
                gridded <= exact + 1e-4,
                "window at {eye:?} reported {gridded} where the field is {exact}, at {point:?}",
            );
            if !grid.holds(point) {
                outside_the_window += 1;
            }
        }
    }
    assert!(
        outside_the_window > 100,
        "only {outside_the_window} samples landed outside the window, so this proves little",
    );
}
