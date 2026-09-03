#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_world}
#import bevy_pbr::mesh_view_bindings::view
#import "shaders/bindings.wgsl"::{MAX_MARCH_DISTANCE, MAX_MARCH_STEPS, render_params}
#import "shaders/scene.wgsl"::{scene_albedo, scene_bounds_span, surface_normal}
#import "shaders/marching.wgsl"::{heat, ray_march}
#import "shaders/lighting.wgsl"::{shade}

const COARSE_SLACK: f32 = 0.02;

@group(#{MATERIAL_BIND_GROUP}) @binding(5) var coarse_distance: texture_2d<f32>;

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

struct FragmentOutput {
    @builtin(frag_depth) depth: f32,
    @location(0) color: vec4<f32>,
};

fn head_start(fragment_position: vec2<f32>, span: vec2<f32>) -> f32 {
    if render_params.hierarchy == 0u {
        return span.x;
    }
    let size = vec2<i32>(textureDimensions(coarse_distance, 0));
    let uv = fragment_position / max(view.viewport.zw, vec2<f32>(1.0));
    let texel = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1));
    let reached = textureLoad(coarse_distance, texel, 0).r - COARSE_SLACK;
    return clamp(reached, span.x, span.y);
}

fn depth_of(world_point: vec3<f32>) -> f32 {
    let clip = view.clip_from_world * vec4<f32>(world_point, 1.0);
    return clip.z / clip.w;
}

@fragment
fn fragment(quad: QuadVertex) -> FragmentOutput {
    let ray_origin = view.world_position;
    let ray_direction = normalize(quad.world_position.xyz - ray_origin);
    let span = scene_bounds_span(ray_origin, ray_direction);
    var march = vec2<f32>(MAX_MARCH_DISTANCE, 0.0);
    if span.y >= span.x {
        march = ray_march(
            head_start(quad.clip_position.xy, span),
            MAX_MARCH_STEPS,
            ray_origin,
            ray_direction,
            span.y,
        );
    }
    let travelled = march.x;

    var output: FragmentOutput;

    if render_params.debug_view == 1u {
        let steps_used = march.y;
        output.depth = 0.0;
        output.color = vec4<f32>(heat(steps_used / f32(MAX_MARCH_STEPS)), 1.0);
        return output;
    }
    if travelled >= span.y {
        discard;
    }

    let surface_point = ray_origin + ray_direction * travelled;
    let albedo = scene_albedo(surface_point);
    let light = shade(surface_point, surface_normal(surface_point));

    output.depth = depth_of(surface_point);
    output.color = vec4<f32>(albedo * light, 1.0);
    return output;
}
