---
id: plan/open-world
type: plan
title: What an open world changes, and the order to do it in
created: 2026-09-06
updated: 2026-09-06
tags: [architecture, open-world, baking, static-dynamic]
links: [reference/engine-field, reference/lights, benchmark/2026-09-06-adf, decision/single-brush]
---

# Open world

**Status, end of 2026-09-06: steps 1 and 4 are done, 2 and 3 were deleted rather
than finished, and 5 is done for physics and shadows. A 1 km world with 101k
bricks bakes in 8.5 s and renders in 3.40 ms.** What is left of this
plan is the *streaming* half — clipmap rings, incremental baking, and a world
larger than one volume. The rest is history and reasoning worth keeping.

Agreed with Flori on 2026-09-06. The demo becomes a large open world, statics
and dynamics are separated, and **the static field, its collision and its
shadows are baked**; only dynamics rebuild per frame.

Everything measured before this date describes 80 shapes inside one AABB. Read
those numbers as the *demo's*, not the engine's.

## What breaks first, and in what order

| limit | where | what happens |
|---|---|---|
| `MAX_SHAPES = 256` | `sdf/brush.rs` | brushes past 256 are dropped with a `warn!` |
| one AABB over everything | `sdf/bounds.rs` | one stray shape encloses the world |
| grid resolution derives from that AABB | `sdf/grid.rs` | 32³ over 1 km is 31 m cells; the grid stops accelerating |
| `GRID_MAX_ENTRIES = 1 << 18` | `sdf/grid.rs` | overflow flips every cell to `GRID_CELL_FULL` and falls back to a linear scan |
| re-packing every static per frame | `sdf/field.rs` | **fixed 2026-09-06** — change detection now settles a still scene |

## Why baking flips the ADF answer

On 2026-09-05 the answer to "would an ADF help" was **no**: the field is
analytic, and an ADF reconstructs by interpolating sampled distances, which is
not an underestimate. That is the hazard that killed cone marching twice
([decision/cone-marching](../decision/cone-marching.md)).

Baking removes the objection, because a baked field is *sampled rather than
evaluated* and costs the same per sample however many brushes authored it. That
is exactly the condition an ADF needs.

**But not a classic ADF octree.** Octree traversal is pointer-chasing and
divergent; GPUs sample 3D textures in hardware. The shape is a **sparse brick
grid, clipmapped around the camera**:

- statics baked into 8³ voxel bricks with a one-voxel apron (10³ at `R16Float`
  is 2 KB), allocated only near surfaces
- bricks indexed through a coarse page table — the same flat two-words-per-cell
  layout `SdfGrid` already uploads
- hardware trilinear does the reconstruction; one fetch replaces the shape loop
- concentric rings, fine near the camera and coarser outward, streamed by ring

Bevy binds this with `#[texture(N, dimension = "3d")]` on `AsBindGroup`, which
this crate already proves out with the 2D `coarse` texture in `sdf/render.rs`.

### The rule that must not be got wrong

Trilinear interpolation between exact samples **overestimates** distance in the
worst case, and an overestimate is the one error sphere tracing cannot survive.
Store `d - (voxel diagonal / 2)` in every texel; the interpolant is then a
guaranteed underestimate. The price is that surfaces thicken by that amount, so
the march stops on a shell and needs a short exact refinement, or finer bricks
near surfaces - which is the adaptive part.

Budget: at 0.25 m voxels a brick covers 2 m, so 1 GB is ~500k bricks, about
2 km² of surface. **Measure the authored surface area before choosing a voxel
size** - that figure moves by an order of magnitude with how much interior is
baked.

### Three things baking fixes for free

1. **Shadows through holes.** The proxy exists only because a second *analytic*
   evaluator costs the whole frame on this GPU - `--shadows 0` paid 13.9 ms of a
   14 ms cost for code it never ran ([reference/lights](../reference/lights.md)).
   A texture fetch is not a second evaluator in that sense, and a baked field
   knows about subtracted cavities, tapers and bores.
2. **Physics.** `scene_distance` walks the analytic field per body per substep;
   a baked sample is O(1) over the same data, so the two-evaluators contract
   survives.
3. **The per-frame upload.** Statics leave the per-frame buffer entirely -
   0.8 ms of CPU and 1.25 MB of bus, measured in
   [benchmark/2026-09-06-adf](../benchmark/2026-09-06-adf.md).

### The seam rule

Dynamics combine with the baked static field by `min` and nothing else.
Subtract, intersect and shell all need the field *before* the fold, and the bake
has already collapsed it. **Write this down before someone authors a door that
carves a wall.**

## Sending distance functions to the GPU without shape structs

Flori's third question. Three real answers:

- **Runtime WGSL codegen.** Generate shader source from Rust and compile a
  specialised pipeline. Fully general, no interpretation cost, the compiler
  folds the scene into straight-line code. Costs pipeline compilation stalls and
  one pipeline per distinct expression; no dynamic authoring without a recompile.
- **An expression tape interpreted in the shader.** Upload postfix opcodes and
  immediates, run a `switch` in the inner loop - no structs at all, the literal
  answer to the question, and what libfive and Fidget do. The payoff comes from
  interval-arithmetic tape specialisation per region, a whole subsystem, and the
  inner-loop `switch` is exactly the divergence and register pressure this GPU
  has been measured punishing. **Wrong GPU for it.**
- **Bake, and the GPU never sees a distance function.** Evaluate arbitrary Rust
  into the brick volume on the CPU. Authoring becomes unconstrained and the
  shader gets *simpler*.

**Chosen: bake.** The single brush stays for dynamics only, where N is small,
where physics wants exactness, and where it already works.

## Order of work, and what happened to it

1. ~~Split the static and dynamic queries so a still scene stops re-packing.~~
   **Done 2026-09-06, then made moot** — there is no per-frame packing left.
2. ~~Take dynamic bodies out of the grid and fold them after the grid walk.~~
   **Deleted unfinished.** Dynamics are ordinary Bevy meshes now and touch the
   field only to read it, so there is nothing to fold. The prediction in the
   original note was right: the grid-indexes-a-prefix machinery did not survive
   baking.
3. ~~`GridWindow`, a camera-following grid window.~~ **Deleted unfinished**, for
   the same reason. It was 9% faster when the window covered the scene and 4x
   slower when it did not, and the cliff it was built to remove is exactly what
   a clipmap removes properly.
4. **Bake statics into sparse bricks — done.** 8³ voxels plus an apron, a flat
   page table, an `R8Unorm` atlas, hardware trilinear.
   **Clipmapping and streaming are not done**: one volume, baked whole, before
   the first frame.
5. **Point physics and the shadow march at the baked volume — done.** Shadows
   read the real field for the first time, holes included, and cost about
   0.02 ms per extra light.

### What is actually left

- Rings of decreasing resolution around the camera, and a bake that runs
  incrementally rather than all at once at startup.
- **A page table that is not dense.** One `u32` per brick over the whole volume
  means memory grows with world *volume*: 1 km at 1 m voxels is 1 MB, 4 km is
  125 MB, and `PAGE_LIMIT` refuses it. This is the first structural wall, and
  the clipmap is what removes it.
- **An incremental bake.** 12 s per 100k bricks, all before the first frame, is
  the practical cap on world size today — not the GPU, not memory.
- Dynamic geometry that is *field* rather than mesh — still nothing needs it.


## What a working engine of this shape does differently

Mike Turitzin's SDF engine (2026-09-07 video) is the same architecture: an
ordered list of edits, distances cached on a grid, sparse **8³ bricks** in a
texture atlas behind a dense pointer grid. Four differences, in the order they
would pay here:

- **A BVH over the primitives, shared CPU and GPU.** It answers three questions
  we currently answer by brute force: which primitives affect a point, which
  affect a ray, and **which bricks a changed edit invalidates**. Ours bins
  triangles into per-brick lists and then linearly scans each list per voxel,
  which is why the bake is the wall — 10 to 50 s. This is the largest available
  win that touches nothing about rendering soundness.
- **Bricks cached only where the distance changes sign**, not merely near a
  primitive. Attempted 2026-09-08; see
  [debt/open-threads](../debt/open-threads.md). The measured prize is real —
  0.146 m voxels against 0.220 at the same budget — and the correct criterion is
  an exact primitive/box overlap, not a sign test on the samples.
- **Clipmap levels**, each double the previous, centred on the player. His
  figure: 2.5 km of draw distance is 200 trillion cells dense, 20 million
  clipmapped.
- **Physics is a mesh, not the field.** Marching cubes at low resolution on CPU
  threads, fed to Jolt. That deletes the whole bound-versus-distance class of
  bugs this project has fought — clearance gating, radius caps, swept motion.

Two things he confirms rather than teaches: terrain is a fully 3D SDF from noise
octaves rather than a heightfield, which removes the skirt-and-floor hack that
makes our world non-manifold; and interpolated distances *"wobble near sharp
corners"*, which is the artefact this project spent two sessions proving is
resolution-bound.

**The engine is built for a requirement we do not have.** Every one of his
choices serves editing geometry at runtime: the edit list, the BVH, incremental
brick regeneration, the mesh-based physics. Ours bakes once at startup. Copying
the BVH and the allocation rule is worth it on their own merits; copying the edit
pipeline only makes sense if runtime editing becomes a goal.
