#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_world}
#import bevy_pbr::mesh_view_bindings::view
#import "shaders/bindings.wgsl"::{MAX_MARCH_DISTANCE, MAX_MARCH_STEPS, render_params}
#import "shaders/scene.wgsl"::{scene_bounds_span, scene_distance_gridded}

const COARSE_SPREAD_SLACK: f32 = 1.25;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

struct QuadVertex {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> QuadVertex {
    var result: QuadVertex;
    result.world_position = mesh_position_local_to_world(
        get_world_from_local(vertex.instance_index),
        vec4<f32>(vertex.position, 1.0),
    );
    result.clip_position = view.clip_from_world * result.world_position;
    return result;
}

fn cone_march(ray_origin: vec3<f32>, ray_direction: vec3<f32>, start: f32, stop: f32) -> f32 {
    let spread = COARSE_SPREAD_SLACK * render_params.tan_half_fov / max(view.viewport.w, 1.0);
    var travelled = start;
    var step = 0;

    loop {
        if step >= MAX_MARCH_STEPS || travelled >= stop {
            break;
        }
        let distance = scene_distance_gridded(ray_origin + ray_direction * travelled);
        let footprint = travelled * spread;
        if distance < footprint {
            break;
        }
        travelled += distance - footprint;
        step++;
    }
    return min(travelled, stop);
}

@fragment
fn fragment(quad: QuadVertex) -> @location(0) vec4<f32> {
    let ray_origin = view.world_position;
    let ray_direction = normalize(quad.world_position.xyz - ray_origin);
    let span = scene_bounds_span(ray_origin, ray_direction);

    var reached = MAX_MARCH_DISTANCE;
    if span.y >= span.x {
        reached = cone_march(ray_origin, ray_direction, span.x, span.y);
    }
    return vec4<f32>(reached, 0.0, 0.0, 1.0);
}
