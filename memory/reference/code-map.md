---
id: reference/code-map
type: reference
title: Layout, and the constants that have a reason
created: 2026-08-27
updated: 2026-09-06
tags: [layout, constants]
links: [reference/engine-field, feedback/code-style]
---

# Code map

Five plugins in three folders. `main.rs` is 56 lines and only wires them.

```
src/sdf/     adf.rs     bake triangles and solids into bricks, sample on the CPU
             solid.rs   exact shapes the bake evaluates: box, sphere, cylinder,
                        capped frustum, torus. Add one here and nowhere else
             scenes.rs  gym / zoo / museum, built from solids, plus their legends
             field.rs   the scene or the model, bake once, spawn the quad
             dynamic.rs moving shapes, folded into the field after the lookup
             shapes.rs  the dynamics' shape table, and the WGSL it generates
             render.rs  material, camera, quad fitting, view toggles
             light.rs   point / directional / spot, packed to the GPU
src/game/    scene.rs   the placeholder scene: three lights, three bodies
             physics.rs sphere rigidbodies against Adf::distance
             input.rs   fly camera and key toggles
             overlay.rs the stats text
src/dev/     benchmark.rs  screenshot.rs  tests/
assets/shaders/  bindings.wgsl  adf.wgsl  marching.wgsl  lighting.wgsl  sdf.wgsl
```

A `bench` run loads `sdf/` and `game/scene.rs`, not physics or the overlay,
which is what makes a frame time attributable to the renderer.

## Constants that are not arbitrary

- **`BRICK = 8`, `APRON = 1`.** 10³ samples per brick. Smaller bricks mean more
  page entries for the same skip; larger ones waste atlas on interior.
- **`MAX_SLOT_SIDE = 204`** is `2048 / SPAN`, the largest 3D texture a backend
  guarantees. The atlas is otherwise sized to the model, so this is a ceiling of
  about 8.5M bricks, not a budget.
- **`PAGE_LIMIT = 8M words`** caps the *dense* page grid at 32 MB, so a brick
  grid may not exceed roughly 200³.
- **`BRICK_BUDGET = 150000`**, `--bricks`. The real quality knob: the bake keeps
  coarsening until the brick count fits. 150k bricks is about 8 s of bake and
  99 MB of atlas.
- **`TARGET_VOXELS = 2048`** is the resolution the bake *asks* for and
  **`COARSEN = 1.5`** how fast it gives up. Starting fine and coarsening means
  the budget decides the voxel size, not the constant.
- **`RANGE_VOXELS = 4.0`** is the encoded distance band, and therefore also the
  width of a penumbra. Narrower buys quantiser precision — measured worth
  nothing — and costs march step length inside allocated bricks and soft-shadow
  reach.
- **`NORMAL_TAP = 1.0`** voxels. Measured: 0.5 and 1.0 tie, 1.5 and wider get
  worse once the taps reach the clamped plateau.
- **`BIAS_VOXELS = 0.866`**, half a voxel diagonal. The worst-case trilinear
  overestimate, and it happens at creases; 0.5 was measured and fails there.
- **`MARGIN = 2`** bricks of empty space around the model, so the flood fill has
  a seed and the outside-the-volume step is safe.
- **The per-brick bin is `RANGE_VOXELS + 1` voxels wide**, not a brick. Widening
  it multiplies the brick count; narrowing it breaks the clearance proof.
- **`TAG_SHIFT = 24`, `TAG_EMPTY = 0xff`, `TAG_SOLID = 0xfe`.** The page word is
  a slot index below `0xfe000000`, otherwise a tag plus a clearance in bricks.
  This is why `SLOT_SIDE` may not exceed 255.
- **16 materials**, in a `paint` volume of 8³ per brick beside the distance
  atlas. Half the memory of the distances, read once per shaded pixel, exact to
  the voxel. The palette belongs to whoever built the geometry, not to `sdf/`.
- **`OMEGA = 1.0`.** Over-relaxation is off: on a field of bounds it eats holes
  in grazing silhouettes. The flag is still there.
- **`DETAIL = 1.0`.** The knob is worth about 3% and moves silhouettes.

## Where a number lives

Every module reads its own flag and keeps the default as a `const` beside it.
A central `settings` resource was planned once and rotted before it was built —
see [history/wrong-turns](../history/wrong-turns.md#process).
