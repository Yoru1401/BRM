#import "shaders/bindings.wgsl"::{Shape}

fn rotate_by_quaternion(offset: vec3<f32>, rotation: vec4<f32>) -> vec3<f32> {
    return offset + 2.0 * cross(rotation.xyz, cross(rotation.xyz, offset) + rotation.w * offset);
}

fn footprint_of(half_size: vec3<f32>) -> f32 {
    return min(half_size.x, half_size.z);
}

fn rounded_box_distance(
    local_position: vec3<f32>,
    half_size: vec3<f32>,
    wall_arg: f32,
    side_radius_arg: f32,
    cap_radius: f32,
) -> f32 {
    let inner = vec3<f32>(
        half_size.x - side_radius_arg,
        half_size.y - cap_radius,
        half_size.z - side_radius_arg,
    );
    let side_radius = side_radius_arg - wall_arg;
    let wall = wall_arg - cap_radius;

    let corner = abs(local_position) - inner;
    let flat = vec2<f32>(corner.x, corner.z);
    let cross_section = length(max(flat, vec2<f32>(0.0))) + min(max(flat.x, flat.y), 0.0) - side_radius;

    let profile = vec2<f32>(abs(cross_section) - wall, corner.y);
    return min(max(profile.x, profile.y), 0.0) + length(max(profile, vec2<f32>(0.0))) - cap_radius;
}

fn bore(wall: f32, unspent_taper: f32, remaining: f32) -> f32 {
    return min(wall + unspent_taper, remaining);
}

fn tapered_box_distance(local_position: vec3<f32>, shape: Shape) -> f32 {
    let footprint = footprint_of(shape.half_size);
    if shape.taper <= 0.0 {
        return rounded_box_distance(
            local_position,
            shape.half_size,
            bore(shape.wall_thickness, 0.0, footprint),
            shape.side_radius,
            shape.cap_radius,
        );
    }

    let taper = shape.taper * footprint;
    let height_fraction = clamp((local_position.y / shape.half_size.y + 1.0) * 0.5, 0.0, 1.0);
    let inset = taper * height_fraction;
    let remaining = max(footprint - inset, 0.0);

    let narrowed = vec3<f32>(
        shape.half_size.x - inset,
        shape.half_size.y,
        shape.half_size.z - inset,
    );
    let distance = rounded_box_distance(
        local_position,
        narrowed,
        bore(shape.wall_thickness, taper - inset, remaining),
        max(shape.side_radius - inset, 0.0),
        shape.cap_radius,
    );

    let slope = taper / (2.0 * shape.half_size.y);
    return distance / sqrt(1.0 + slope * slope);
}

fn shape_distance(shape: Shape, world_position: vec3<f32>) -> f32 {
    let local_position = rotate_by_quaternion(world_position - shape.center, shape.inverse_rotation);
    return tapered_box_distance(local_position, shape);
}
