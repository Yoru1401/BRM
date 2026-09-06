use bevy::prelude::*;

use crate::command_line;
use crate::sdf::blending::blend;
use crate::sdf::bounds::shape_cannot_reach;
use crate::sdf::brush::{
    GPU_MODE_ADD, GPU_MODE_INTERSECT, GPU_MODE_PAINT, GPU_MODE_SHELL, GPU_MODE_SUBTRACT, GpuShape,
    MIN_RADIUS, footprint_of,
};
use crate::sdf::distance::{MAX_MARCH_DISTANCE, rounded_box_distance, shape_distance};
use crate::sdf::field::scene_distance;

pub(crate) const GRID_DEFAULT_RESOLUTION: u32 = 16;
pub(crate) const GRID_MAX_RESOLUTION: u32 = 32;
pub(crate) const SKIP_RESOLUTION: u32 = 32;
pub(crate) const SKIP_LEVELS: u32 = 6;
pub(crate) const SKIP_WORDS: usize = 37449;
const GRID_MAX_CELLS: usize =
    (GRID_MAX_RESOLUTION * GRID_MAX_RESOLUTION * GRID_MAX_RESOLUTION) as usize;

pub(crate) const GRID_CELL_WORDS: usize = GRID_MAX_CELLS * 2;
pub(crate) const GRID_INDEX_WORDS: usize = GRID_MAX_ENTRIES;

const GRID_MAX_ENTRIES: usize = 1 << 18;

pub(crate) const GRID_CELL_FULL: u32 = u32::MAX;

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct GridSettings {
    pub(crate) resolution: u32,
    pub(crate) cell_size: Option<f32>,
    pub(crate) enabled: bool,
}

impl Default for GridSettings {
    fn default() -> Self {
        GridSettings {
            resolution: command_line::value("--grid")
                .map_or(GRID_DEFAULT_RESOLUTION, |cells| cells as u32),
            cell_size: command_line::value("--cell"),
            enabled: !command_line::flag("--no-grid"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct GridWindow {
    pub(crate) origin: Vec3,
    pub(crate) cell_size: Vec3,
    pub(crate) resolution: UVec3,
}

impl GridWindow {
    pub(crate) fn around(centre: Vec3, cell_size: f32, resolution: u32) -> Self {
        let resolution = UVec3::splat(resolution.clamp(1, GRID_MAX_RESOLUTION));
        let cell = cell_size.max(MIN_RADIUS);
        let corner = centre - resolution.as_vec3() * cell * 0.5;
        GridWindow {
            origin: (corner / cell).floor() * cell,
            cell_size: Vec3::splat(cell),
            resolution,
        }
    }

    pub(crate) fn covering(minimum: Vec3, maximum: Vec3, resolution: u32) -> Self {
        let span = (maximum - minimum).max(Vec3::splat(MIN_RADIUS));
        let cell = span.max_element() / resolution.clamp(1, GRID_MAX_RESOLUTION) as f32;
        GridWindow {
            origin: minimum,
            cell_size: Vec3::splat(cell),
            resolution: (span / cell)
                .ceil()
                .as_uvec3()
                .max(UVec3::ONE)
                .min(UVec3::splat(GRID_MAX_RESOLUTION)),
        }
    }

    fn far_corner(&self) -> Vec3 {
        self.origin + self.cell_size * self.resolution.as_vec3()
    }
}

#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub(crate) struct SdfGrid {
    pub(crate) origin: Vec3,

    pub(crate) cell_size: Vec3,

    pub(crate) resolution: UVec3,

    pub(crate) cells: Vec<u32>,
    pub(crate) indices: Vec<u32>,

    pub(crate) indexed: usize,
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

    pub(crate) fn wall_distance(&self, point: Vec3) -> f32 {
        let slot = self.slot_of(point);
        let low = self.origin + slot * self.cell_size;
        let high = low + self.cell_size;
        (point - low).min(high - point).min_element().max(0.0)
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

pub(crate) fn build_grid(shapes: &[GpuShape], window: GridWindow) -> SdfGrid {
    let cells_total = (window.resolution.x * window.resolution.y * window.resolution.z) as usize;

    let mut grid = SdfGrid {
        origin: window.origin,
        cell_size: window.cell_size,
        resolution: window.resolution,
        cells: vec![0; cells_total * 2],
        indices: Vec::new(),
        indexed: shapes.len(),
    };
    if shapes.is_empty() {
        return grid;
    }

    let margin = SdfGrid::overlap(window.cell_size);

    let range_of = |index: usize, shape: &GpuShape| -> (Vec3, Vec3) {
        if index == 0 || shape.blend.mode != GPU_MODE_ADD {
            return (window.origin, window.far_corner());
        }
        (
            shape.center - shape.cull_extent - margin,
            shape.center + shape.cull_extent + margin,
        )
    };
    let last = (window.resolution - UVec3::ONE).as_vec3();
    let slots = |corner: Vec3| -> [usize; 3] {
        let slot = ((corner - window.origin) / window.cell_size)
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
                    let cell = x
                        + y * window.resolution.x as usize
                        + z * (window.resolution.x * window.resolution.y) as usize;
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
                    let cell = x
                        + y * window.resolution.x as usize
                        + z * (window.resolution.x * window.resolution.y) as usize;
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
    skip: &SkipPyramid,
    world_point: Vec3,
) -> f32 {
    if grid.cells.len() < grid.cell_count() * 2 || shapes.is_empty() || !grid.holds(world_point) {
        let span = skip.empty_span(world_point);
        if span > 0.0 {
            return span;
        }
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

    if count as usize != grid.indexed {
        field = field.min(grid.exit_distance(world_point));
    }
    fold_unindexed(shapes, grid.indexed, world_point, field)
}

fn fold_unindexed(shapes: &[GpuShape], indexed: usize, world_point: Vec3, mut field: f32) -> f32 {
    for (offset, shape) in shapes[indexed.min(shapes.len())..].iter().enumerate() {
        let distance = shape_distance(shape, world_point);
        field = if indexed == 0 && offset == 0 {
            distance
        } else {
            blend(distance, field, &shape.blend, shape.blend.chamfer != 0)
        };
    }
    field
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ShadowProbe {
    pub(crate) occluder: f32,
    pub(crate) advance: f32,
}

#[allow(dead_code)]
pub(crate) fn shadow_proxy_distance(
    shapes: &[GpuShape],
    grid: &SdfGrid,
    world_point: Vec3,
) -> ShadowProbe {
    let bound = |shape: &GpuShape| shadow_proxy_bound(shape, world_point);
    let everything = || {
        let reach = shapes.iter().map(bound).fold(MAX_MARCH_DISTANCE, f32::min);
        ShadowProbe {
            occluder: reach,
            advance: reach,
        }
    };

    if grid.cells.len() < grid.cell_count() * 2 || shapes.is_empty() || !grid.holds(world_point) {
        return everything();
    }
    let cell = grid.cell_of(world_point);
    let count = grid.cells[cell * 2 + 1];
    if count == GRID_CELL_FULL {
        return everything();
    }

    let offset = grid.cells[cell * 2] as usize;
    let listed = (0..count as usize)
        .map(|slot| bound(&shapes[grid.indices[offset + slot] as usize]))
        .fold(MAX_MARCH_DISTANCE, f32::min);
    let occluder = shapes[grid.indexed.min(shapes.len())..]
        .iter()
        .map(bound)
        .fold(listed, f32::min);

    ShadowProbe {
        occluder,
        advance: match count as usize == grid.indexed {
            true => occluder,
            false => occluder.min(grid.exit_distance(world_point)),
        },
    }
}

fn can_add_material(shape: &GpuShape) -> bool {
    !matches!(shape.blend.mode, GPU_MODE_SUBTRACT | GPU_MODE_PAINT)
}

fn folds_the_whole_field(shape: &GpuShape) -> bool {
    matches!(shape.blend.mode, GPU_MODE_INTERSECT | GPU_MODE_SHELL)
}

#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub(crate) struct SkipPyramid {
    pub(crate) origin: Vec3,
    pub(crate) finest_cell: Vec3,
    pub(crate) levels: u32,
    pub(crate) occupied: Vec<u32>,
}

impl SkipPyramid {
    pub(crate) fn side(level: u32) -> u32 {
        (SKIP_RESOLUTION >> level).max(1)
    }

    fn offset(level: u32) -> usize {
        (0..level)
            .map(|earlier| {
                let side = Self::side(earlier) as usize;
                side * side * side
            })
            .sum()
    }

    pub(crate) fn words() -> usize {
        Self::offset(SKIP_LEVELS)
    }

    fn slot(&self, level: u32, point: Vec3) -> Option<UVec3> {
        let side = Self::side(level);
        let cell = self.finest_cell * (1u32 << level) as f32;
        let local = (point - self.origin) / cell;
        let last = UVec3::splat(side - 1).as_vec3();
        match local.cmplt(Vec3::ZERO).any() || local.cmpgt(last + Vec3::ONE).any() {
            true => None,
            false => Some(local.floor().clamp(Vec3::ZERO, last).as_uvec3()),
        }
    }

    pub(crate) fn empty_span(&self, point: Vec3) -> f32 {
        for level in (0..self.levels).rev() {
            let Some(slot) = self.slot(level, point) else {
                continue;
            };
            let side = Self::side(level) as usize;
            let index = Self::offset(level)
                + slot.x as usize
                + slot.y as usize * side
                + slot.z as usize * side * side;
            if self.occupied[index] != 0 {
                continue;
            }
            let cell = self.finest_cell * (1u32 << level) as f32;
            let low = self.origin + slot.as_vec3() * cell;
            return (point - low).min(low + cell - point).min_element().max(0.0);
        }
        0.0
    }
}

pub(crate) fn build_skip_pyramid(shapes: &[GpuShape], minimum: Vec3, maximum: Vec3) -> SkipPyramid {
    let span = (maximum - minimum).max(Vec3::splat(MIN_RADIUS));
    let finest_cell = span / SKIP_RESOLUTION as f32;
    let mut pyramid = SkipPyramid {
        origin: minimum,
        finest_cell,
        levels: SKIP_LEVELS,
        occupied: vec![0; SkipPyramid::words()],
    };

    let side = SKIP_RESOLUTION as usize;
    if shapes.iter().any(folds_the_whole_field) {
        pyramid.occupied.iter_mut().for_each(|cell| *cell = 1);
        return pyramid;
    }

    let last = Vec3::splat(side as f32 - 1.0);
    let slots = |corner: Vec3| -> [usize; 3] {
        let slot = ((corner - minimum) / finest_cell)
            .floor()
            .clamp(Vec3::ZERO, last);
        [slot.x as usize, slot.y as usize, slot.z as usize]
    };
    for shape in shapes.iter().filter(|shape| can_add_material(shape)) {
        let from = slots(shape.center - shape.cull_extent);
        let to = slots(shape.center + shape.cull_extent);
        for z in from[2]..=to[2] {
            for y in from[1]..=to[1] {
                for x in from[0]..=to[0] {
                    pyramid.occupied[x + y * side + z * side * side] = 1;
                }
            }
        }
    }

    for level in 1..SKIP_LEVELS {
        let (coarse_side, fine_side) = (
            SkipPyramid::side(level) as usize,
            SkipPyramid::side(level - 1) as usize,
        );
        let (coarse_at, fine_at) = (SkipPyramid::offset(level), SkipPyramid::offset(level - 1));
        for z in 0..coarse_side {
            for y in 0..coarse_side {
                for x in 0..coarse_side {
                    let mut occupied = 0u32;
                    for dz in 0..2 {
                        for dy in 0..2 {
                            for dx in 0..2 {
                                let (cx, cy, cz) = (x * 2 + dx, y * 2 + dy, z * 2 + dz);
                                if cx < fine_side && cy < fine_side && cz < fine_side {
                                    occupied |= pyramid.occupied[fine_at
                                        + cx
                                        + cy * fine_side
                                        + cz * fine_side * fine_side];
                                }
                            }
                        }
                    }
                    pyramid.occupied
                        [coarse_at + x + y * coarse_side + z * coarse_side * coarse_side] =
                        occupied;
                }
            }
        }
    }
    pyramid
}
