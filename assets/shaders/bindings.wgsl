struct LevelInfo {
    origin: vec3<f32>,
    brick_size: f32,
    bricks: vec3<u32>,
    page_at: u32,
    voxel: f32,
    range: f32,
    spare_one: u32,
    spare_two: u32,
};

struct RenderParams {
    origin: vec3<f32>,
    brick_size: f32,
    bricks: vec3<u32>,
    light_count: u32,
    voxel: f32,
    tan_half_fov: f32,
    detail: f32,
    omega: f32,
    shadow_steps: u32,
    debug_view: u32,
    slot_side: u32,
    atlas_side: f32,
    dynamic_count: u32,
    paint_side: f32,
    level_count: u32,
    padding_three: u32,
    dynamic_bound: vec4<f32>,
    levels: array<LevelInfo, 4>,
};

struct Dynamic {
    start: vec3<f32>,
    radius: f32,
    end: vec3<f32>,
    flags: u32,
};

struct Material {
    albedo: vec3<f32>,
    gloss: f32,
};

struct GpuLight {
    position: vec3<f32>,
    kind: u32,
    direction: vec3<f32>,
    range: f32,
    colour: vec3<f32>,
    intensity: f32,
    cos_inner: f32,
    cos_outer: f32,
    shadow: u32,
    softness: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> render_params: RenderParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> page: array<u32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> lights: array<GpuLight>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var atlas: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var atlas_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var<storage, read> dynamics: array<Dynamic>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var<storage, read> materials: array<Material>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var paint: texture_3d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var paint_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var<storage, read> coarse: array<f32>;

const BRICK: u32 = 8u;
const APRON: u32 = 1u;
const SPAN: u32 = 10u;
const RANGE_VOXELS: f32 = 4.0;
const NORMAL_TAP: f32 = 1.0;
const TAG_EMPTY: u32 = 255u;
const TAG_SOLID: u32 = 254u;
const TAG_SHIFT: u32 = 24u;
const SLOT_MASK: u32 = 1048575u;

const MAX_MARCH_STEPS: i32 = 128;
const MAX_MARCH_DISTANCE: f32 = 100000.0;
const SURFACE_THRESHOLD: f32 = 0.001;
const AMBIENT: f32 = 0.05;

const LIGHT_DIRECTIONAL: u32 = 0u;
const LIGHT_POINT: u32 = 1u;
const LIGHT_SPOT: u32 = 2u;

const SHADOW_BIAS: f32 = 0.02;
