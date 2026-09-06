use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    image::ImageSampler,
    prelude::*,
    reflect::TypePath,
    render::{
        render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat},
        storage::ShaderBuffer,
    },
    shader::ShaderRef,
};

use crate::command_line;
use crate::game::input::Action;
use crate::sdf::adf::Adf;
use crate::sdf::dynamic::{GpuDynamic, MAX_DYNAMICS};
use crate::sdf::light::{GpuLight, MAX_LIGHTS};

pub(crate) struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SdfMaterial>::default())
            .add_systems(Startup, (load_shader_modules, spawn_camera))
            .add_systems(Update, (fit_quad, toggle_debug_view, toggle_quad));
    }
}

const SHADER_PATH: &str = "shaders/sdf.wgsl";

const SHADER_MODULES: [&str; 4] = [
    "shaders/bindings.wgsl",
    "shaders/adf.wgsl",
    "shaders/marching.wgsl",
    "shaders/lighting.wgsl",
];

#[derive(Resource)]
struct ShaderModules(
    #[expect(dead_code, reason = "held to keep the assets alive")] Vec<Handle<Shader>>,
);

fn load_shader_modules(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(ShaderModules(
        SHADER_MODULES
            .iter()
            .map(|path| assets.load(*path))
            .collect(),
    ));
}

const QUAD_DIST: f32 = 1.0;
const QUAD_OVERSCAN: f32 = 1.01;

pub(crate) const OMEGA: f32 = 1.0;
pub(crate) const SHADOW_STEPS: u32 = 48;
pub(crate) const DETAIL: f32 = 1.0;

#[derive(Component)]
pub(crate) struct Quad;

#[derive(Component)]
pub(crate) struct MainCamera;

#[derive(ShaderType, Debug, Clone, Default, PartialEq)]
pub(crate) struct RenderParams {
    pub(crate) origin: Vec3,
    pub(crate) brick_size: f32,
    pub(crate) bricks: UVec3,
    pub(crate) light_count: u32,
    pub(crate) voxel: f32,
    pub(crate) tan_half_fov: f32,
    pub(crate) detail: f32,
    pub(crate) omega: f32,
    pub(crate) shadow_steps: u32,
    pub(crate) debug_view: u32,
    pub(crate) slot_side: u32,
    pub(crate) atlas_side: f32,
    pub(crate) dynamic_count: u32,
    pub(crate) padding_one: u32,
    pub(crate) padding_two: u32,
    pub(crate) padding_three: u32,
    pub(crate) dynamic_bound: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub(crate) struct SdfMaterial {
    #[uniform(0)]
    pub(crate) render_params: RenderParams,
    #[storage(1, read_only)]
    pub(crate) page: Handle<ShaderBuffer>,
    #[storage(2, read_only)]
    pub(crate) lights: Handle<ShaderBuffer>,
    #[texture(3, dimension = "3d")]
    #[sampler(4)]
    pub(crate) atlas: Handle<Image>,
    #[storage(5, read_only)]
    pub(crate) dynamics: Handle<ShaderBuffer>,
}

impl Material for SdfMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER_PATH.into()
    }
    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        MainCamera,
        Transform::from_xyz(0.0, 2.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::layer(0),
    ));
}

pub(crate) fn atlas_image(voxels: &[u8], side: u32) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: side,
        },
        TextureDimension::D3,
        voxels.to_vec(),
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

pub(crate) fn spawn_quad(
    commands: &mut Commands,
    camera: Entity,
    adf: &Adf,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<SdfMaterial>,
    images: &mut Assets<Image>,
    buffers: &mut Assets<ShaderBuffer>,
) {
    let material = materials.add(SdfMaterial {
        render_params: RenderParams {
            origin: adf.origin,
            brick_size: adf.brick_size(),
            bricks: adf.bricks,
            voxel: adf.voxel,
            omega: command_line::value("--omega").unwrap_or(OMEGA),
            shadow_steps: command_line::value("--shadow-steps").map_or(SHADOW_STEPS, |s| s as u32),
            detail: command_line::value("--detail").unwrap_or(DETAIL),
            debug_view: u32::from(command_line::flag("--debug-view")),
            slot_side: adf.slots,
            atlas_side: adf.atlas_side() as f32,
            ..default()
        },
        page: buffers.add(ShaderBuffer::from(adf.page.clone())),
        lights: buffers.add(ShaderBuffer::from(vec![
            GpuLight::default();
            MAX_LIGHTS
        ])),
        atlas: images.add(atlas_image(&adf.atlas, adf.atlas_side())),
        dynamics: buffers.add(ShaderBuffer::from(vec![
            GpuDynamic::default();
            MAX_DYNAMICS
        ])),
    });

    commands.entity(camera).with_child((
        Quad,
        RenderLayers::layer(0),
        Mesh3d(meshes.add(Plane3d::new(Vec3::Z, Vec2::splat(1.0)))),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, 0.0, -QUAD_DIST),
    ));
}

fn toggle_debug_view(
    actions: Res<ButtonInput<Action>>,
    quad: Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
) {
    if !actions.just_pressed(Action::ToggleDebugView) {
        return;
    }
    if let Some(mut material) = materials.get_mut(&quad.0) {
        material.render_params.debug_view = 1 - material.render_params.debug_view;
    }
}

fn toggle_quad(actions: Res<ButtonInput<Action>>, visibility: Single<&mut Visibility, With<Quad>>) {
    if !actions.just_pressed(Action::ToggleQuad) {
        return;
    }
    let mut visibility = visibility.into_inner();
    *visibility = match *visibility {
        Visibility::Hidden => Visibility::Inherited,
        _ => Visibility::Hidden,
    };
}

fn fit_quad(
    proj: Single<&Projection, With<MainCamera>>,
    quad: Single<(&mut Mesh3d, &MeshMaterial3d<SdfMaterial>), With<Quad>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
    mut last_aspect: Local<f32>,
) {
    let Projection::Perspective(perspective) = &*proj else {
        return;
    };
    if (perspective.aspect_ratio - *last_aspect).abs() < 1e-6 {
        return;
    }
    *last_aspect = perspective.aspect_ratio;

    let tan_half_fov = (perspective.fov * 0.5).tan();
    let half_height = tan_half_fov * QUAD_DIST * QUAD_OVERSCAN;
    let half_width = half_height * perspective.aspect_ratio;

    let (mut mesh, quad_material) = quad.into_inner();
    mesh.0 = meshes.add(Plane3d::new(Vec3::Z, Vec2::new(half_width, half_height)));
    if let Some(mut material) = materials.get_mut(&quad_material.0) {
        material.render_params.tan_half_fov = tan_half_fov;
    }
}
