use bevy::{
    prelude::*,
    render::{render_resource::ShaderType, storage::ShaderBuffer},
};

use crate::sdf::render::{Quad, SdfMaterial};
use crate::sdf::shapes::{CAPSULE, SPHERE};

const KIND_SHIFT: u32 = 16;

pub(crate) const MAX_DYNAMICS: usize = 64;

pub(crate) struct DynamicPlugin;

impl Plugin for DynamicPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, sync_dynamics_to_gpu);
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct Dynamic {
    pub(crate) start: Vec3,
    pub(crate) end: Vec3,
    pub(crate) radius: f32,
    pub(crate) material: u32,
    pub(crate) kind: u32,
}

impl Dynamic {
    pub(crate) fn ball(centre: Vec3, radius: f32, material: u32) -> Self {
        Dynamic {
            start: centre,
            end: centre,
            radius,
            material,
            kind: SPHERE,
        }
    }

    pub(crate) fn rod(start: Vec3, end: Vec3, radius: f32, material: u32) -> Self {
        Dynamic {
            start,
            end,
            radius,
            material,
            kind: CAPSULE,
        }
    }

    fn flags(&self) -> u32 {
        self.material | (self.kind << KIND_SHIFT)
    }
}

#[derive(ShaderType, Debug, Clone, PartialEq, Default)]
pub(crate) struct GpuDynamic {
    pub(crate) start: Vec3,
    pub(crate) radius: f32,
    pub(crate) end: Vec3,
    pub(crate) flags: u32,
}

fn bounding_sphere(bodies: &[GpuDynamic]) -> Vec4 {
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for body in bodies {
        let reach = Vec3::splat(body.radius);
        low = low.min(body.start.min(body.end) - reach);
        high = high.max(body.start.max(body.end) + reach);
    }
    if bodies.is_empty() {
        return Vec4::ZERO;
    }
    let centre = (low + high) * 0.5;
    centre.extend((high - centre).length())
}

fn sync_dynamics_to_gpu(
    bodies: Query<&Dynamic>,
    quad: Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut uploaded: Local<Vec<GpuDynamic>>,
) {
    let mut packed: Vec<GpuDynamic> = bodies
        .iter()
        .map(|body| GpuDynamic {
            start: body.start,
            radius: body.radius,
            end: body.end,
            flags: body.flags(),
        })
        .collect();
    if packed.len() > MAX_DYNAMICS {
        packed.truncate(MAX_DYNAMICS);
    }
    if packed == *uploaded {
        return;
    }
    *uploaded = packed.clone();

    let Some(mut material) = materials.get_mut(&quad.0) else {
        return;
    };
    material.render_params.dynamic_count = packed.len() as u32;
    material.render_params.dynamic_bound = bounding_sphere(&packed);

    let handle = material.dynamics.clone();
    if let Some(mut buffer) = buffers.get_mut(&handle) {
        packed.resize(MAX_DYNAMICS, GpuDynamic::default());
        buffer.set_data(packed);
    }
}
