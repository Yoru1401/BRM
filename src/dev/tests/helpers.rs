use bevy::prelude::*;

use crate::sdf::brush::*;

pub(crate) fn placed(placement: Transform, operation: CsgOperation) -> GpuShape {
    pack_brush(
        &GlobalTransform::from(placement),
        None,
        Some(&operation),
        None,
    )
}

pub(crate) fn shaped(placement: Transform, modifiers: Modifiers) -> GpuShape {
    pack_brush(
        &GlobalTransform::from(placement),
        Some(&modifiers),
        None,
        None,
    )
}

pub(crate) fn sphere_modifiers() -> Modifiers {
    Modifiers {
        round: 1.0,
        ..default()
    }
}

pub(crate) fn union(radius: f32) -> CsgOperation {
    CsgOperation {
        radius,
        ..default()
    }
}

pub(crate) fn march(
    field: &dyn Fn(Vec3) -> f32,
    confirm: &dyn Fn(Vec3) -> f32,
    origin: Vec3,
    direction: Vec3,
    omega: f32,
    threshold: f32,
    budget: u32,
) -> (f32, u32) {
    const STOP: f32 = 60.0;

    let mut travelled = 0.0;
    let mut steps = 0;
    let mut relaxation = omega.max(1.0);
    let mut previous_distance = 0.0;
    let mut step_length = 0.0;

    while steps < budget {
        let distance = field(origin + direction * travelled);
        let overshot = relaxation > 1.0 && (distance.abs() + previous_distance) < step_length;
        steps += 1;

        if !overshot && distance < threshold && confirm(origin + direction * travelled) < threshold
        {
            return (travelled + distance, steps);
        }
        if overshot {
            step_length *= 1.0 - relaxation;
            relaxation = 1.0;
        } else {
            step_length = distance * relaxation;
        }
        previous_distance = distance.abs();
        travelled += step_length;
        if travelled >= STOP {
            break;
        }
    }

    (STOP, steps)
}

pub(crate) fn pack(operation: CsgOperation) -> GpuBlend {
    GpuBlend {
        mode: operation.mode,
        radius: operation.radius,
        strength: operation.strength,
        chamfer: u32::from(operation.chamfer),
    }
}
