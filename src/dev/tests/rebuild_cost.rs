use std::time::Instant;

use bevy::prelude::*;

use crate::dev::tests::helpers::placed;
use crate::sdf::bounds::scene_bounds;
use crate::sdf::brush::{CsgOperation, GPU_MODE_ADD, GPU_MODE_SUBTRACT, GpuShape, MAX_SHAPES};
use crate::sdf::grid::{GRID_CELL_WORDS, GRID_INDEX_WORDS, build_grid};
use bevy::render::storage::ShaderBuffer;

fn scene_of(count: usize, carved: usize) -> Vec<GpuShape> {
    let per_axis = (count as f32).cbrt().ceil().max(1.0) as usize;
    (0..count)
        .map(|index| {
            let slot = Vec3::new(
                (index % per_axis) as f32,
                ((index / per_axis) % per_axis) as f32,
                (index / (per_axis * per_axis)) as f32,
            );
            let mode = if index > 0 && index % (count / carved.max(1)).max(1) == 0 {
                GPU_MODE_SUBTRACT
            } else {
                GPU_MODE_ADD
            };
            placed(
                Transform {
                    translation: slot * 4.0 - Vec3::splat(per_axis as f32 * 2.0),
                    scale: Vec3::splat(1.0),
                    ..default()
                },
                CsgOperation {
                    mode,
                    radius: 0.2,
                    ..default()
                },
            )
        })
        .collect()
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn time<T>(runs: usize, mut work: impl FnMut() -> T) -> f64 {
    let samples = (0..runs)
        .map(|_| {
            let start = Instant::now();
            let produced = work();
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            drop(produced);
            elapsed
        })
        .collect();
    median(samples)
}

fn pad<T: Clone + Default>(values: &[T], capacity: usize) -> Vec<T> {
    let mut padded = values.to_vec();
    padded.resize_with(capacity, T::default);
    padded
}

#[test]
#[ignore = "timing probe, run explicitly"]
fn what_a_rebuild_costs() {
    println!(
        "\nshapes\tcarved\tcells\tentries\tbounds\tbuild\tpad_shapes\tpad_cells\tpad_indices\ttotal"
    );
    for (count, carved) in [(8, 2), (32, 4), (128, 8), (256, 16)] {
        let shapes = scene_of(count, carved);
        let bounds_cost = time(200, || scene_bounds(&shapes));
        let (bounds_min, bounds_max) = scene_bounds(&shapes);
        let build_cost = time(200, || build_grid(&shapes, bounds_min, bounds_max, 16));
        let grid = build_grid(&shapes, bounds_min, bounds_max, 16);

        let pad_shapes = time(200, || pad(&shapes, MAX_SHAPES));
        let pad_cells = time(200, || pad(&grid.cells, GRID_CELL_WORDS));
        let pad_indices = time(200, || pad(&grid.indices, GRID_INDEX_WORDS));

        let padded_shapes = pad(&shapes, MAX_SHAPES);
        let padded_cells = pad(&grid.cells, GRID_CELL_WORDS);
        let padded_indices = pad(&grid.indices, GRID_INDEX_WORDS);
        let mut buffer = ShaderBuffer::default();
        let set_shapes = time(200, || buffer.set_data(padded_shapes.clone()));
        let set_cells = time(200, || buffer.set_data(padded_cells.clone()));
        let set_indices = time(200, || buffer.set_data(padded_indices.clone()));

        let total = bounds_cost
            + build_cost
            + pad_shapes
            + pad_cells
            + pad_indices
            + set_shapes
            + set_cells
            + set_indices;

        println!(
            "{count}\t{carved}\t{}\t{}\t{bounds_cost:.4}\t{build_cost:.4}\t{pad_shapes:.4}\t\
             {pad_cells:.4}\t{pad_indices:.4}\t{set_shapes:.4}\t{set_cells:.4}\t\
             {set_indices:.4}\t{total:.4}",
            grid.cells.len() / 2,
            grid.indices.len(),
        );
    }
    println!(
        "\nuploaded every rebuild: shapes {} KB, cells {} KB, indices {} KB",
        MAX_SHAPES * size_of::<GpuShape>() / 1024,
        GRID_CELL_WORDS * 4 / 1024,
        GRID_INDEX_WORDS * 4 / 1024,
    );
}
