#import idk::shapes::{shape_distance, shape_normal}
#import "shaders/bindings.wgsl"::{APRON, BRICK, Dynamic, NORMAL_TAP, RANGE_VOXELS, SLOT_MASK, SPAN, TAG_SHIFT, TAG_SOLID, atlas, atlas_sampler, coarse, dynamics, page, paint, paint_sampler, render_params}

fn adf_low() -> vec3<f32> {
    return render_params.origin;
}

fn adf_high() -> vec3<f32> {
    return render_params.origin + vec3<f32>(render_params.bricks) * render_params.brick_size;
}

fn slot_origin(slot: u32) -> vec3<f32> {
    let side = render_params.slot_side;
    return vec3<f32>(
        f32(slot % side),
        f32((slot / side) % side),
        f32(slot / (side * side)),
    ) * f32(SPAN);
}

fn outside_box(offset: vec3<f32>, half_extent: vec3<f32>) -> f32 {
    return length(max(abs(offset) - half_extent, vec3<f32>(0.0)));
}

fn smallest(value: vec3<f32>) -> f32 {
    return min(value.x, min(value.y, value.z));
}

fn baked_probe(world_position: vec3<f32>) -> vec3<f32> {
    var roomiest = -1e30;
    var roomy_voxel = render_params.voxel;
    var found = false;

    for (var step = 0u; step < render_params.level_count; step++) {
        let level = render_params.levels[step];
        let size = level.brick_size;
        let local = (world_position - level.origin) / size;
        let last = vec3<f32>(level.bricks) - vec3<f32>(1.0);
        if any(local < vec3<f32>(0.0)) || any(local > last + vec3<f32>(1.0)) {
            continue;
        }

        let cell = clamp(floor(local), vec3<f32>(0.0), last);
        let index = level.page_at
            + u32(cell.x)
            + u32(cell.y) * level.bricks.x
            + u32(cell.z) * level.bricks.x * level.bricks.y;
        let word = page[index];
        let tag = word >> TAG_SHIFT;
        let range = level.range;

        if tag >= TAG_SOLID {
            let low = level.origin + cell * size;
            let gap = min(world_position - low, low + vec3<f32>(size) - world_position);
            let reach = max(smallest(gap), 0.0) + coarse[index] + range;
            if tag == TAG_SOLID {
                return vec3<f32>(-reach, 0.0, level.voxel);
            }
            var room = reach;
            if step + 1u < render_params.level_count {
                let high = level.origin + vec3<f32>(level.bricks) * size;
                let wall = max(smallest(min(world_position - level.origin, high - world_position)), 0.0);
                room = min(reach, wall);
            }
            if room > roomiest {
                roomiest = room;
                roomy_voxel = level.voxel;
            }
            found = true;
            continue;
        }

        let inside = clamp(local - cell, vec3<f32>(0.0), vec3<f32>(1.0)) * f32(BRICK) + f32(APRON);
        let uvw = (slot_origin(word & SLOT_MASK) + inside + vec3<f32>(0.5)) / render_params.atlas_side;
        let unit = textureSampleLevel(atlas, atlas_sampler, uvw, 0.0).r;
        let value = unit * 2.0 * range - range;
        if unit <= 0.002 {
            return vec3<f32>(value, 0.0, level.voxel);
        }
        if unit < 0.998 {
            return vec3<f32>(value, 1.0, level.voxel);
        }
        if value > roomiest {
            roomiest = value;
            roomy_voxel = level.voxel;
        }
        found = true;
    }

    if found {
        return vec3<f32>(roomiest, 0.0, roomy_voxel);
    }
    let coarsest = render_params.levels[render_params.level_count - 1u].voxel;
    let centre = (adf_low() + adf_high()) * 0.5;
    let half_extent = (adf_high() - adf_low()) * 0.5;
    return vec3<f32>(
        max(outside_box(world_position - centre, half_extent), render_params.voxel),
        0.0,
        coarsest,
    );
}

fn nearest_dynamic(world_position: vec3<f32>, ceiling: f32) -> vec2<f32> {
    if render_params.dynamic_count == 0u {
        return vec2<f32>(1e30, -1.0);
    }
    let bound = render_params.dynamic_bound;
    if length(world_position - bound.xyz) - bound.w >= ceiling {
        return vec2<f32>(1e30, -1.0);
    }
    var nearest = 1e30;
    var which = -1.0;
    for (var index = 0u; index < render_params.dynamic_count; index++) {
        let reach = shape_distance(world_position, dynamics[index]);
        if reach < nearest {
            nearest = reach;
            which = f32(index);
        }
    }
    return vec2<f32>(nearest, which);
}

fn adf_probe(world_position: vec3<f32>) -> vec3<f32> {
    let baked = baked_probe(world_position);
    let body = nearest_dynamic(world_position, baked.x);
    if body.x < baked.x {
        return vec3<f32>(body.x, 1.0, render_params.voxel);
    }
    return baked;
}

fn adf_distance(world_position: vec3<f32>) -> f32 {
    return adf_probe(world_position).x;
}

fn baked_normal(surface_point: vec3<f32>) -> vec3<f32> {
    let offset = vec2<f32>(1.0, -1.0) * render_params.voxel * NORMAL_TAP * 0.35;
    return normalize(
        offset.xyy * baked_probe(surface_point + offset.xyy).x +
        offset.yyx * baked_probe(surface_point + offset.yyx).x +
        offset.yxy * baked_probe(surface_point + offset.yxy).x +
        offset.xxx * baked_probe(surface_point + offset.xxx).x
    );
}

fn surface_normal(surface_point: vec3<f32>) -> vec3<f32> {
    let baked = baked_probe(surface_point).x;
    let body = nearest_dynamic(surface_point, baked);
    if body.y >= 0.0 && body.x < baked {
        return shape_normal(surface_point, dynamics[u32(body.y)]);
    }
    return baked_normal(surface_point);
}

fn level_at(world_position: vec3<f32>) -> u32 {
    return finest_level(world_position);
}

fn finest_level(world_position: vec3<f32>) -> u32 {
    for (var step = 0u; step < render_params.level_count; step++) {
        let level = render_params.levels[step];
        let local = (world_position - level.origin) / level.brick_size;
        let last = vec3<f32>(level.bricks) - vec3<f32>(1.0);
        if any(local < vec3<f32>(0.0)) || any(local > last + vec3<f32>(1.0)) {
            continue;
        }
        let cell = clamp(floor(local), vec3<f32>(0.0), last);
        let index = level.page_at
            + u32(cell.x)
            + u32(cell.y) * level.bricks.x
            + u32(cell.z) * level.bricks.x * level.bricks.y;
        if page[index] >> TAG_SHIFT < TAG_SOLID {
            return step;
        }
    }
    return render_params.level_count;
}

fn brick_cell(world_position: vec3<f32>) -> vec3<f32> {
    let step = min(finest_level(world_position), render_params.level_count - 1u);
    let level = render_params.levels[step];
    let local = (world_position - level.origin) / level.brick_size;
    return clamp(floor(local), vec3<f32>(0.0), vec3<f32>(level.bricks) - vec3<f32>(1.0));
}

fn brick_word(world_position: vec3<f32>) -> u32 {
    let step = min(finest_level(world_position), render_params.level_count - 1u);
    let level = render_params.levels[step];
    let cell = brick_cell(world_position);
    return page[level.page_at
        + u32(cell.x)
        + u32(cell.y) * level.bricks.x
        + u32(cell.z) * level.bricks.x * level.bricks.y];
}

fn baked_material(world_position: vec3<f32>) -> u32 {
    let step = finest_level(world_position);
    if step >= render_params.level_count {
        return 0u;
    }
    let level = render_params.levels[step];
    let local = (world_position - level.origin) / level.brick_size;
    let cell = clamp(floor(local), vec3<f32>(0.0), vec3<f32>(level.bricks) - vec3<f32>(1.0));
    let word = page[level.page_at
        + u32(cell.x)
        + u32(cell.y) * level.bricks.x
        + u32(cell.z) * level.bricks.x * level.bricks.y];
    let slot = word & SLOT_MASK;
    let side = render_params.slot_side;
    let base = vec3<f32>(
        f32(slot % side),
        f32((slot / side) % side),
        f32(slot / (side * side)),
    ) * f32(BRICK);
    let inside = clamp(local - cell, vec3<f32>(0.0), vec3<f32>(0.9999)) * f32(BRICK);
    let uvw = (base + floor(inside) + vec3<f32>(0.5)) / render_params.paint_side;
    return u32(textureSampleLevel(paint, paint_sampler, uvw, 0.0).r * 255.0 + 0.5);
}

fn adf_span(ray_origin: vec3<f32>, ray_direction: vec3<f32>) -> vec2<f32> {
    let inverse_direction = 1.0 / ray_direction;
    let to_min = (adf_low() - ray_origin) * inverse_direction;
    let to_max = (adf_high() - ray_origin) * inverse_direction;
    let nearest = min(to_min, to_max);
    let farthest = max(to_min, to_max);
    let entry = max(max(nearest.x, nearest.y), nearest.z);
    let exit = min(min(farthest.x, farthest.y), farthest.z);
    return vec2<f32>(max(entry, 0.0), exit);
}
