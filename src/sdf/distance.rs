use bevy::prelude::*;

use crate::sdf::brush::{GpuShape, footprint_of};

pub(crate) const MAX_MARCH_DISTANCE: f32 = 100.0;

pub(crate) fn shape_distance(shape: &GpuShape, world_point: Vec3) -> f32 {
    let local_point = Quat::from_vec4(shape.inverse_rotation) * (world_point - shape.center);
    tapered_box_distance(local_point, shape)
}

pub(crate) fn tapered_box_distance(local_point: Vec3, shape: &GpuShape) -> f32 {
    let footprint = footprint_of(shape.half_size);
    if shape.taper <= 0.0 {
        return rounded_box_distance(
            local_point,
            shape.half_size,
            bore(shape.wall_thickness, 0.0, footprint),
            shape.side_radius,
            shape.cap_radius,
        );
    }

    let taper = shape.taper * footprint;
    let height_fraction = ((local_point.y / shape.half_size.y + 1.0) * 0.5).clamp(0.0, 1.0);
    let inset = taper * height_fraction;
    let remaining = (footprint - inset).max(0.0);

    let narrowed = Vec3::new(
        shape.half_size.x - inset,
        shape.half_size.y,
        shape.half_size.z - inset,
    );
    let distance = rounded_box_distance(
        local_point,
        narrowed,
        bore(shape.wall_thickness, taper - inset, remaining),
        (shape.side_radius - inset).max(0.0),
        shape.cap_radius,
    );

    let slope = taper / (2.0 * shape.half_size.y);
    distance / (1.0 + slope * slope).sqrt()
}

pub(crate) fn rounded_box_distance(
    local_point: Vec3,
    half_size: Vec3,
    wall: f32,
    side_radius: f32,
    cap_radius: f32,
) -> f32 {
    let inner = Vec3::new(
        half_size.x - side_radius,
        half_size.y - cap_radius,
        half_size.z - side_radius,
    );

    let side_radius = side_radius - wall;
    let wall = wall - cap_radius;

    let corner = local_point.abs() - inner;
    let flat = Vec2::new(corner.x, corner.z);
    let cross_section = flat.max(Vec2::ZERO).length() + flat.max_element().min(0.0) - side_radius;

    let profile = Vec2::new(cross_section.abs() - wall, corner.y);
    profile.max_element().min(0.0) + profile.max(Vec2::ZERO).length() - cap_radius
}

pub(crate) fn bore(wall: f32, unspent_taper: f32, remaining: f32) -> f32 {
    (wall + unspent_taper).min(remaining)
}
