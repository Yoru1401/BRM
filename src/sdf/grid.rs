use bevy::prelude::*;

use crate::command_line;
use crate::sdf::blending::blend;
use crate::sdf::bounds::shape_cannot_reach;
use crate::sdf::brush::{GPU_MODE_ADD, GpuShape, MIN_RADIUS, footprint_of};
use crate::sdf::distance::{MAX_MARCH_DISTANCE, rounded_box_distance, shape_distance};
use crate::sdf::field::scene_distance;

pub(crate) const GRID_DEFAULT_RESOLUTION: u32 = 16;
pub(crate) const GRID_MAX_RESOLUTION: u32 = 32;
const GRID_MAX_CELLS: usize =
    (GRID_MAX_RESOLUTION * GRID_MAX_RESOLUTION * GRID_MAX_RESOLUTION) as usize;

pub(crate) const GRID_CELL_WORDS: usize = GRID_MAX_CELLS * 2;
pub(crate) const GRID_INDEX_WORDS: usize = GRID_MAX_ENTRIES;

const GRID_MAX_ENTRIES: usize = 1 << 18;

pub(crate) const GRID_CELL_FULL: u32 = u32::MAX;

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct GridSettings {
    pub(crate) resolution: u32,
    pub(crate) enabled: bool,
}

impl Default for GridSettings {
    fn default() -> Self {
        GridSettings {
            resolution: command_line::value("--grid")
                .map_or(GRID_DEFAULT_RESOLUTION, |cells| cells as u32),
            enabled: !command_line::flag("--no-grid"),
        }
    }
}

#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub(crate) struct SdfGrid {
    pub(crate) origin: Vec3,

    pub(crate) cell_size: Vec3,

    pub(crate) resolution: UVec3,

    pub(crate) cells: Vec<u32>,
    pub(crate) indices: Vec<u32>,
}

#[allow(dead_code)]
impl SdfGrid {
    fn cell_count(&self) -> usize {
        (self.resolution.x * self.resolution.y * self.resolution.z) as usize
    }

    pub(crate) fn cell_of(&self, point: Vec3) -> usize {
        let slot = self.slot_of(point);
        (slot.x as usize)
            + (slot.y as usize) * self.resolution.x as usize
            + (slot.z as usize) * (self.resolution.x * self.resolution.y) as usize
    }

    fn slot_of(&self, point: Vec3) -> Vec3 {
        let last = (self.resolution - UVec3::ONE).as_vec3();
        ((point - self.origin) / self.cell_size)
            .floor()
            .clamp(Vec3::ZERO, last)
    }

    pub(crate) fn holds(&self, point: Vec3) -> bool {
        let high = self.origin + self.cell_size * self.resolution.as_vec3();
        point.cmpge(self.origin).all() && point.cmple(high).all()
    }

    pub(crate) fn overlap(cell_size: Vec3) -> Vec3 {
        cell_size * 0.5
    }

    pub(crate) fn exit_distance(&self, point: Vec3) -> f32 {
        let slot = self.slot_of(point);
        let overlap = Self::overlap(self.cell_size);
        let low = self.origin + slot * self.cell_size - overlap;
        let high = self.origin + (slot + Vec3::ONE) * self.cell_size + overlap;
        let to_wall = (point - low).min(high - point);
        to_wall.min_element().max(0.0)
    }
}

pub(crate) fn build_grid(
    shapes: &[GpuShape],
    bounds_min: Vec3,
    bounds_max: Vec3,
    resolution: u32,
) -> SdfGrid {
    let requested = resolution.clamp(1, GRID_MAX_RESOLUTION);
    let span = (bounds_max - bounds_min).max(Vec3::splat(MIN_RADIUS));

    let side = span.max_element() / requested as f32;
    let cell_size = Vec3::splat(side);
    let resolution = (span / side)
        .ceil()
        .as_uvec3()
        .max(UVec3::ONE)
        .min(UVec3::splat(GRID_MAX_RESOLUTION));
    let cells_total = (resolution.x * resolution.y * resolution.z) as usize;

    let mut grid = SdfGrid {
        origin: bounds_min,
        cell_size,
        resolution,
        cells: vec![0; cells_total * 2],
        indices: Vec::new(),
    };
    if shapes.is_empty() {
        return grid;
    }

    let margin = SdfGrid::overlap(cell_size);

    let range_of = |index: usize, shape: &GpuShape| -> (Vec3, Vec3) {
        if index == 0 || shape.blend.mode != GPU_MODE_ADD {
            return (bounds_min, bounds_max);
        }
        (
            shape.center - shape.cull_extent - margin,
            shape.center + shape.cull_extent + margin,
        )
    };
    let last = (resolution - UVec3::ONE).as_vec3();
    let slots = |corner: Vec3| -> [usize; 3] {
        let slot = ((corner - bounds_min) / cell_size)
            .floor()
            .clamp(Vec3::ZERO, last);
        [slot.x as usize, slot.y as usize, slot.z as usize]
    };

    let mut counts = vec![0u32; cells_total];
    let mut total = 0usize;
    for (index, shape) in shapes.iter().enumerate() {
        let (low, high) = range_of(index, shape);
        let (from, to) = (slots(low), slots(high));
        for z in from[2]..=to[2] {
            for y in from[1]..=to[1] {
                for x in from[0]..=to[0] {
                    let cell =
                        x + y * resolution.x as usize + z * (resolution.x * resolution.y) as usize;
                    counts[cell] += 1;
                    total += 1;
                }
            }
        }
    }

    if total > GRID_MAX_ENTRIES {
        for cell in 0..cells_total {
            grid.cells[cell * 2 + 1] = GRID_CELL_FULL;
        }
        return grid;
    }

    let mut offset = 0u32;
    for (cell, count) in counts.iter().enumerate() {
        grid.cells[cell * 2] = offset;
        offset += count;
    }
    grid.indices = vec![0; total];

    let mut written = vec![0u32; cells_total];
    for (index, shape) in shapes.iter().enumerate() {
        let (low, high) = range_of(index, shape);
        let (from, to) = (slots(low), slots(high));
        for z in from[2]..=to[2] {
            for y in from[1]..=to[1] {
                for x in from[0]..=to[0] {
                    let cell =
                        x + y * resolution.x as usize + z * (resolution.x * resolution.y) as usize;
                    let at = (grid.cells[cell * 2] + written[cell]) as usize;
                    grid.indices[at] = index as u32;
                    written[cell] += 1;
                }
            }
        }
    }
    for (cell, count) in written.iter().enumerate() {
        grid.cells[cell * 2 + 1] = *count;
    }
    grid
}

#[allow(dead_code)]
pub(crate) fn scene_distance_gridded(
    shapes: &[GpuShape],
    grid: &SdfGrid,
    world_point: Vec3,
) -> f32 {
    if grid.cells.len() < grid.cell_count() * 2 || shapes.is_empty() || !grid.holds(world_point) {
        return scene_distance(shapes, world_point);
    }
    let cell = grid.cell_of(world_point);
    let count = grid.cells[cell * 2 + 1];
    if count == GRID_CELL_FULL {
        return scene_distance(shapes, world_point);
    }

    let offset = grid.cells[cell * 2] as usize;
    let mut field = MAX_MARCH_DISTANCE;
    let mut evaluated = 0;
    for slot in 0..count as usize {
        let shape = &shapes[grid.indices[offset + slot] as usize];
        if evaluated > 0 && shape_cannot_reach(shape, world_point, field) {
            continue;
        }
        let distance = shape_distance(shape, world_point);
        field = if evaluated == 0 {
            distance
        } else {
            blend(distance, field, &shape.blend, shape.blend.chamfer != 0)
        };
        evaluated += 1;
    }

    if count as usize == shapes.len() {
        return field;
    }
    field.min(grid.exit_distance(world_point))
}

#[allow(dead_code)]
pub(crate) fn shadow_proxy_bound(shape: &GpuShape, world_point: Vec3) -> f32 {
    if shape.blend.mode != GPU_MODE_ADD {
        return MAX_MARCH_DISTANCE;
    }
    let local_point = Quat::from_vec4(shape.inverse_rotation) * (world_point - shape.center);

    rounded_box_distance(
        local_point,
        shape.half_size,
        footprint_of(shape.half_size),
        shape.side_radius,
        shape.cap_radius,
    )
}

#[allow(dead_code)]
pub(crate) fn shadow_proxy_distance(shapes: &[GpuShape], grid: &SdfGrid, world_point: Vec3) -> f32 {
    let bound = |shape: &GpuShape| shadow_proxy_bound(shape, world_point);
    let everything = || shapes.iter().map(bound).fold(MAX_MARCH_DISTANCE, f32::min);

    if grid.cells.len() < grid.cell_count() * 2 || shapes.is_empty() || !grid.holds(world_point) {
        return everything();
    }
    let cell = grid.cell_of(world_point);
    let count = grid.cells[cell * 2 + 1];
    if count == GRID_CELL_FULL {
        return everything();
    }

    let offset = grid.cells[cell * 2] as usize;
    let field = (0..count as usize)
        .map(|slot| bound(&shapes[grid.indices[offset + slot] as usize]))
        .fold(MAX_MARCH_DISTANCE, f32::min);

    if count as usize == shapes.len() {
        return field;
    }
    field.min(grid.exit_distance(world_point))
}
