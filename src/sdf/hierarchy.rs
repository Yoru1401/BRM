use bevy::{
    asset::RenderAssetUsages,
    camera::{Hdr, ImageRenderTarget, RenderTarget, visibility::RenderLayers},
    image::ImageSampler,
    prelude::*,
    render::{
        render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat, TextureUsages},
        storage::ShaderBuffer,
    },
    shader::ShaderRef,
};

use crate::command_line;
use crate::sdf::render::{MainCamera, Quad, RenderParams, SdfMaterial};

pub(crate) fn coarse_scale() -> f32 {
    command_line::value("--coarse-scale").unwrap_or(4.0)
}
const COARSE_SHADER: &str = "shaders/coarse.wgsl";
const COARSE_LAYER: usize = 1;

pub(crate) fn requested() -> bool {
    command_line::flag("--hierarchical")
}

#[derive(Component)]
struct CoarseQuad;

#[derive(Component)]
pub(crate) struct CoarseCamera;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Default)]
pub(crate) struct CoarseMaterial {
    #[uniform(0)]
    pub(crate) render_params: RenderParams,
    #[storage(1, read_only)]
    pub(crate) shapes: Handle<ShaderBuffer>,
    #[storage(2, read_only)]
    pub(crate) grid_cells: Handle<ShaderBuffer>,
    #[storage(3, read_only)]
    pub(crate) grid_indices: Handle<ShaderBuffer>,
}

impl Material for CoarseMaterial {
    fn vertex_shader() -> ShaderRef {
        COARSE_SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        COARSE_SHADER.into()
    }
}

pub(crate) fn coarse_image(width: u32, height: u32) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 8],
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::nearest();
    image
}

pub(crate) struct HierarchyPlugin;

impl Plugin for HierarchyPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<CoarseMaterial>::default())
            .add_systems(
                Update,
                (follow_main_quad, start_coarse_pass, mirror_render_params),
            );
    }
}

pub(crate) fn spawn_coarse_pass(
    commands: &mut Commands,
    materials: &mut Assets<CoarseMaterial>,
    mesh: Handle<Mesh>,
    target: Handle<Image>,
    render_params: RenderParams,
    shapes: Handle<ShaderBuffer>,
    grid_cells: Handle<ShaderBuffer>,
    grid_indices: Handle<ShaderBuffer>,
    quad_distance: f32,
) -> Entity {
    commands
        .spawn((
            Camera3d::default(),
            CoarseCamera,
            Camera {
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            Hdr,
            RenderTarget::Image(ImageRenderTarget {
                handle: target,
                scale_factor: 1.0,
            }),
            RenderLayers::layer(COARSE_LAYER),
            Transform::default(),
            children![(
                CoarseQuad,
                RenderLayers::layer(COARSE_LAYER),
                Mesh3d(mesh),
                MeshMaterial3d(materials.add(CoarseMaterial {
                    render_params,
                    shapes,
                    grid_cells,
                    grid_indices,
                })),
                Transform::from_xyz(0.0, 0.0, -quad_distance),
            )],
        ))
        .id()
}

#[derive(Resource)]
pub(crate) struct PendingCoarsePass {
    pub(crate) render_params: RenderParams,
    pub(crate) mesh: Handle<Mesh>,
    pub(crate) shapes: Handle<ShaderBuffer>,
    pub(crate) grid_cells: Handle<ShaderBuffer>,
    pub(crate) grid_indices: Handle<ShaderBuffer>,
    pub(crate) camera: Entity,
    pub(crate) quad_distance: f32,
}

fn start_coarse_pass(
    mut commands: Commands,
    pending: Option<Res<PendingCoarsePass>>,
    camera: Option<Single<&Camera, With<MainCamera>>>,
    quad: Option<Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<CoarseMaterial>>,
    mut sdf_materials: ResMut<Assets<SdfMaterial>>,
) {
    let (Some(pending), Some(camera), Some(quad)) = (pending, camera, quad) else {
        return;
    };
    let Some(size) = camera.physical_target_size() else {
        return;
    };
    let target = images.add(coarse_image(
        (size.x as f32 / coarse_scale()).ceil() as u32,
        (size.y as f32 / coarse_scale()).ceil() as u32,
    ));

    let pass = spawn_coarse_pass(
        &mut commands,
        &mut materials,
        pending.mesh.clone(),
        target.clone(),
        pending.render_params.clone(),
        pending.shapes.clone(),
        pending.grid_cells.clone(),
        pending.grid_indices.clone(),
        pending.quad_distance,
    );
    commands.entity(pending.camera).add_child(pass);

    if let Some(mut material) = sdf_materials.get_mut(&quad.0) {
        material.coarse = target;
        material.render_params.hierarchy = 1;
    }
    commands.remove_resource::<PendingCoarsePass>();
}

fn mirror_render_params(
    main: Option<Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>>,
    coarse: Option<Single<&MeshMaterial3d<CoarseMaterial>, With<CoarseQuad>>>,
    sdf_materials: Res<Assets<SdfMaterial>>,
    mut coarse_materials: ResMut<Assets<CoarseMaterial>>,
) {
    let (Some(main), Some(coarse)) = (main, coarse) else {
        return;
    };
    let Some(source) = sdf_materials.get(&main.0) else {
        return;
    };
    let Some(mut target) = coarse_materials.get_mut(&coarse.0) else {
        return;
    };
    if target.render_params != source.render_params {
        target.render_params = source.render_params.clone();
    }
}

fn follow_main_quad(
    main: Option<Single<&Mesh3d, With<Quad>>>,
    coarse: Option<Single<&mut Mesh3d, (With<CoarseQuad>, Without<Quad>)>>,
) {
    let (Some(main), Some(coarse)) = (main, coarse) else {
        return;
    };
    let mut coarse = coarse.into_inner();
    if coarse.0 != main.0 {
        coarse.0 = main.0.clone();
    }
}
