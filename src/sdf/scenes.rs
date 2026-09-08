use bevy::prelude::*;

use crate::command_line;
use crate::sdf::field::{
    MATERIAL_BODY, MATERIAL_CHALK, MATERIAL_CLAY, MATERIAL_GLASS, MATERIAL_METAL, MATERIAL_STONE,
    MATERIAL_TERRAIN,
};
use crate::sdf::solid::{Op, Shape, Solid};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scene {
    Play,
    Gym,
    Zoo,
    Museum,
}

pub(crate) fn chosen() -> Scene {
    match command_line::text("--scene").as_deref() {
        Some("gym") => Scene::Gym,
        Some("zoo") => Scene::Zoo,
        Some("museum") => Scene::Museum,
        _ => Scene::Play,
    }
}

impl Scene {
    pub(crate) fn size(self) -> Option<f32> {
        match self {
            Scene::Play => None,
            Scene::Gym => Some(140.0),
            Scene::Zoo => Some(90.0),
            Scene::Museum => Some(150.0),
        }
    }

    pub(crate) fn legend(self) -> &'static str {
        match self {
            Scene::Play => "",
            Scene::Gym => concat!(
                "GYM - what the controller can do\n",
                "+X  stairs, riser 0.25 to 3.0 m\n",
                "-X  gaps, 1 to 6 m across\n",
                "+Z  ramps, 10 20 30 40 50 degrees\n",
                "-Z  bars, clearance 1.2 to 3.0 m\n",
                "walk each until one stops you",
            ),
            Scene::Zoo => concat!(
                "ZOO - every material, every shape\n",
                "front row: one sphere per material\n",
                "back row: every solid the baker knows\n",
                "the thin post is one character tall",
            ),
            Scene::Museum => concat!(
                "MUSEUM - how the systems work\n",
                "[H] shaded / step heatmap / bricks\n",
                "[V] hide the marching quad\n",
                "west: penumbra widens with distance\n",
                "centre: bodies folded into the field\n",
                "east: spheres shrinking past the voxel\n",
                "south: subtract, smooth union, smooth subtract",
            ),
        }
    }
}

struct Bench {
    solids: Vec<Solid>,
    part: u16,
}

impl Bench {
    fn new() -> Self {
        Bench {
            solids: Vec::new(),
            part: 0,
        }
    }

    fn put(&mut self, shape: Shape, centre: Vec3, material: u32) {
        self.turned(shape, centre, Quat::IDENTITY, material);
    }

    fn turned(&mut self, shape: Shape, centre: Vec3, turn: Quat, material: u32) {
        self.solids
            .push(Solid::new(shape, centre, turn, material as u8, self.part));
        self.part = self.part.wrapping_add(1);
    }

    fn folded(&mut self, shape: Shape, centre: Vec3, material: u32, op: Op) {
        self.solids.push(
            Solid::new(shape, centre, Quat::IDENTITY, material as u8, self.part).with_op(op),
        );
        self.part = self.part.wrapping_add(1);
    }

    fn slab(&mut self, half: f32) {
        self.put(
            Shape::Box {
                half: Vec3::new(half, 1.0, half),
            },
            Vec3::new(0.0, -1.0, 0.0),
            MATERIAL_TERRAIN,
        );
    }
}

const STAIRS: [f32; 8] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 2.5, 3.0];
const GAPS: [f32; 6] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
const RAMPS: [f32; 5] = [10.0, 20.0, 30.0, 40.0, 50.0];
const BARS: [f32; 5] = [1.2, 1.6, 2.0, 2.4, 3.0];

fn gym() -> Vec<Solid> {
    let mut bench = Bench::new();
    bench.slab(65.0);

    let mut reach = 6.0;
    for rise in STAIRS {
        bench.put(
            Shape::Box {
                half: Vec3::new(2.0, rise * 0.5, 3.0),
            },
            Vec3::new(reach, rise * 0.5, 0.0),
            MATERIAL_STONE,
        );
        reach += 7.0;
    }

    let mut edge = -6.0;
    for gap in GAPS.into_iter().chain([0.0]) {
        bench.put(
            Shape::Box {
                half: Vec3::new(2.0, 1.5, 3.0),
            },
            Vec3::new(edge - 2.0, 1.5, 0.0),
            MATERIAL_METAL,
        );
        edge -= 4.0 + gap;
    }

    let mut along = 10.0;
    for degrees in RAMPS {
        let tilt = degrees.to_radians();
        bench.turned(
            Shape::Box {
                half: Vec3::new(6.0, 0.4, 3.0),
            },
            Vec3::new(0.0, tilt.sin() * 6.0, along),
            Quat::from_rotation_z(-tilt),
            MATERIAL_CLAY,
        );
        along += 9.0;
    }

    let mut back = -10.0;
    for clearance in BARS {
        bench.put(
            Shape::Box {
                half: Vec3::new(3.0, 0.35, 0.6),
            },
            Vec3::new(0.0, clearance + 0.35, back),
            MATERIAL_CHALK,
        );
        back -= 6.0;
    }

    bench.put(
        Shape::Box {
            half: Vec3::new(0.6, 6.0, 12.0),
        },
        Vec3::new(-58.0, 6.0, 0.0),
        MATERIAL_GLASS,
    );
    bench.solids
}

const PALETTE: [u32; 6] = [
    MATERIAL_TERRAIN,
    MATERIAL_STONE,
    MATERIAL_METAL,
    MATERIAL_CHALK,
    MATERIAL_CLAY,
    MATERIAL_GLASS,
];

fn zoo() -> Vec<Solid> {
    let mut bench = Bench::new();
    bench.slab(42.0);

    let row = (PALETTE.len() - 1) as f32 * 7.0;
    for (index, material) in PALETTE.iter().enumerate() {
        bench.put(
            Shape::Sphere { radius: 2.5 },
            Vec3::new(index as f32 * 7.0 - row * 0.5, 2.5, 10.0),
            *material,
        );
    }

    let catalogue = [
        Shape::Box {
            half: Vec3::splat(2.5),
        },
        Shape::Sphere { radius: 2.5 },
        Shape::Cylinder {
            radius: 2.0,
            half_height: 2.5,
        },
        Shape::Frustum {
            top: 0.6,
            bottom: 2.5,
            half_height: 2.5,
        },
        Shape::Torus {
            major: 2.2,
            minor: 0.9,
        },
    ];
    let row = (catalogue.len() - 1) as f32 * 8.0;
    for (index, shape) in catalogue.into_iter().enumerate() {
        bench.put(
            shape,
            Vec3::new(index as f32 * 8.0 - row * 0.5, 3.2, -8.0),
            MATERIAL_CHALK,
        );
    }

    bench.put(
        Shape::Cylinder {
            radius: 0.5,
            half_height: 0.9,
        },
        Vec3::new(row * 0.5 + 8.0, 0.9, -8.0),
        MATERIAL_BODY,
    );
    bench.solids
}

const SHRINKING: [f32; 7] = [4.0, 2.0, 1.0, 0.5, 0.25, 0.12, 0.06];

fn museum() -> Vec<Solid> {
    let mut bench = Bench::new();
    bench.slab(70.0);

    for (index, height) in [16.0f32, 10.0, 6.0].into_iter().enumerate() {
        bench.put(
            Shape::Cylinder {
                radius: 1.0,
                half_height: height * 0.5,
            },
            Vec3::new(-46.0 + index as f32 * 9.0, height * 0.5, 0.0),
            MATERIAL_STONE,
        );
    }
    bench.put(
        Shape::Box {
            half: Vec3::new(16.0, 0.3, 14.0),
        },
        Vec3::new(-36.0, 0.3, 22.0),
        MATERIAL_CHALK,
    );

    bench.put(
        Shape::Torus {
            major: 7.0,
            minor: 1.6,
        },
        Vec3::new(0.0, 9.0, 0.0),
        MATERIAL_GLASS,
    );

    let mut along = 22.0;
    for radius in SHRINKING {
        bench.put(
            Shape::Sphere { radius },
            Vec3::new(along, radius, 0.0),
            MATERIAL_METAL,
        );
        along += radius * 2.0 + 4.0;
    }

    bench.put(
        Shape::Box {
            half: Vec3::new(4.0, 4.0, 4.0),
        },
        Vec3::new(-20.0, 4.0, -30.0),
        MATERIAL_CLAY,
    );
    bench.folded(
        Shape::Sphere { radius: 3.2 },
        Vec3::new(-20.0, 7.5, -33.0),
        MATERIAL_CLAY,
        Op::Subtract,
    );

    bench.put(
        Shape::Sphere { radius: 3.0 },
        Vec3::new(0.0, 3.0, -34.0),
        MATERIAL_CHALK,
    );
    bench.folded(
        Shape::Sphere { radius: 2.4 },
        Vec3::new(5.5, 3.0, -34.0),
        MATERIAL_CHALK,
        Op::SmoothUnion(2.2),
    );

    bench.put(
        Shape::Box {
            half: Vec3::splat(3.0),
        },
        Vec3::new(18.0, 3.0, -34.0),
        MATERIAL_METAL,
    );
    bench.folded(
        Shape::Sphere { radius: 3.0 },
        Vec3::new(21.0, 5.5, -32.0),
        MATERIAL_METAL,
        Op::SmoothSubtract(1.5),
    );

    for (index, material) in [MATERIAL_CLAY, MATERIAL_METAL, MATERIAL_CHALK]
        .into_iter()
        .enumerate()
    {
        let across = index as f32 * 11.0 - 11.0;
        bench.put(
            Shape::Box {
                half: Vec3::new(3.5, 0.5, 3.5),
            },
            Vec3::new(across, 0.5, -28.0),
            material,
        );
        bench.put(
            Shape::Sphere { radius: 2.0 },
            Vec3::new(across, 3.0, -28.0),
            material,
        );
    }
    bench.solids
}

pub(crate) fn build(scene: Scene) -> Vec<Solid> {
    match scene {
        Scene::Play => Vec::new(),
        Scene::Gym => gym(),
        Scene::Zoo => zoo(),
        Scene::Museum => museum(),
    }
}
