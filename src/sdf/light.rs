use bevy::{
    prelude::*,
    render::{render_resource::ShaderType, storage::ShaderBuffer},
};

use crate::sdf::render::{Quad, SdfMaterial};

pub(crate) struct LightPlugin;

impl Plugin for LightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SdfLights>()
            .add_systems(Update, sync_lights_to_gpu);
    }
}

pub(crate) const MAX_LIGHTS: usize = 32;

const GPU_LIGHT_DIRECTIONAL: u32 = 0;
const GPU_LIGHT_POINT: u32 = 1;
const GPU_LIGHT_SPOT: u32 = 2;

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct Light {
    pub(crate) kind: LightKind,

    pub(crate) colour: Vec3,
    pub(crate) intensity: f32,

    pub(crate) range: f32,

    pub(crate) shadow: bool,

    pub(crate) softness: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum LightKind {
    #[default]
    Directional,

    Point,

    Spot {
        inner: f32,
        outer: f32,
    },
}

impl Default for Light {
    fn default() -> Self {
        Light {
            kind: LightKind::Directional,
            colour: Vec3::ONE,
            intensity: 1.0,
            range: 20.0,
            shadow: false,
            softness: 8.0,
        }
    }
}

#[derive(ShaderType, Debug, Clone, PartialEq, Default)]
pub(crate) struct GpuLight {
    pub(crate) position: Vec3,
    pub(crate) kind: u32,

    pub(crate) direction: Vec3,
    pub(crate) range: f32,
    pub(crate) colour: Vec3,
    pub(crate) intensity: f32,

    pub(crate) cos_inner: f32,
    pub(crate) cos_outer: f32,
    pub(crate) shadow: u32,
    pub(crate) softness: f32,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct SdfLights {
    pub(crate) lights: Vec<GpuLight>,
}

impl Light {
    pub(crate) fn to_gpu(self, placement: &GlobalTransform) -> GpuLight {
        let direction = (placement.rotation() * Vec3::NEG_Z).normalize_or_zero();
        let (kind, cos_inner, cos_outer) = match self.kind {
            LightKind::Directional => (GPU_LIGHT_DIRECTIONAL, 1.0, -1.0),
            LightKind::Point => (GPU_LIGHT_POINT, 1.0, -1.0),
            LightKind::Spot { inner, outer } => {
                let outer = outer.max(inner + 1e-3);
                (GPU_LIGHT_SPOT, inner.cos(), outer.cos())
            }
        };
        GpuLight {
            position: placement.translation(),
            kind,
            direction,
            range: self.range.max(1e-3),
            colour: self.colour,
            intensity: self.intensity,
            cos_inner,
            cos_outer,
            shadow: u32::from(self.shadow),
            softness: self.softness,
        }
    }
}

fn sync_lights_to_gpu(
    lights: Query<(&Light, &GlobalTransform)>,
    quad: Single<&MeshMaterial3d<SdfMaterial>, With<Quad>>,
    mut materials: ResMut<Assets<SdfMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut scene: ResMut<SdfLights>,
) {
    let mut packed: Vec<GpuLight> = lights
        .iter()
        .map(|(light, placement)| light.to_gpu(placement))
        .collect();

    if packed.len() > MAX_LIGHTS {
        warn!(
            "scene has {} lights, buffer holds {MAX_LIGHTS}; the rest are dropped",
            packed.len()
        );
        packed.truncate(MAX_LIGHTS);
    }

    if packed == scene.lights {
        return;
    }
    scene.lights = packed;

    let Some(mut material) = materials.get_mut(&quad.0) else {
        return;
    };
    material.render_params.light_count = scene.lights.len() as u32;

    let handle = material.lights.clone();
    if let Some(mut buffer) = buffers.get_mut(&handle) {
        let mut padded = scene.lights.clone();
        padded.resize(MAX_LIGHTS, GpuLight::default());
        buffer.set_data(padded);
    }
}
