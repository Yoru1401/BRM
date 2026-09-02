//! Lights, as entities.
//!
//! A light is an entity with a [`Light`] and a `Transform`, exactly like a
//! brush. It can be parented to a moving thing, animated, spawned in `bsn!`.
//! `sync_lights_to_gpu` packs them into a fixed-size storage buffer the shader
//! walks per shaded pixel.
//!
//! # What each kind costs
//!
//! Diffuse is a dot product and an attenuation - nothing. **A shadow is a whole
//! march**, per light, per pixel. That is why [`Light::shadow`] is opt-in: a
//! room with twenty torches wants twenty lights and one or two shadow casters,
//! not twenty extra marches.

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

/// The buffer is allocated once at this size and never resized, for the same
/// reason the shape buffer is: a resize rebuilds the GPU buffer while the bind
/// group still points at the old one.
pub(crate) const MAX_LIGHTS: usize = 32;

const GPU_LIGHT_DIRECTIONAL: u32 = 0;
const GPU_LIGHT_POINT: u32 = 1;
const GPU_LIGHT_SPOT: u32 = 2;

/// What a light is. The transform says where it is and which way it faces;
/// this says what it does.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct Light {
    pub(crate) kind: LightKind,
    /// Linear RGB, before intensity.
    pub(crate) colour: Vec3,
    pub(crate) intensity: f32,
    /// Point and spot only. Brightness falls to nothing here, so it is also
    /// the radius the shader uses to skip the light entirely.
    pub(crate) range: f32,
    /// Whether this light casts. **One march per shaded pixel** when it does,
    /// so it is off by default and turned on for the few that matter.
    pub(crate) shadow: bool,
    /// Penumbra width. 0 is a hard edge; larger spreads the shadow's edge out
    /// with distance from the caster. Free - it comes out of the march that
    /// was already happening.
    pub(crate) softness: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum LightKind {
    /// Infinitely far away, so only its direction matters. The sun.
    #[default]
    Directional,
    /// Shines everywhere from a point, fading out by `range`.
    Point,
    /// A point light with a cone. `inner` is where it starts to fade, `outer`
    /// where it ends, both half-angles in radians.
    Spot { inner: f32, outer: f32 },
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

/// Storage-buffer layout. Must match `struct GpuLight` in sdf.wgsl.
#[derive(ShaderType, Debug, Clone, PartialEq, Default)]
pub(crate) struct GpuLight {
    pub(crate) position: Vec3,
    pub(crate) kind: u32,
    /// Where the light points, for directional and spot. Unit length.
    pub(crate) direction: Vec3,
    pub(crate) range: f32,
    pub(crate) colour: Vec3,
    pub(crate) intensity: f32,
    /// Cosines rather than angles, so the shader compares against a dot
    /// product without a trig call per pixel.
    pub(crate) cos_inner: f32,
    pub(crate) cos_outer: f32,
    pub(crate) shadow: u32,
    pub(crate) softness: f32,
}

/// The packed lights, kept so the upload can be skipped when nothing moved.
#[derive(Resource, Debug, Default)]
pub(crate) struct SdfLights {
    pub(crate) lights: Vec<GpuLight>,
}

impl Light {
    pub(crate) fn to_gpu(self, placement: &GlobalTransform) -> GpuLight {
        // -Z is forward, the same convention Bevy's own lights and cameras use.
        let direction = (placement.rotation() * Vec3::NEG_Z).normalize_or_zero();
        let (kind, cos_inner, cos_outer) = match self.kind {
            LightKind::Directional => (GPU_LIGHT_DIRECTIONAL, 1.0, -1.0),
            LightKind::Point => (GPU_LIGHT_POINT, 1.0, -1.0),
            LightKind::Spot { inner, outer } => {
                // Outer must not be inside inner, or the falloff divides by a
                // negative and the cone turns inside out.
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

/// Mirrors every [`Light`] entity into the material's storage buffer.
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
    // Order is irrelevant here - light contributions add - but the equality
    // gate still needs a stable list, and query order is stable per archetype.
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
