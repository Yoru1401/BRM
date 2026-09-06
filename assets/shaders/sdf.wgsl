#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_world}
#import bevy_pbr::mesh_view_bindings::view
#import "shaders/bindings.wgsl"::{MAX_MARCH_STEPS, render_params}
#import "shaders/adf.wgsl"::{adf_span, dynamic_distance, surface_normal}
#import "shaders/marching.wgsl"::{heat, ray_march}
#import "shaders/lighting.wgsl"::{shade}

const ALBEDO: vec3<f32> = vec3<f32>(0.78, 0.72, 0.64);
const BODY_ALBEDO: vec3<f32> = vec3<f32>(0.90, 0.35, 0.18);

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

fn depth_of(world_point: vec3<f32>) -> f32 {
    let clip = view.clip_from_world * vec4<f32>(world_point, 1.0);
    return clip.z / clip.w;
}

@fragment
fn fragment(quad: QuadVertex) -> FragmentOutput {
    let ray_origin = view.world_position;
    let ray_direction = normalize(quad.world_position.xyz - ray_origin);
    let span = adf_span(ray_origin, ray_direction);
    var march = vec2<f32>(0.0, 0.0);
    var hit = false;
    if span.y >= span.x {
        march = ray_march(span.x, MAX_MARCH_STEPS, ray_origin, ray_direction, span.y);
        hit = march.x < span.y;
    }

    var output: FragmentOutput;

    if render_params.debug_view == 1u {
        output.depth = 0.0;
        output.color = vec4<f32>(heat(march.y / f32(MAX_MARCH_STEPS)), 1.0);
        return output;
    }
    if !hit {
        discard;
    }

    let surface_point = ray_origin + ray_direction * march.x;
    var albedo = ALBEDO;
    if dynamic_distance(surface_point) < render_params.voxel {
        albedo = BODY_ALBEDO;
    }
    output.depth = depth_of(surface_point);
    output.color = vec4<f32>(albedo * shade(surface_point, surface_normal(surface_point)), 1.0);
    return output;
}
