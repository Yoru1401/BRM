#import "shaders/bindings.wgsl"::{APRON, BRICK, CLEARANCE_MASK, Dynamic, NORMAL_TAP, RANGE_VOXELS, SPAN, TAG_SHIFT, TAG_SOLID, atlas, atlas_sampler, dynamics, page, render_params}

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

fn baked_probe(world_position: vec3<f32>) -> vec2<f32> {
    let size = render_params.brick_size;
    let local = (world_position - render_params.origin) / size;
    let last = vec3<f32>(render_params.bricks) - vec3<f32>(1.0);

    if any(local < vec3<f32>(0.0)) || any(local > last + vec3<f32>(1.0)) {
        let centre = (adf_low() + adf_high()) * 0.5;
        let half_extent = (adf_high() - adf_low()) * 0.5;
        return vec2<f32>(max(outside_box(world_position - centre, half_extent), render_params.voxel), 0.0);
    }

    let cell = clamp(floor(local), vec3<f32>(0.0), last);
    let word = page[u32(cell.x)
        + u32(cell.y) * render_params.bricks.x
        + u32(cell.z) * render_params.bricks.x * render_params.bricks.y];
    let tag = word >> TAG_SHIFT;

    if tag >= TAG_SOLID {
        let low = render_params.origin + cell * size;
        let gap = min(world_position - low, low + vec3<f32>(size) - world_position);
        let clearance = f32(max(word & CLEARANCE_MASK, 1u) - 1u);
        let reach = max(min(gap.x, min(gap.y, gap.z)), 0.0)
            + clearance * size
            + render_params.voxel * RANGE_VOXELS;
        if tag == TAG_SOLID {
            return vec2<f32>(-reach, 0.0);
        }
        return vec2<f32>(reach, 0.0);
    }
    let slot = word;

    let inside = clamp(local - cell, vec3<f32>(0.0), vec3<f32>(1.0)) * f32(BRICK) + f32(APRON);
    let uvw = (slot_origin(slot) + inside + vec3<f32>(0.5)) / render_params.atlas_side;
    let range = render_params.voxel * RANGE_VOXELS;
    let unit = textureSampleLevel(atlas, atlas_sampler, uvw, 0.0).r;
    let banded = f32(unit > 0.002 && unit < 0.998);
    return vec2<f32>(unit * 2.0 * range - range, banded);
}

fn capsule_distance(world_position: vec3<f32>, body: Dynamic) -> f32 {
    let from_start = world_position - body.start;
    let along = body.end - body.start;
    let fraction = clamp(dot(from_start, along) / max(dot(along, along), 1e-8), 0.0, 1.0);
    return length(from_start - along * fraction) - body.radius;
}

fn dynamic_distance(world_position: vec3<f32>) -> f32 {
    var nearest = 1e30;
    for (var index = 0u; index < render_params.dynamic_count; index++) {
        nearest = min(nearest, capsule_distance(world_position, dynamics[index]));
    }
    return nearest;
}

fn adf_probe(world_position: vec3<f32>) -> vec2<f32> {
    let baked = baked_probe(world_position);
    if render_params.dynamic_count == 0u {
        return baked;
    }
    let bound = render_params.dynamic_bound;
    if length(world_position - bound.xyz) - bound.w >= baked.x {
        return baked;
    }
    let body = dynamic_distance(world_position);
    if body < baked.x {
        return vec2<f32>(body, 1.0);
    }
    return baked;
}

fn adf_distance(world_position: vec3<f32>) -> f32 {
    return adf_probe(world_position).x;
}

fn surface_normal(surface_point: vec3<f32>) -> vec3<f32> {
    let offset = vec2<f32>(1.0, -1.0) * render_params.voxel * NORMAL_TAP;
    return normalize(
        offset.xyy * adf_distance(surface_point + offset.xyy) +
        offset.yyx * adf_distance(surface_point + offset.yyx) +
        offset.yxy * adf_distance(surface_point + offset.yxy) +
        offset.xxx * adf_distance(surface_point + offset.xxx)
    );
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
