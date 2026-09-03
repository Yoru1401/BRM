use bevy::prelude::*;

use crate::sdf::brush::{
    GPU_MODE_AVOID, GPU_MODE_DEBOSS, GPU_MODE_EMBOSS, GPU_MODE_INTERSECT, GPU_MODE_PAINT,
    GPU_MODE_PUSH, GPU_MODE_SHELL, GPU_MODE_SUBTRACT, GpuBlend,
};

fn union_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mix = (0.5 + 0.5 * (field - shape) / radius).clamp(0.0, 1.0);
    field.lerp(shape, mix) - radius * mix * (1.0 - mix)
}

fn subtract_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mix = (0.5 - 0.5 * (field + shape) / radius).clamp(0.0, 1.0);
    field.lerp(-shape, mix) + radius * mix * (1.0 - mix)
}

fn intersect_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mix = (0.5 - 0.5 * (field - shape) / radius).clamp(0.0, 1.0);
    field.lerp(shape, mix) + radius * mix * (1.0 - mix)
}

fn op_union(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return shape.min(field).min((shape - 0.5 * radius + field) * 0.5);
    }
    if radius > 0.0 {
        return union_smooth(shape, field, radius);
    }
    shape.min(field)
}

fn op_intersect(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return shape.max(field).max((field + 0.5 * radius + shape) * 0.5);
    }
    if radius > 0.0 {
        return intersect_smooth(shape, field, radius);
    }
    shape.max(field)
}

fn op_subtract(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return op_intersect(-shape, field, radius, true);
    }
    if radius > 0.0 {
        return subtract_smooth(shape, field, radius);
    }
    field.max(-shape)
}

pub(crate) fn blend(shape: f32, field: f32, blend: &GpuBlend, chamfer: bool) -> f32 {
    let (radius, strength) = (blend.radius, blend.strength);
    match blend.mode {
        GPU_MODE_SUBTRACT => op_subtract(shape, field, radius, chamfer),
        GPU_MODE_INTERSECT => op_intersect(shape, field, radius, chamfer),

        GPU_MODE_PAINT => field,
        GPU_MODE_PUSH => op_subtract(shape - strength, field, radius, chamfer).min(shape),
        GPU_MODE_AVOID => op_subtract(field - strength, shape, radius, chamfer).min(field),
        GPU_MODE_EMBOSS => op_union(
            field,
            op_intersect(shape, field - strength, radius, chamfer),
            radius,
            chamfer,
        ),
        GPU_MODE_DEBOSS => op_subtract(
            op_subtract(field + strength, shape, radius, chamfer),
            field,
            radius,
            chamfer,
        ),
        GPU_MODE_SHELL => op_intersect(shape, (field + strength).abs() - strength, radius, chamfer),
        _ => op_union(shape, field, radius, chamfer),
    }
}
