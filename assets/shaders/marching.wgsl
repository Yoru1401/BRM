#import bevy_pbr::mesh_view_bindings::view
#import "shaders/bindings.wgsl"::{SURFACE_THRESHOLD, render_params}
#import "shaders/scene.wgsl"::{scene_distance, scene_distance_gridded}

fn pixel_radius_per_unit() -> f32 {
    return render_params.tan_half_fov / max(view.viewport.w, 1.0);
}

fn ray_march(
    start_distance: f32,
    step_budget: i32,
    ray_origin: vec3<f32>,
    ray_direction: vec3<f32>,
    stop_distance: f32,
) -> vec2<f32> {
    let precision_per_unit = pixel_radius_per_unit();
    var travelled = start_distance;
    var step = 0;

    var relaxation = max(render_params.omega, 1.0);
    var previous_distance = 0.0;
    var step_length = 0.0;

    loop {
        if step >= step_budget {
            return vec2<f32>(stop_distance, f32(step));
        }
        let distance = scene_distance_gridded(ray_origin + ray_direction * travelled);
        let overshot = relaxation > 1.0 && (abs(distance) + previous_distance) < step_length;
        let close_enough = max(SURFACE_THRESHOLD, travelled * precision_per_unit);
        step++;

        if !overshot && distance < close_enough
            && scene_distance(ray_origin + ray_direction * travelled) < close_enough {
            travelled += distance;
            break;
        }

        if overshot {
            step_length = step_length * (1.0 - relaxation);
            relaxation = 1.0;
        } else {
            step_length = distance * relaxation;
        }
        previous_distance = abs(distance);
        travelled += step_length;
        if travelled >= stop_distance {
            break;
        }
    }
    return vec2<f32>(travelled, f32(step));
}

fn heat(fraction: f32) -> vec3<f32> {
    let t = clamp(fraction, 0.0, 1.0);
    return clamp(
        vec3<f32>(3.0 * t - 1.2, 1.6 - abs(3.0 * t - 1.6), 1.2 - 3.0 * t),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
}
