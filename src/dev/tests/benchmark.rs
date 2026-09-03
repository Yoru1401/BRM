use bevy::prelude::*;

#[test]
fn every_bench_count_fills_the_same_slab() {
    use crate::dev::benchmark::{SLAB_HALF_SIZE, cells_per_axis, grid_layout};

    for count in [1, 8, 20, 27, 64, 80, 125] {
        let layout = grid_layout(count);
        assert_eq!(layout.len(), count);

        let cell = SLAB_HALF_SIZE / cells_per_axis(count) as f32;
        for placement in &layout {
            assert_eq!(placement.scale, cell);
            let corner = placement.translation.abs() + cell;
            assert!(
                corner.cmple(SLAB_HALF_SIZE + Vec3::splat(1e-4)).all(),
                "count {count} put a brush corner at {corner:?}, outside {SLAB_HALF_SIZE:?}"
            );
        }

        let widest = layout
            .iter()
            .map(|placement| placement.translation.x + cell.x)
            .fold(f32::MIN, f32::max);
        assert!(
            (widest - SLAB_HALF_SIZE.x).abs() < 1e-4,
            "count {count} stopped at {widest}"
        );
    }
}

#[test]
fn spread_boxes_never_touch() {
    use crate::dev::benchmark::{SPREAD_HALF_SIZE, spread_layout};

    for count in [8, 20, 80, 125, 256] {
        let layout = spread_layout(count);
        assert_eq!(layout.len(), count);

        for placement in &layout {
            let corner = placement.translation.abs() + placement.scale;
            assert!(
                corner.cmple(SPREAD_HALF_SIZE + Vec3::splat(1e-4)).all(),
                "count {count} put a box corner at {corner:?}, outside the volume"
            );
        }

        for (index, one) in layout.iter().enumerate() {
            for other in &layout[index + 1..] {
                let gap = (one.translation - other.translation).abs() - one.scale - other.scale;
                assert!(
                    gap.cmpgt(Vec3::ZERO).any(),
                    "count {count} overlapped two boxes at {:?} and {:?}",
                    one.translation,
                    other.translation
                );
            }
        }
    }
}
