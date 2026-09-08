#import "shaders/bindings.wgsl"::{AMBIENT, GpuLight, LIGHT_DIRECTIONAL, LIGHT_SPOT, MAX_MARCH_DISTANCE, Material, SHADOW_BIAS, SURFACE_THRESHOLD, lights, render_params}
#import "shaders/adf.wgsl"::{adf_probe}

fn shadow_bias() -> f32 {
    return max(SHADOW_BIAS, render_params.voxel * 2.0);
}

fn shadow_factor(origin: vec3<f32>, direction: vec3<f32>, far: f32, softness: f32) -> f32 {
    var shade = 1.0;
    var travelled = shadow_bias();
    for (var step = 0u; step < render_params.shadow_steps; step++) {
        if travelled >= far {
            break;
        }
        let probe = adf_probe(origin + direction * travelled);
        if probe.x < SURFACE_THRESHOLD {
            return 0.0;
        }
        if probe.y > 0.5 {
            shade = min(shade, softness * probe.x / travelled);
        }
        travelled += probe.x;
    }
    return clamp(shade, 0.0, 1.0);
}

fn light_contribution(
    light: GpuLight,
    surface_point: vec3<f32>,
    normal: vec3<f32>,
    towards_eye: vec3<f32>,
    surface: Material,
) -> vec3<f32> {
    var to_light = -light.direction;
    var distance_to_light = MAX_MARCH_DISTANCE;
    var attenuation = 1.0;

    if light.kind != LIGHT_DIRECTIONAL {
        let offset = light.position - surface_point;
        distance_to_light = length(offset);
        if distance_to_light >= light.range || distance_to_light < 1e-4 {
            return vec3<f32>(0.0);
        }
        to_light = offset / distance_to_light;
        let fraction = distance_to_light / light.range;
        let falloff = clamp(1.0 - fraction * fraction, 0.0, 1.0);
        attenuation = falloff * falloff;

        if light.kind == LIGHT_SPOT {
            let alignment = dot(light.direction, -to_light);
            attenuation *= smoothstep(light.cos_outer, light.cos_inner, alignment);
        }
    }

    let facing = max(dot(normal, to_light), 0.0);
    if facing <= 0.0 || attenuation <= 0.0 {
        return vec3<f32>(0.0);
    }

    var visibility = 1.0;
    if light.shadow != 0u {
        let reach = min(distance_to_light, MAX_MARCH_DISTANCE);
        let slope = clamp(1.0 / max(dot(normal, to_light), 0.15), 1.0, 6.0);
        visibility = shadow_factor(
            surface_point + normal * shadow_bias() * slope,
            to_light,
            reach,
            max(light.softness, 1e-3),
        );
    }
    let halfway = normalize(to_light + towards_eye);
    let sharpness = 8.0 + surface.gloss * 200.0;
    let highlight = pow(max(dot(normal, halfway), 0.0), sharpness) * surface.gloss;
    let reach = light.intensity * attenuation * visibility;
    return light.colour * reach * (surface.albedo * facing + vec3<f32>(highlight * facing));
}

fn shade(
    surface_point: vec3<f32>,
    normal: vec3<f32>,
    towards_eye: vec3<f32>,
    surface: Material,
) -> vec3<f32> {
    var total = surface.albedo * AMBIENT;
    for (var index = 0u; index < render_params.light_count; index++) {
        total += light_contribution(lights[index], surface_point, normal, towards_eye, surface);
    }
    return total;
}
