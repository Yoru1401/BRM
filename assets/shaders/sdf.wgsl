#import bevy_pbr::mesh_functions::{get_world_from_local, mesh_position_local_to_world}
#import bevy_pbr::mesh_view_bindings::view
#import "shaders/bindings.wgsl"::{MAX_MARCH_STEPS, TAG_SHIFT, TAG_SOLID, dynamics, materials, render_params}
#import "shaders/adf.wgsl"::{adf_span, baked_material, baked_normal, baked_probe, brick_cell, brick_word, nearest_dynamic}
#import idk::shapes::{shape_normal}
#import "shaders/marching.wgsl"::{heat, ray_march}
#import "shaders/lighting.wgsl"::{shade}

fn brick_colour(world_position: vec3<f32>) -> vec3<f32> {
    let cell = brick_cell(world_position);
    let word = brick_word(world_position);
    let tag = word >> TAG_SHIFT;
    let checker = f32((u32(cell.x) + u32(cell.y) + u32(cell.z)) % 2u) * 0.25 + 0.55;
    if tag == TAG_SOLID {
        return vec3<f32>(0.75, 0.2, 0.2) * checker;
    }
    if tag >= TAG_SOLID {
        let clearance = f32(word & 0xffffffu);
        return vec3<f32>(0.15, 0.25, 0.6) * checker * clamp(clearance / 8.0, 0.3, 1.4);
    }
    return heat(f32(baked_material(world_position)) / 7.0) * checker;
}

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
    output.depth = depth_of(surface_point);

    if render_params.debug_view == 2u {
        output.color = vec4<f32>(brick_colour(surface_point), 1.0);
        return output;
    }

    let baked = baked_probe(surface_point).x;
    let body = nearest_dynamic(surface_point, baked);
    var normal = baked_normal(surface_point);
    var surface = materials[baked_material(surface_point)];
    if body.y >= 0.0 && body.x < baked {
        normal = shape_normal(surface_point, dynamics[u32(body.y)]);
        surface = materials[dynamics[u32(body.y)].flags & 0xffffu];
    }

    output.color = vec4<f32>(shade(surface_point, normal, -ray_direction, surface), 1.0);
    return output;
}
