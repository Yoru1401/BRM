#import "shaders/bindings.wgsl"::{Blend}

fn union_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mixture = clamp(0.5 + 0.5 * (field - shape) / radius, 0.0, 1.0);
    return mix(field, shape, mixture) - radius * mixture * (1.0 - mixture);
}

fn subtract_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mixture = clamp(0.5 - 0.5 * (field + shape) / radius, 0.0, 1.0);
    return mix(field, -shape, mixture) + radius * mixture * (1.0 - mixture);
}

fn intersect_smooth(shape: f32, field: f32, radius: f32) -> f32 {
    let mixture = clamp(0.5 - 0.5 * (field - shape) / radius, 0.0, 1.0);
    return mix(field, shape, mixture) + radius * mixture * (1.0 - mixture);
}

fn op_union(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return min(min(shape, field), (shape - 0.5 * radius + field) * 0.5);
    }
    if radius > 0.0 {
        return union_smooth(shape, field, radius);
    }
    return min(shape, field);
}

fn op_intersect(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return max(max(shape, field), (field + 0.5 * radius + shape) * 0.5);
    }
    if radius > 0.0 {
        return intersect_smooth(shape, field, radius);
    }
    return max(shape, field);
}

fn op_subtract(shape: f32, field: f32, radius: f32, chamfer: bool) -> f32 {
    if chamfer {
        return op_intersect(-shape, field, radius, true);
    }
    if radius > 0.0 {
        return subtract_smooth(shape, field, radius);
    }
    return max(field, -shape);
}

fn blend_shape(shape: f32, field: f32, blend: Blend, chamfer: bool) -> f32 {
    let radius = blend.radius;
    let strength = blend.strength;
    switch blend.mode {
        case 1u: { return op_subtract(shape, field, radius, chamfer); }
        case 2u: { return op_intersect(shape, field, radius, chamfer); }
        case 3u: { return field; }
        case 4u: { return min(op_subtract(shape - strength, field, radius, chamfer), shape); }
        case 5u: { return min(op_subtract(field - strength, shape, radius, chamfer), field); }
        case 6u: {
            return op_union(
                field,
                op_intersect(shape, field - strength, radius, chamfer),
                radius,
                chamfer,
            );
        }
        case 7u: {
            return op_subtract(
                op_subtract(field + strength, shape, radius, chamfer),
                field,
                radius,
                chamfer,
            );
        }
        case 8u: {
            return op_intersect(shape, abs(field + strength) - strength, radius, chamfer);
        }
        default: { return op_union(shape, field, radius, chamfer); }
    }
}
