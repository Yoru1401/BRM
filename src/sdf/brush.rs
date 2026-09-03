use bevy::{prelude::*, render::render_resource::ShaderType};

use crate::sdf::bounds::cull_bound;

pub(crate) const MIN_RADIUS: f32 = 1e-5;
pub(crate) const MAX_SHAPES: usize = 256;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Brush;

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct Modifiers {
    pub(crate) round: f32,

    pub(crate) bevel: f32,

    pub(crate) thickness: f32,

    pub(crate) cone: f32,
}

impl Default for Modifiers {
    fn default() -> Self {
        Modifiers {
            round: 0.0,
            bevel: 0.0,
            thickness: 1.0,
            cone: 0.0,
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct CsgOperation {
    pub(crate) mode: u32,
    pub(crate) chamfer: bool,
    pub(crate) radius: f32,
    pub(crate) strength: f32,
}

impl Default for CsgOperation {
    fn default() -> Self {
        CsgOperation {
            mode: GPU_MODE_ADD,
            chamfer: false,
            radius: 0.0,
            strength: 0.0,
        }
    }
}

#[derive(ShaderType, Debug, Clone, PartialEq, Default)]
pub(crate) struct GpuBlend {
    pub(crate) mode: u32,

    pub(crate) radius: f32,

    pub(crate) strength: f32,

    pub(crate) chamfer: u32,
}

#[derive(ShaderType, Debug, Clone, PartialEq, Default)]
pub(crate) struct GpuShape {
    pub(crate) center: Vec3,

    pub(crate) wall_thickness: f32,

    pub(crate) half_size: Vec3,

    pub(crate) side_radius: f32,

    pub(crate) inverse_rotation: Vec4,

    pub(crate) albedo: Vec3,

    pub(crate) cap_radius: f32,

    pub(crate) cull_extent: Vec3,

    pub(crate) cull_scale: f32,

    pub(crate) taper: f32,
    pub(crate) padding_one: f32,
    pub(crate) padding_two: f32,
    pub(crate) padding_three: f32,
    pub(crate) blend: GpuBlend,
}

pub(crate) const GPU_MODE_ADD: u32 = 0;
pub(crate) const GPU_MODE_SUBTRACT: u32 = 1;
pub(crate) const GPU_MODE_INTERSECT: u32 = 2;
pub(crate) const GPU_MODE_PAINT: u32 = 3;
pub(crate) const GPU_MODE_PUSH: u32 = 4;
pub(crate) const GPU_MODE_AVOID: u32 = 5;
pub(crate) const GPU_MODE_EMBOSS: u32 = 6;
pub(crate) const GPU_MODE_DEBOSS: u32 = 7;
pub(crate) const GPU_MODE_SHELL: u32 = 8;

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct SphereBody {
    pub(crate) radius: f32,
    pub(crate) velocity: Vec3,
    pub(crate) angular_velocity: Vec3,

    pub(crate) orientation: Quat,

    pub(crate) resting: bool,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct Albedo(pub(crate) Vec3);

impl Default for Albedo {
    fn default() -> Self {
        Albedo(DEFAULT_ALBEDO)
    }
}

const DEFAULT_ALBEDO: Vec3 = Vec3::new(1.0, 0.4, 0.2);

pub(crate) fn pack_brush(
    placement: &GlobalTransform,
    modifiers: Option<&Modifiers>,
    operation: Option<&CsgOperation>,
    albedo: Option<&Albedo>,
) -> GpuShape {
    let (scale, rotation, translation) = placement.to_scale_rotation_translation();
    let half_size = scale.abs().max(Vec3::splat(f32::EPSILON));
    let modifiers = modifiers.copied().unwrap_or_default();
    let operation = operation.copied().unwrap_or_default();
    let (side_radius, cap_radius) = corner_radii(&modifiers, half_size);

    let mut packed = GpuShape {
        center: translation,
        wall_thickness: wall_thickness(modifiers.thickness, footprint_of(half_size)),
        half_size,
        side_radius,
        inverse_rotation: quaternion_words(rotation.inverse()),
        albedo: albedo.map_or(DEFAULT_ALBEDO, |albedo| albedo.0),
        cap_radius,
        cull_extent: Vec3::ZERO,
        cull_scale: 1.0,
        taper: modifiers.cone,
        padding_one: 0.0,
        padding_two: 0.0,
        padding_three: 0.0,
        blend: GpuBlend {
            mode: operation.mode,
            radius: operation.radius,
            strength: operation.strength,
            chamfer: u32::from(operation.chamfer),
        },
    };
    (packed.cull_extent, packed.cull_scale) = cull_bound(&packed);
    packed
}

pub(crate) fn footprint_of(half_size: Vec3) -> f32 {
    half_size.x.min(half_size.z)
}

fn corner_radii(modifiers: &Modifiers, half_size: Vec3) -> (f32, f32) {
    let footprint = footprint_of(half_size);
    let rounded = modifiers.round * half_size.min_element();
    let bevelled = modifiers.bevel * footprint;
    (
        rounded.max(bevelled).min(footprint),
        rounded.min(half_size.y),
    )
}

fn quaternion_words(rotation: Quat) -> Vec4 {
    Vec4::new(rotation.x, rotation.y, rotation.z, rotation.w)
}

fn wall_thickness(thickness: f32, footprint: f32) -> f32 {
    if thickness >= 1.0 {
        return footprint;
    }
    thickness * footprint * 0.5
}
