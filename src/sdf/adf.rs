use bevy::{
    mesh::{Indices, VertexAttributeValues},
    prelude::*,
    tasks::{ComputeTaskPool, TaskPool},
};

use crate::command_line;
use crate::sdf::solid::Solid;

pub(crate) const BRICK: u32 = 8;
pub(crate) const APRON: u32 = 1;
pub(crate) const SPAN: u32 = BRICK + 2 * APRON;
pub(crate) const MAX_SLOT_SIDE: u32 = 2048 / SPAN;
pub(crate) const PAGE_LIMIT: usize = 8 << 20;

pub(crate) const TAG_EMPTY: u32 = 0xff;
pub(crate) const TAG_SOLID: u32 = 0xfe;
pub(crate) const TAG_SHIFT: u32 = 24;
pub(crate) const SLOT_MASK: u32 = 0x0f_ffff;
pub(crate) const MAX_MATERIALS: usize = 16;
pub(crate) const EMPTY: u32 = (TAG_EMPTY << TAG_SHIFT) | 1;

const MARGIN: f32 = 2.0;
const TARGET_VOXELS: f32 = 2048.0;
const COARSEN: f32 = 1.5;
const RANGE_VOXELS: f32 = 4.0;
pub(crate) const BIAS_VOXELS: f32 = 0.866;
const NORMAL_TAP: f32 = 1.0;
const BRICK_BUDGET: u32 = 150_000;

pub(crate) fn brick_budget() -> u32 {
    command_line::value("--bricks")
        .map_or(BRICK_BUDGET, |count| count as u32)
        .clamp(1, (MAX_SLOT_SIDE * MAX_SLOT_SIDE * MAX_SLOT_SIDE).min(SLOT_MASK))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Material {
    pub(crate) albedo: Vec3,
    pub(crate) gloss: f32,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            albedo: Vec3::splat(0.72),
            gloss: 0.08,
        }
    }
}

#[derive(Resource, Clone)]
pub(crate) struct Adf {
    pub(crate) origin: Vec3,
    pub(crate) voxel: f32,
    pub(crate) bricks: UVec3,
    pub(crate) slots: u32,
    pub(crate) page: Vec<u32>,
    pub(crate) atlas: Vec<u8>,
    pub(crate) paint: Vec<u8>,
    pub(crate) coarse: Vec<f32>,
    pub(crate) palette: Vec<Material>,
    pub(crate) used: u32,
}

impl Default for Adf {
    fn default() -> Self {
        Adf {
            origin: Vec3::ZERO,
            voxel: 1.0,
            bricks: UVec3::ONE,
            slots: 1,
            page: vec![EMPTY],
            atlas: vec![0; (SPAN * SPAN * SPAN) as usize],
            paint: vec![0; (BRICK * BRICK * BRICK) as usize],
            coarse: vec![0.0],
            palette: vec![Material::default()],
            used: 0,
        }
    }
}

impl Adf {
    pub(crate) fn brick_size(&self) -> f32 {
        self.voxel * BRICK as f32
    }

    pub(crate) fn range(&self) -> f32 {
        self.voxel * RANGE_VOXELS
    }

    pub(crate) fn atlas_side(&self) -> u32 {
        self.slots * SPAN
    }

    pub(crate) fn paint_side(&self) -> u32 {
        self.slots * BRICK
    }

    pub(crate) fn bounds(&self) -> (Vec3, Vec3) {
        (
            self.origin,
            self.origin + self.bricks.as_vec3() * self.brick_size(),
        )
    }

    pub(crate) fn distance(&self, world_point: Vec3) -> f32 {
        self.probe(world_point).0
    }

    pub(crate) fn probe(&self, world_point: Vec3) -> (f32, bool) {
        let size = self.brick_size();
        let local = (world_point - self.origin) / size;
        let last = self.bricks.as_vec3() - Vec3::ONE;
        if local.cmplt(Vec3::ZERO).any() || local.cmpgt(last + Vec3::ONE).any() {
            let (low, high) = self.bounds();
            let centre = (low + high) * 0.5;
            return (
                outside_box(world_point - centre, (high - low) * 0.5).max(self.voxel),
                false,
            );
        }
        let cell = local.floor().clamp(Vec3::ZERO, last);
        let index = cell.x as usize
            + cell.y as usize * self.bricks.x as usize
            + cell.z as usize * (self.bricks.x * self.bricks.y) as usize;
        let word = self.page[index];
        let tag = word >> TAG_SHIFT;
        if tag >= TAG_SOLID {
            let low = self.origin + cell * size;
            let gap = (world_point - low).min(low + Vec3::splat(size) - world_point);
            let room = gap.min_element().max(0.0) + self.coarse[index] + self.range();
            return match tag == TAG_SOLID {
                true => (-room, false),
                false => (room, false),
            };
        }
        let inside = (local - cell).clamp(Vec3::ZERO, Vec3::ONE) * BRICK as f32 + APRON as f32;
        let range = self.range();
        let unit = self.fetch(word & SLOT_MASK, inside);
        (unit * 2.0 * range - range, unit > 0.002 && unit < 0.998)
    }

    pub(crate) fn normal(&self, world_point: Vec3) -> Vec3 {
        let step = self.voxel * NORMAL_TAP;
        TETRAHEDRON
            .iter()
            .map(|corner| *corner * self.distance(world_point + *corner * step))
            .sum::<Vec3>()
            .normalize_or_zero()
    }

    fn fetch(&self, slot: u32, inside: Vec3) -> f32 {
        let base = slot_origin(slot, self.slots).as_vec3();
        let low = (base + inside.floor().min(Vec3::splat((SPAN - 2) as f32))).as_uvec3();
        let fraction = inside.fract();
        let side = self.atlas_side();
        let mut total = 0.0;
        for corner in 0..8u32 {
            let step = UVec3::new(corner & 1, (corner >> 1) & 1, corner >> 2);
            let weight = (0..3)
                .map(|axis| match step[axis] {
                    0 => 1.0 - fraction[axis],
                    _ => fraction[axis],
                })
                .product::<f32>();
            total += weight * f32::from(self.atlas[atlas_index(low + step, side)]) / 255.0;
        }
        total
    }
}

const TETRAHEDRON: [Vec3; 4] = [
    Vec3::new(1.0, -1.0, -1.0),
    Vec3::new(-1.0, -1.0, 1.0),
    Vec3::new(-1.0, 1.0, -1.0),
    Vec3::new(1.0, 1.0, 1.0),
];

fn outside_box(offset: Vec3, half_extent: Vec3) -> f32 {
    (offset.abs() - half_extent).max(Vec3::ZERO).length()
}

fn slot_origin(slot: u32, slots: u32) -> UVec3 {
    UVec3::new(slot % slots, (slot / slots) % slots, slot / (slots * slots)) * SPAN
}

fn atlas_index(texel: UVec3, side: u32) -> usize {
    (texel.x + texel.y * side + texel.z * side * side) as usize
}

pub(crate) struct Surface<'a> {
    pub(crate) triangles: &'a [[Vec3; 3]],
    pub(crate) solids: &'a [Solid],
    materials: &'a [u8],
    parts: &'a [u16],
    boxes: Vec<(Vec3, Vec3)>,
}

impl<'a> Surface<'a> {
    pub(crate) fn new(triangles: &'a [[Vec3; 3]], materials: &'a [u8], parts: &'a [u16]) -> Self {
        Surface {
            triangles,
            solids: &[],
            materials,
            parts,
            boxes: triangles
                .iter()
                .map(|triangle| {
                    (
                        triangle[0].min(triangle[1]).min(triangle[2]),
                        triangle[0].max(triangle[1]).max(triangle[2]),
                    )
                })
                .collect(),
        }
    }

    pub(crate) fn with_solids(mut self, solids: &'a [Solid]) -> Self {
        self.boxes.extend(solids.iter().map(Solid::bounds));
        self.solids = solids;
        self
    }

    fn solid(&self, index: u32) -> Option<&Solid> {
        self.solids.get((index as usize).checked_sub(self.triangles.len())?)
    }

    fn part(&self, index: u32) -> u16 {
        match self.solid(index) {
            Some(solid) => solid.part,
            None => self.parts.get(index as usize).copied().unwrap_or(0),
        }
    }

    fn material(&self, index: u32) -> u8 {
        self.materials.get(index as usize).copied().unwrap_or(0)
    }
}

pub(crate) fn triangles_of(mesh: &Mesh) -> (Vec<[Vec3; 3]>, Vec<u32>) {
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return (Vec::new(), Vec::new());
    };
    let points: Vec<Vec3> = positions.iter().map(|point| Vec3::from(*point)).collect();
    let corners: Vec<usize> = match mesh.indices() {
        Some(Indices::U16(list)) => list.iter().map(|index| *index as usize).collect(),
        Some(Indices::U32(list)) => list.iter().map(|index| *index as usize).collect(),
        None => (0..points.len()).collect(),
    };
    corners
        .chunks_exact(3)
        .enumerate()
        .filter_map(|(source, corner)| {
            let triangle = [
                *points.get(corner[0])?,
                *points.get(corner[1])?,
                *points.get(corner[2])?,
            ];
            let area = (triangle[1] - triangle[0]).cross(triangle[2] - triangle[0]);
            (area.length_squared() > 1e-20).then_some((triangle, source as u32))
        })
        .unzip()
}

pub(crate) fn fit(triangles: &mut [[Vec3; 3]], solids: &mut [Solid], widest: f32) {
    let (mut low, mut high) = span_of(triangles);
    for solid in solids.iter() {
        let (corner_low, corner_high) = solid.bounds();
        low = low.min(corner_low);
        high = high.max(corner_high);
    }
    let scale = widest / (high - low).max_element().max(1e-6);
    let centre = (low + high) * 0.5;
    for triangle in triangles.iter_mut() {
        for corner in triangle.iter_mut() {
            *corner = (*corner - centre) * scale;
        }
    }
    for solid in solids.iter_mut() {
        *solid = solid.scaled(centre, scale);
    }
}

fn span_of(triangles: &[[Vec3; 3]]) -> (Vec3, Vec3) {
    let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for triangle in triangles {
        for corner in triangle {
            low = low.min(*corner);
            high = high.max(*corner);
        }
    }
    (low, high)
}

pub(crate) fn bake(surface: &Surface) -> Adf {
    let asked = command_line::value("--voxel");
    bake_at(surface, asked)
}

pub(crate) fn bake_at(surface: &Surface, asked: Option<f32>) -> Adf {
    if surface.triangles.is_empty() && surface.solids.is_empty() {
        return Adf::default();
    }
    let (mut low, mut high) = span_of(surface.triangles);
    for solid in surface.solids {
        let (corner_low, corner_high) = solid.bounds();
        low = low.min(corner_low);
        high = high.max(corner_high);
    }
    let widest = (high - low).max_element().max(1e-3);
    let mut voxel = asked.unwrap_or(widest / TARGET_VOXELS);
    let budget = brick_budget();
    loop {
        if let Some(baked) = try_bake(surface, low, high, voxel, budget) {
            warn_about_thin_parts(surface, baked.voxel);
            return baked;
        }
        voxel *= COARSEN;
    }
}

const THIN_VOXELS: f32 = 4.0;

fn warn_about_thin_parts(surface: &Surface, voxel: f32) {
    let mut span: Vec<(Vec3, Vec3)> = Vec::new();
    for (index, triangle) in surface.triangles.iter().enumerate() {
        let part = surface.part(index as u32) as usize;
        if span.len() <= part {
            span.resize(part + 1, (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)));
        }
        for corner in triangle {
            span[part].0 = span[part].0.min(*corner);
            span[part].1 = span[part].1.max(*corner);
        }
    }

    let thinnest = voxel * THIN_VOXELS;
    let mut thin = 0;
    let mut worst = f32::MAX;
    for (low, high) in &span {
        if low.x > high.x {
            continue;
        }
        let width = (*high - *low).min_element();
        if width < thinnest {
            thin += 1;
            worst = worst.min(width);
        }
    }
    if thin > 0 {
        warn!(
            "adf: {thin} parts are thinner than {THIN_VOXELS} voxels ({thinnest:.3} m),              the thinnest {worst:.3} m; they will bake as noise. Raise --bricks or the model scale."
        );
    }
}

type Brick = (usize, Vec<u8>, Vec<u8>);

fn try_bake(surface: &Surface, low: Vec3, high: Vec3, voxel: f32, budget: u32) -> Option<Adf> {
    let size = voxel * BRICK as f32;
    let range = voxel * RANGE_VOXELS;
    let margin = Vec3::splat(size * MARGIN);
    let origin = low - margin;
    let bricks = ((high + margin - origin) / size)
        .ceil()
        .as_uvec3()
        .max(UVec3::ONE);

    let count = (bricks.x as usize) * (bricks.y as usize) * (bricks.z as usize);
    if count > PAGE_LIMIT {
        return None;
    }

    let mut lists: Vec<Vec<u32>> = vec![Vec::new(); count];
    let last = bricks.as_vec3() - Vec3::ONE;
    let reach = Vec3::splat(range + voxel);
    let brick_of = |point: Vec3| ((point - origin) / size).floor().clamp(Vec3::ZERO, last).as_uvec3();
    let skin = size * 0.866_025_4 + range + voxel;
    for (index, (corner_low, corner_high)) in surface.boxes.iter().enumerate() {
        let first = brick_of(*corner_low - reach);
        let final_cell = brick_of(*corner_high + reach);
        let solid = surface.solid(index as u32);
        for z in first.z..=final_cell.z {
            for y in first.y..=final_cell.y {
                for x in first.x..=final_cell.x {
                    if let Some(solid) = solid {
                        let middle = origin
                            + (UVec3::new(x, y, z).as_vec3() + Vec3::splat(0.5)) * size;
                        if solid.distance(middle).abs() > skin {
                            continue;
                        }
                    }
                    lists[(x + y * bricks.x + z * bricks.x * bricks.y) as usize].push(index as u32);
                }
            }
        }
    }

    for list in lists.iter_mut() {
        list.sort_unstable_by_key(|index| surface.part(*index));
    }
    let buried = buried_bricks(surface, &lists, bricks);

    let mut candidates = Vec::new();
    for (index, list) in lists.iter().enumerate() {
        if list.is_empty() || buried[index] {
            continue;
        }
        let cell = UVec3::new(
            (index as u32) % bricks.x,
            ((index as u32) / bricks.x) % bricks.y,
            (index as u32) / (bricks.x * bricks.y),
        );
        candidates.push((index, origin + cell.as_vec3() * size, list));
    }

    let bias = BIAS_VOXELS * voxel;
    let tie = (voxel * 1e-3).powi(2);
    let pool = ComputeTaskPool::get_or_init(TaskPool::default);
    let batch = candidates.len().div_ceil(pool.thread_num().max(1) * 8).max(1);
    let blocks: Vec<Vec<Brick>> = pool.scope(|scope| {
        for batch in candidates.chunks(batch) {
            scope.spawn(async move {
                batch
                    .iter()
                    .map(|(index, corner, list)| {
                        let (voxels, painted) =
                            brick_voxels(*corner, voxel, range, bias, tie, surface, list);
                        (*index, voxels, painted)
                    })
                    .collect()
            });
        }
    });
    let kept: Vec<Brick> = blocks.into_iter().flatten().collect();

    let used = kept.len() as u32;
    if used > budget {
        return None;
    }
    let slots = (used.max(1) as f32).cbrt().ceil() as u32;
    if slots > MAX_SLOT_SIDE {
        return None;
    }
    let side = slots * SPAN;
    let paint_side = slots * BRICK;

    let mut page = vec![EMPTY; count];
    let mut atlas = vec![0u8; (side as usize).pow(3)];
    let mut paint = vec![0u8; (paint_side as usize).pow(3)];
    for (slot, (index, voxels, painted)) in kept.into_iter().enumerate() {
        let slot = slot as u32;
        page[index] = slot;
        let base = slot_origin(slot, slots);
        let paint_base =
            UVec3::new(slot % slots, (slot / slots) % slots, slot / (slots * slots)) * BRICK;
        for z in 0..SPAN {
            for y in 0..SPAN {
                for x in 0..SPAN {
                    atlas[atlas_index(base + UVec3::new(x, y, z), side)] =
                        voxels[(x + y * SPAN + z * SPAN * SPAN) as usize];
                }
            }
        }
        for z in 0..BRICK {
            for y in 0..BRICK {
                for x in 0..BRICK {
                    paint[atlas_index(paint_base + UVec3::new(x, y, z), paint_side)] =
                        painted[(x + y * BRICK + z * BRICK * BRICK) as usize];
                }
            }
        }
    }

    seal_interior(&mut page, bricks);
    let near: Vec<bool> = lists.iter().map(|list| !list.is_empty()).collect();
    let coarse = measure_coarse(&near, bricks, size);

    Some(Adf {
        origin,
        voxel,
        bricks,
        slots,
        page,
        atlas,
        paint,
        coarse,
        palette: vec![Material::default()],
        used,
    })
}

fn buried_bricks(surface: &Surface, lists: &[Vec<u32>], bricks: UVec3) -> Vec<bool> {
    let width = bricks.x as usize;
    let plane = width * bricks.y as usize;
    let at = |cell: UVec3| {
        cell.x as usize + cell.y as usize * width + cell.z as usize * plane
    };
    let holds = |index: usize, part: u16| {
        lists[index]
            .binary_search_by_key(&part, |triangle| surface.part(*triangle))
            .is_ok()
    };

    let mut span: Vec<Option<(UVec3, UVec3)>> = Vec::new();
    for (index, list) in lists.iter().enumerate() {
        let cell = UVec3::new(
            (index % width) as u32,
            ((index / width) % bricks.y as usize) as u32,
            (index / plane) as u32,
        );
        for triangle in list {
            let part = surface.part(*triangle) as usize;
            if span.len() <= part {
                span.resize(part + 1, None);
            }
            span[part] = Some(match span[part] {
                Some((low, high)) => (low.min(cell), high.max(cell)),
                None => (cell, cell),
            });
        }
    }

    let mut buried = vec![false; lists.len()];
    let mut reached = vec![false; lists.len()];
    let mut queue: Vec<usize> = Vec::new();

    for (part, range) in span.iter().enumerate() {
        let Some((low, high)) = *range else {
            continue;
        };
        let low = low.saturating_sub(UVec3::ONE);
        let high = (high + UVec3::ONE).min(bricks - UVec3::ONE);
        let part = part as u16;

        queue.clear();
        for z in low.z..=high.z {
            for y in low.y..=high.y {
                for x in low.x..=high.x {
                    let cell = UVec3::new(x, y, z);
                    let index = at(cell);
                    reached[index] = false;
                    let edge = cell.cmpeq(low).any() || cell.cmpeq(high).any();
                    if edge && !holds(index, part) {
                        reached[index] = true;
                        queue.push(index);
                    }
                }
            }
        }

        let mut frontier = 0;
        while frontier < queue.len() {
            let index = queue[frontier];
            frontier += 1;
            let cell = IVec3::new(
                (index % width) as i32,
                ((index / width) % bricks.y as usize) as i32,
                (index / plane) as i32,
            );
            for step in NEIGHBOURS {
                let next = cell + step;
                if next.cmplt(low.as_ivec3()).any() || next.cmpgt(high.as_ivec3()).any() {
                    continue;
                }
                let ahead = at(next.as_uvec3());
                if reached[ahead] || holds(ahead, part) {
                    continue;
                }
                reached[ahead] = true;
                queue.push(ahead);
            }
        }

        for z in low.z..=high.z {
            for y in low.y..=high.y {
                for x in low.x..=high.x {
                    let index = at(UVec3::new(x, y, z));
                    if !reached[index] && !holds(index, part) {
                        buried[index] = true;
                    }
                }
            }
        }
    }
    buried
}

fn seal_interior(page: &mut [u32], bricks: UVec3) {
    let width = bricks.x as usize;
    let plane = width * bricks.y as usize;
    let mut reached = vec![false; page.len()];
    let mut queue: Vec<usize> = Vec::new();

    for (index, slot) in page.iter().enumerate() {
        let cell = UVec3::new(
            (index % width) as u32,
            ((index / width) % bricks.y as usize) as u32,
            (index / plane) as u32,
        );
        let edge = cell.cmpeq(UVec3::ZERO).any() || cell.cmpeq(bricks - UVec3::ONE).any();
        if edge && *slot >> TAG_SHIFT == TAG_EMPTY {
            reached[index] = true;
            queue.push(index);
        }
    }

    while let Some(index) = queue.pop() {
        let cell = IVec3::new(
            (index % width) as i32,
            ((index / width) % bricks.y as usize) as i32,
            (index / plane) as i32,
        );
        for step in NEIGHBOURS {
            let next = cell + step;
            if next.cmplt(IVec3::ZERO).any() || next.cmpge(bricks.as_ivec3()).any() {
                continue;
            }
            let at = next.x as usize + next.y as usize * width + next.z as usize * plane;
            if reached[at] || page[at] >> TAG_SHIFT != TAG_EMPTY {
                continue;
            }
            reached[at] = true;
            queue.push(at);
        }
    }

    for (index, slot) in page.iter_mut().enumerate() {
        if *slot >> TAG_SHIFT == TAG_EMPTY && !reached[index] {
            *slot = (TAG_SOLID << TAG_SHIFT) | 1;
        }
    }
}

fn measure_coarse(near: &[bool], bricks: UVec3, size: f32) -> Vec<f32> {
    let width = bricks.x as usize;
    let plane = width * bricks.y as usize;
    let mut coarse: Vec<f32> = near
        .iter()
        .map(|seeded| match seeded {
            true => 0.0,
            false => f32::MAX,
        })
        .collect();

    let steps: Vec<(IVec3, f32)> = (-1..=1)
        .flat_map(|z| (-1..=1).flat_map(move |y| (-1..=1).map(move |x| IVec3::new(x, y, z))))
        .filter(|step| *step != IVec3::ZERO)
        .map(|step| (step, step.as_vec3().length() * size))
        .collect();

    let sweep = |coarse: &mut Vec<f32>, forwards: bool| {
        let order: Vec<usize> = match forwards {
            true => (0..coarse.len()).collect(),
            false => (0..coarse.len()).rev().collect(),
        };
        for index in order {
            let cell = IVec3::new(
                (index % width) as i32,
                ((index / width) % bricks.y as usize) as i32,
                (index / plane) as i32,
            );
            let mut best = coarse[index];
            for (step, weight) in &steps {
                let next = cell + *step;
                if next.cmplt(IVec3::ZERO).any() || next.cmpge(bricks.as_ivec3()).any() {
                    continue;
                }
                let at = next.x as usize + next.y as usize * width + next.z as usize * plane;
                if coarse[at] < f32::MAX {
                    best = best.min(coarse[at] + weight);
                }
            }
            coarse[index] = best;
        }
    };

    for _ in 0..2 {
        sweep(&mut coarse, true);
        sweep(&mut coarse, false);
    }
    let slack = 3f32.sqrt() * size;
    for room in coarse.iter_mut() {
        if *room == f32::MAX {
            *room = 0.0;
        }
        *room = (*room - slack).max(0.0);
    }
    coarse
}

const NEIGHBOURS: [IVec3; 6] = [
    IVec3::new(1, 0, 0),
    IVec3::new(-1, 0, 0),
    IVec3::new(0, 1, 0),
    IVec3::new(0, -1, 0),
    IVec3::new(0, 0, 1),
    IVec3::new(0, 0, -1),
];

#[allow(clippy::too_many_arguments)]
fn brick_voxels(
    corner: Vec3,
    voxel: f32,
    range: f32,
    bias: f32,
    tie: f32,
    surface: &Surface,
    list: &[u32],
) -> (Vec<u8>, Vec<u8>) {
    let mut voxels = Vec::with_capacity((SPAN * SPAN * SPAN) as usize);
    let mut painted = vec![0u8; (BRICK * BRICK * BRICK) as usize];
    for z in 0..SPAN {
        for y in 0..SPAN {
            for x in 0..SPAN {
                let offset = Vec3::new(x as f32, y as f32, z as f32) - Vec3::splat(APRON as f32);
                let point = corner + offset * voxel;
                let (signed, material) = signed_distance(point, surface, list, tie);
                let unit = ((signed - bias + range) / (2.0 * range)).clamp(0.0, 1.0);
                voxels.push((unit * 255.0).floor() as u8);

                let inner = IVec3::new(x as i32, y as i32, z as i32) - IVec3::splat(APRON as i32);

                if inner.cmpge(IVec3::ZERO).all() && inner.cmplt(IVec3::splat(BRICK as i32)).all() {
                    let at = inner.x as u32 + inner.y as u32 * BRICK + inner.z as u32 * BRICK * BRICK;
                    painted[at as usize] = material;
                }
            }
        }
    }

    (voxels, painted)
}

fn signed_distance(point: Vec3, surface: &Surface, list: &[u32], tie: f32) -> (f32, u8) {
    let mut nearest_signed = f32::MAX;
    let mut material = 0u8;
    let mut at = 0usize;

    while at < list.len() {
        let part = surface.part(list[at]);
        if let Some(solid) = surface.solid(list[at]) {
            let signed = solid.distance(point);
            if signed < nearest_signed {
                nearest_signed = signed;
                material = solid.material;
            }
            while at < list.len() && surface.part(list[at]) == part {
                at += 1;
            }
            continue;
        }
        let mut best = f32::MAX;
        let mut nearest = Vec3::ZERO;
        let mut pseudo = Vec3::ZERO;
        let mut chosen = 0u8;

        while at < list.len() && surface.part(list[at]) == part {
            let index = list[at];
            at += 1;

            let (corner_low, corner_high) = surface.boxes[index as usize];
            let outside = (corner_low - point).max(point - corner_high).max(Vec3::ZERO);
            if outside.length_squared() > best + tie {
                continue;
            }

            let triangle = &surface.triangles[index as usize];
            let contact = closest_on_triangle(point, triangle);
            let reach = point.distance_squared(contact);
            let tied = best < f32::MAX && (contact - nearest).length_squared() <= tie;
            if !tied && reach >= best {
                continue;
            }
            let facing = (triangle[1] - triangle[0])
                .cross(triangle[2] - triangle[0])
                .normalize_or_zero()
                * corner_weight(triangle, contact, tie);
            if tied {
                pseudo += facing;
                best = best.min(reach);
            } else {
                best = reach;
                nearest = contact;
                pseudo = facing;
                chosen = surface.material(index);
            }
        }

        if best == f32::MAX {
            continue;
        }
        let reach = best.max(0.0).sqrt();
        let signed = match (point - nearest).dot(pseudo) < 0.0 {
            true => -reach,
            false => reach,
        };
        if signed < nearest_signed {
            nearest_signed = signed;
            material = chosen;
        }
    }

    (nearest_signed, material)
}

fn corner_weight(triangle: &[Vec3; 3], contact: Vec3, tie: f32) -> f32 {
    for corner in 0..3 {
        if (contact - triangle[corner]).length_squared() > tie {
            continue;
        }
        let first = (triangle[(corner + 1) % 3] - triangle[corner]).normalize_or_zero();
        let second = (triangle[(corner + 2) % 3] - triangle[corner]).normalize_or_zero();
        return first.dot(second).clamp(-1.0, 1.0).acos();
    }
    1.0
}

pub(crate) fn closest_on_triangle(point: Vec3, triangle: &[Vec3; 3]) -> Vec3 {
    let (a, b, c) = (triangle[0], triangle[1], triangle[2]);
    let (along, across) = (b - a, c - a);

    let from_a = point - a;
    let (d1, d2) = (along.dot(from_a), across.dot(from_a));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let from_b = point - b;
    let (d3, d4) = (along.dot(from_b), across.dot(from_b));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let from_c = point - c;
    let (d5, d6) = (along.dot(from_c), across.dot(from_c));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }

    let edge_ab = d1 * d4 - d3 * d2;
    if edge_ab <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + along * (d1 / (d1 - d3));
    }
    let edge_ac = d5 * d2 - d1 * d6;
    if edge_ac <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + across * (d2 / (d2 - d6));
    }
    let edge_bc = d3 * d6 - d5 * d4;
    if edge_bc <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }

    let total = 1.0 / (edge_bc + edge_ac + edge_ab);
    a + along * (edge_ac * total) + across * (edge_ab * total)
}
