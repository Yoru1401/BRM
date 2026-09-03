#import "shaders/bindings.wgsl"::{AMBIENT, GpuLight, LIGHT_DIRECTIONAL, LIGHT_SPOT, MAX_MARCH_DISTANCE, SHADOW_BIAS, SURFACE_THRESHOLD, lights, render_params}
#import "shaders/scene.wgsl"::{shadow_proxy_distance}

fn shadow_factor(origin: vec3<f32>, direction: vec3<f32>, far: f32, softness: f32) -> f32 {
    var shade = 1.0;
    var travelled = SHADOW_BIAS;
    for (var step = 0u; step < render_params.shadow_steps; step++) {
        if travelled >= far {
            break;
        }
        let distance = shadow_proxy_distance(origin + direction * travelled);
        if distance < SURFACE_THRESHOLD {
            return 0.0;
        }
        shade = min(shade, softness * distance / travelled);
        travelled += distance;
    }
    return clamp(shade, 0.0, 1.0);
}

fn light_contribution(light: GpuLight, surface_point: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
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
        visibility = shadow_factor(
            surface_point + normal * SHADOW_BIAS,
            to_light,
            reach,
            max(light.softness, 1e-3),
        );
    }
    return light.colour * (light.intensity * facing * attenuation * visibility);
}

fn shade(surface_point: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    var total = vec3<f32>(AMBIENT);
    for (var index = 0u; index < render_params.light_count; index++) {
        total += light_contribution(lights[index], surface_point, normal);
    }
    return total;
}
