use bevy::math::{Quat, Vec2, Vec3};

#[derive(Clone, Copy)]
pub(crate) enum Shape {
    Box {
        half: Vec3,
    },
    Sphere {
        radius: f32,
    },
    Cylinder {
        radius: f32,
        half_height: f32,
    },
    Frustum {
        top: f32,
        bottom: f32,
        half_height: f32,
    },
    Torus {
        major: f32,
        minor: f32,
    },
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Op {
    Union,
    Subtract,
    Intersect,
    SmoothUnion(f32),
    SmoothSubtract(f32),
}

impl Op {
    pub(crate) fn fold(self, standing: f32, edit: f32) -> f32 {
        match self {
            Op::Union => standing.min(edit),
            Op::Subtract => standing.max(-edit),
            Op::Intersect => standing.max(edit),
            Op::SmoothUnion(width) => soft_min(standing, edit, width),
            Op::SmoothSubtract(width) => -soft_min(-standing, edit, width),
        }
    }

    pub(crate) fn everywhere(self) -> bool {
        self == Op::Intersect
    }

    pub(crate) fn width(self) -> f32 {
        match self {
            Op::SmoothUnion(width) | Op::SmoothSubtract(width) => width,
            _ => 0.0,
        }
    }

    pub(crate) fn carves(self) -> bool {
        matches!(self, Op::Subtract | Op::SmoothSubtract(_))
    }
}

fn soft_min(first: f32, second: f32, width: f32) -> f32 {
    if width <= 0.0 {
        return first.min(second);
    }
    let blend = (0.5 + 0.5 * (second - first) / width).clamp(0.0, 1.0);
    second * (1.0 - blend) + first * blend - width * blend * (1.0 - blend)
}

#[derive(Clone, Copy)]
pub(crate) struct Solid {
    shape: Shape,
    centre: Vec3,
    unturn: Quat,
    pub(crate) material: u8,
    pub(crate) part: u16,
    pub(crate) op: Op,
}

impl Solid {
    pub(crate) fn new(shape: Shape, centre: Vec3, turn: Quat, material: u8, part: u16) -> Self {
        Solid {
            shape,
            centre,
            unturn: turn.inverse(),
            material,
            part,
            op: Op::Union,
        }
    }

    pub(crate) fn with_op(mut self, op: Op) -> Self {
        self.op = op;
        self
    }

    pub(crate) fn distance(&self, point: Vec3) -> f32 {
        self.shape.distance(self.unturn * (point - self.centre))
    }

    pub(crate) fn bounds(&self) -> (Vec3, Vec3) {
        let half = self.shape.half_extent();
        let turn = self.unturn.inverse();
        let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for corner in 0..8 {
            let sign = Vec3::new(
                (corner & 1) as f32 * 2.0 - 1.0,
                (corner >> 1 & 1) as f32 * 2.0 - 1.0,
                (corner >> 2 & 1) as f32 * 2.0 - 1.0,
            );
            let at = self.centre + turn * (half * sign);
            low = low.min(at);
            high = high.max(at);
        }
        (low, high)
    }

    pub(crate) fn scaled(&self, centre: Vec3, scale: f32) -> Self {
        Solid {
            shape: self.shape.scaled(scale),
            centre: (self.centre - centre) * scale,
            ..*self
        }
    }
}

impl Shape {
    fn distance(&self, point: Vec3) -> f32 {
        match *self {
            Shape::Box { half } => {
                let outside = point.abs() - half;
                outside.max(Vec3::ZERO).length() + outside.max_element().min(0.0)
            }
            Shape::Sphere { radius } => point.length() - radius,
            Shape::Cylinder {
                radius,
                half_height,
            } => {
                let outside = Vec2::new(
                    Vec2::new(point.x, point.z).length() - radius,
                    point.y.abs() - half_height,
                );
                outside.max(Vec2::ZERO).length() + outside.max_element().min(0.0)
            }
            Shape::Frustum {
                top,
                bottom,
                half_height,
            } => {
                let flat = Vec2::new(Vec2::new(point.x, point.z).length(), point.y);
                let cap = match flat.y < 0.0 {
                    true => bottom,
                    false => top,
                };
                let radial = Vec2::new(flat.x - flat.x.min(cap), flat.y.abs() - half_height);
                let rim = Vec2::new(top, half_height);
                let slope = Vec2::new(top - bottom, 2.0 * half_height);
                let along = flat - rim
                    + slope * ((rim - flat).dot(slope) / slope.length_squared().max(1e-12)).clamp(0.0, 1.0);
                let inside = along.x < 0.0 && radial.y < 0.0;
                let reach = radial.length_squared().min(along.length_squared()).sqrt();
                match inside {
                    true => -reach,
                    false => reach,
                }
            }
            Shape::Torus { major, minor } => {
                let ring = Vec2::new(Vec2::new(point.x, point.z).length() - major, point.y);
                ring.length() - minor
            }
        }
    }

    fn half_extent(&self) -> Vec3 {
        match *self {
            Shape::Box { half } => half,
            Shape::Sphere { radius } => Vec3::splat(radius),
            Shape::Cylinder {
                radius,
                half_height,
            } => Vec3::new(radius, half_height, radius),
            Shape::Frustum {
                top,
                bottom,
                half_height,
            } => {
                let widest = top.max(bottom);
                Vec3::new(widest, half_height, widest)
            }
            Shape::Torus { major, minor } => Vec3::new(major + minor, minor, major + minor),
        }
    }

    fn scaled(&self, scale: f32) -> Self {
        match *self {
            Shape::Box { half } => Shape::Box { half: half * scale },
            Shape::Sphere { radius } => Shape::Sphere {
                radius: radius * scale,
            },
            Shape::Cylinder {
                radius,
                half_height,
            } => Shape::Cylinder {
                radius: radius * scale,
                half_height: half_height * scale,
            },
            Shape::Frustum {
                top,
                bottom,
                half_height,
            } => Shape::Frustum {
                top: top * scale,
                bottom: bottom * scale,
                half_height: half_height * scale,
            },
            Shape::Torus { major, minor } => Shape::Torus {
                major: major * scale,
                minor: minor * scale,
            },
        }
    }
}
