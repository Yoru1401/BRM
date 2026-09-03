use bevy::prelude::*;

use crate::sdf::brush::{
    GPU_MODE_ADD, GPU_MODE_INTERSECT, GPU_MODE_PAINT, GPU_MODE_SUBTRACT, GpuShape, MIN_RADIUS,
    footprint_of,
};

pub(crate) fn shape_half_extent(shape: &GpuShape) -> Vec3 {
    world_aligned_extent(shape.half_size, shape.inverse_rotation) + Vec3::splat(shape.blend.radius)
}

pub(crate) fn world_aligned_extent(local_extent: Vec3, inverse_rotation: Vec4) -> Vec3 {
    let rotation = Mat3::from_quat(Quat::from_vec4(inverse_rotation).inverse());
    let unsigned = Mat3::from_cols(
        rotation.x_axis.abs(),
        rotation.y_axis.abs(),
        rotation.z_axis.abs(),
    );
    unsigned * local_extent
}

const BOUNDS_SLACK: f32 = 0.05;

pub(crate) fn scene_bounds(shapes: &[GpuShape]) -> (Vec3, Vec3) {
    let mut minimum = Vec3::splat(f32::MAX);
    let mut maximum = Vec3::splat(f32::MIN);
    for shape in shapes {
        if matches!(
            shape.blend.mode,
            GPU_MODE_SUBTRACT | GPU_MODE_INTERSECT | GPU_MODE_PAINT
        ) {
            continue;
        }
        let half_extent = shape_half_extent(shape);
        minimum = minimum.min(shape.center - half_extent);
        maximum = maximum.max(shape.center + half_extent);
    }
    if minimum.cmpgt(maximum).any() {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    (
        minimum - Vec3::splat(BOUNDS_SLACK),
        maximum + Vec3::splat(BOUNDS_SLACK),
    )
}

pub(crate) fn cull_box_distance(offset: Vec3, half_extent: Vec3) -> f32 {
    (offset.abs() - half_extent).max(Vec3::ZERO).length()
}

pub(crate) fn shape_cannot_reach(shape: &GpuShape, world_point: Vec3, field: f32) -> bool {
    shape.blend.mode == GPU_MODE_ADD
        && shape.cull_scale * cull_box_distance(world_point - shape.center, shape.cull_extent)
            >= field
}

pub(crate) fn cull_bound(shape: &GpuShape) -> (Vec3, f32) {
    let footprint = footprint_of(shape.half_size);
    let slope = (shape.taper * footprint) / (2.0 * shape.half_size.y.max(MIN_RADIUS));
    let scale = 1.0 / (1.0 + slope * slope).sqrt();
    (
        world_aligned_extent(shape.half_size, shape.inverse_rotation)
            + Vec3::splat(shape.blend.radius / scale),
        scale,
    )
}
