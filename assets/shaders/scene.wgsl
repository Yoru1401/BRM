#import "shaders/bindings.wgsl"::{GRID_CELL_FULL, MAX_MARCH_DISTANCE, MODE_ADD, MODE_INTERSECT, MODE_SUBTRACT, NORMAL_EPSILON, Shape, grid_cells, grid_indices, render_params, shapes}
#import "shaders/shapes.wgsl"::{footprint_of, rotate_by_quaternion, rounded_box_distance, shape_distance}
#import "shaders/operations.wgsl"::{blend_shape}

fn cull_box_distance(offset: vec3<f32>, half_extent: vec3<f32>) -> f32 {
    return length(max(abs(offset) - half_extent, vec3<f32>(0.0)));
}

fn shape_cannot_reach(shape: Shape, world_position: vec3<f32>, field: f32) -> bool {
    return render_params.cull != 0u
        && shape.blend.mode == MODE_ADD
        && shape.cull_scale * cull_box_distance(world_position - shape.center, shape.cull_extent)
            >= field;
}

fn scene_distance(world_position: vec3<f32>) -> f32 {
    var field = MAX_MARCH_DISTANCE;
    for (var i = 0u; i < render_params.shape_count; i++) {
        let shape = shapes[i];
        if i > 0u && shape_cannot_reach(shape, world_position, field) {
            continue;
        }
        let distance = shape_distance(shape, world_position);
        if i == 0u {
            field = distance;
        } else {
            field = blend_shape(distance, field, shape.blend, shape.blend.chamfer != 0u);
        }
    }
    return field;
}

fn grid_slot(world_position: vec3<f32>) -> vec3<f32> {
    return clamp(
        floor((world_position - render_params.grid_origin) / render_params.grid_cell),
        vec3<f32>(0.0),
        vec3<f32>(render_params.grid_resolution - vec3<u32>(1u)),
    );
}

fn grid_cell(world_position: vec3<f32>) -> u32 {
    let slot = grid_slot(world_position);
    return u32(slot.x)
        + u32(slot.y) * render_params.grid_resolution.x
        + u32(slot.z) * render_params.grid_resolution.x * render_params.grid_resolution.y;
}

fn grid_holds(world_position: vec3<f32>) -> bool {
    let high = render_params.grid_origin
        + render_params.grid_cell * vec3<f32>(render_params.grid_resolution);
    return all(world_position >= render_params.grid_origin) && all(world_position <= high);
}

fn grid_exit_distance(world_position: vec3<f32>) -> f32 {
    let slot = grid_slot(world_position);
    let overlap = render_params.grid_cell * 0.5;
    let low = render_params.grid_origin + slot * render_params.grid_cell - overlap;
    let high = render_params.grid_origin
        + (slot + vec3<f32>(1.0)) * render_params.grid_cell + overlap;
    let to_wall = min(world_position - low, high - world_position);
    return max(min(to_wall.x, min(to_wall.y, to_wall.z)), 0.0);
}

fn scene_distance_gridded(world_position: vec3<f32>) -> f32 {
    if render_params.grid == 0u || !grid_holds(world_position) {
        return scene_distance(world_position);
    }
    let cell = grid_cell(world_position);
    let count = grid_cells[cell * 2u + 1u];
    if count == GRID_CELL_FULL {
        return scene_distance(world_position);
    }

    let offset = grid_cells[cell * 2u];
    var field = MAX_MARCH_DISTANCE;
    var evaluated = 0u;
    for (var slot = 0u; slot < count; slot++) {
        let shape = shapes[grid_indices[offset + slot]];
        if evaluated > 0u && shape_cannot_reach(shape, world_position, field) {
            continue;
        }
        let distance = shape_distance(shape, world_position);
        if evaluated == 0u {
            field = distance;
        } else {
            field = blend_shape(distance, field, shape.blend, shape.blend.chamfer != 0u);
        }
        evaluated++;
    }

    if count == render_params.shape_count {
        return field;
    }
    return min(field, grid_exit_distance(world_position));
}

fn shadow_proxy_bound(shape: Shape, world_position: vec3<f32>) -> f32 {
    if shape.blend.mode != MODE_ADD {
        return MAX_MARCH_DISTANCE;
    }
    let local = rotate_by_quaternion(world_position - shape.center, shape.inverse_rotation);
    return rounded_box_distance(
        local,
        shape.half_size,
        footprint_of(shape.half_size),
        shape.side_radius,
        shape.cap_radius,
    );
}

fn shadow_proxy_distance(world_position: vec3<f32>) -> f32 {
    var field = MAX_MARCH_DISTANCE;
    var count = 0u;
    var offset = 0u;
    var indexed = false;

    if render_params.grid != 0u && grid_holds(world_position) {
        let cell = grid_cell(world_position);
        count = grid_cells[cell * 2u + 1u];
        if count != GRID_CELL_FULL {
            offset = grid_cells[cell * 2u];
            indexed = true;
        }
    }

    if !indexed {
        for (var i = 0u; i < render_params.shape_count; i++) {
            field = min(field, shadow_proxy_bound(shapes[i], world_position));
        }
        return field;
    }

    for (var slot = 0u; slot < count; slot++) {
        let shape = shapes[grid_indices[offset + slot]];
        field = min(field, shadow_proxy_bound(shape, world_position));
    }
    if count == render_params.shape_count {
        return field;
    }
    return min(field, grid_exit_distance(world_position));
}

fn scene_albedo(world_position: vec3<f32>) -> vec3<f32> {
    var nearest = MAX_MARCH_DISTANCE;
    var colour = vec3<f32>(0.5);
    for (var i = 0u; i < render_params.shape_count; i++) {
        let shape = shapes[i];
        if shape.blend.mode == MODE_SUBTRACT || shape.blend.mode == MODE_INTERSECT {
            continue;
        }
        let distance = shape_distance(shape, world_position);
        if distance < nearest {
            nearest = distance;
            colour = shape.albedo;
        }
    }
    return colour;
}

fn surface_normal(surface_point: vec3<f32>) -> vec3<f32> {
    let offset = vec2<f32>(1.0, -1.0) * NORMAL_EPSILON;
    return normalize(
        offset.xyy * scene_distance_gridded(surface_point + offset.xyy) +
        offset.yyx * scene_distance_gridded(surface_point + offset.yyx) +
        offset.yxy * scene_distance_gridded(surface_point + offset.yxy) +
        offset.xxx * scene_distance_gridded(surface_point + offset.xxx)
    );
}

fn scene_bounds_span(ray_origin: vec3<f32>, ray_direction: vec3<f32>) -> vec2<f32> {
    let inverse_direction = 1.0 / ray_direction;
    let to_min = (render_params.bounds_min - ray_origin) * inverse_direction;
    let to_max = (render_params.bounds_max - ray_origin) * inverse_direction;
    let nearest = min(to_min, to_max);
    let farthest = max(to_min, to_max);
    let entry = max(max(nearest.x, nearest.y), nearest.z);
    let exit = min(min(farthest.x, farthest.y), farthest.z);
    return vec2<f32>(max(entry, 0.0), min(exit, MAX_MARCH_DISTANCE));
}
