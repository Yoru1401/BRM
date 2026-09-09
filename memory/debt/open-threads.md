---
id: debt/open-threads
type: debt
title: Unverified, deferred, known-fragile
created: 2026-08-27
updated: 2026-09-06
tags: [debt, open]
links: [reference/engine-field, benchmark/2026-09-06-adf, decision/authoring-in-code]
---

# Open threads

Nothing is blocking. Resolved threads are deleted rather than struck through —
what they taught lives in [history/wrong-turns](../history/wrong-turns.md).

- **The BVH is built but not used.** `bvh.rs` produces the tree and the debug
  view reads it; the bake still scatters. The gather attempt was reverted on
  2026-09-08. Retry it as **one** change, with the mesh overlap tests as the
  gate - they are what caught it, and the solid tests did not.
- **Intersect is implemented and untested**, and it is binned into every brick
  because a list cannot say "culled". Fine for one or two; nothing warns above
  that.
- **Sign-change allocation is now half-built.** The per-brick fold cull is its
  conservative form. Doing it properly — allocate only where the folded field
  changes sign — is the next single change, with the truth probe after it.
- **The bake takes 28.4 s on the 1 km world** and nobody knows whether that is a
  regression, because the 8.5 s on record predates several bake changes. The
  render is now cleared: 4.0 ms GPU, matching the pre-solids figure.
- **The frame column is still vsync-capped** from the second `--repeat` block on,
  and that idling also inflates the GPU column by ~12%. Rendering offscreen with
  no present would remove both; reading the first run is the cheap workaround.
- **Materials still melt, and analytic primitives did not touch that.** The
  0.866-voxel bias inflates every surface, so anything closer than about 1.7
  voxels fuses and the fused blob takes its material from whichever source is
  nearest. Fixing it means smaller voxels near surfaces, not better input.
- **`warn_about_thin_parts` only walks triangles**, so a solid thinner than four
  voxels passes without a warning.
- **No BVH over the geometry.** The bake bins triangles into per-brick lists and
  then linearly scans each list per voxel. A BVH would replace both, and is the
  prerequisite for regenerating only the bricks a change touches. Biggest
  available win on bake time, and it touches no rendering code.

## Tried on 2026-09-08 and reverted, worth retrying one at a time

Mike Turitzin's engine video (same architecture: brick map, brick atlas, 8³
bricks) points at three changes. All four were attempted together and reverted;
each is still worth trying **alone**, with the truth probe run after each:

- **Allocate on exact triangle/box overlap** rather than proximity. Measured: at
  the same brick budget the bake landed on 0.146 m voxels instead of 0.220, which
  is the resolution win everything else depends on. Needs the chamfer seeded from
  *near* bricks, not allocated ones, or the empty-space bound goes unsound.
- **Narrow the band** toward half a voxel diagonal. Only viable together with a
  hit criterion that does not rely on the stored magnitude.
- **Drop the stored bias and detect hits by sign change.** Failed the soundness
  tests as built; the trilinear overestimate at creases is the reason the bias
  exists.

## The one that matters

- **Resolution is spent uniformly, and that is now the binding constraint on
  every visual complaint.** The field is provably sound — `how_far_the_bake_strays_from_the_truth`
  reports zero overestimate — and the two remaining errors, a 0.87 voxel surface
  inflation and voxel-resolution reconstruction at creases, **scale only with the
  voxel**. A 200 m world at 100k bricks is 0.22 m voxels *everywhere*, including
  190 m away where a pixel covers metres. Spending bricks near the camera instead
  is the clipmap in [plan/open-world](../plan/open-world.md), and it is the only
  structural answer left.
- **Measured and rejected on the way there:** a per-brick min/max window in place
  of the `range` clamp (unsound, see the wrong turns); `RANGE_VOXELS` 8 (+36%
  bricks, 2x bake, frame time no better); a 0.5 voxel bias (overestimates at
  creases).

## The visible one

- **Normals carry about 2.9° of mean error.** It is measurable but **not** what
  the speckling on 2026-09-06 was — that was the shadow penumbra reading brick
  bounds, and the geometry is clean at `--shadow-steps 0`. Whether 2.9° is
  visible at all on its own has not been established. Measured against an
  analytic torus by `how_much_of_the_normal_error_is_the_mesh` (ignored
  diagnostic, `cargo test --release ... -- --ignored --nocapture`). Only about
  0.6° of it is a coarse mesh's own faceting: 24 rings reads 3.51°, 48 reads
  2.90, 96 reads 2.99 and stops improving. The rest is the trilinear
  reconstruction's discontinuous gradient plus 8-bit quantisation.

  **What was already tried and did not help**: widening `NORMAL_TAP` (0.5 and
  1.0 tie; 1.5 and up get worse, the taps reach the clamped plateau), halving
  `RANGE_VOXELS` from 4 to 2 with rounding instead of truncation (no measurable
  change in the angle), scaling the shadow bias with the voxel (it was not
  shadow acne). **The untried lever is atlas precision** — an `R16Float` atlas
  is 256x the levels for 2x the memory, and `r16float` is core-filterable.
  Nothing is a texture-format hazard here; it is just work.

- **Soft shadows are hard shadows now.** The penumbra may only use samples from
  the encoded band ([reference/lights](../reference/lights.md)), which is four
  voxels wide, so `softness` does nothing at map scale. Widening the band widens
  the penumbra and shortens march steps inside allocated bricks. Untried.

- **Nothing under about four voxels survives the bake.** The bake warns, and the
  generated world no longer authors anything that small, but an imported model
  with fine detail will shred silently apart from that warning.
- **Creases staircase at voxel resolution.** Where two surfaces meet — a cone on
  the ground, the terrain's own lip — the intersection line is reconstructed at
  the voxel and reads as a scalloped or sawtooth edge. Proven to be resolution
  by rendering the same view at 0.165 m and 0.110 m voxels. `--bricks` buys it
  back; nothing in the logic is wrong.
- **A glTF's own materials are ignored.** `Adf::palette` is filled by whoever
  built the geometry, and the glTF path fills nothing, so an imported model is
  entirely material 0. Reading base colour and roughness out of the file is the
  obvious next step.
- **`RANGE_VOXELS = 8` is unresolved.** It made the step heatmap visibly cooler
  and cost 22% more bricks and 76% more bake. Reverted to 4 on bake cost alone,
  because the frame side could not be read.

## Fragile

- **`shot` at 1920x1080 exits 139.** Segfault during shutdown, *after* the file
  is written correctly — the image decodes and diffs clean. 1600x900 and below
  exit 0. **Screenshot at 1600x900 or below.**
- **Sign needs a closed mesh.** The bake reads inside/outside from an
  angle-weighted pseudonormal at the closest point. An open, self-intersecting or
  inverted mesh produces a plausible-looking field with wrong interiors, and
  nothing warns. No non-manifold model has ever been baked here.
- **Nine constants are declared twice**, in `sdf/adf.rs` and `bindings.wgsl`, and
  nothing checks they agree. See
  [reference/engine-field](../reference/engine-field.md#two-evaluators-still).

- **The page grid is dense.** One `u32` per brick over the whole volume, so
  memory grows with world *volume* rather than with surface. A 4 km world at 1 m
  voxels wants 125M words. `PAGE_LIMIT` refuses it rather than thrashing; the
  clipmap is the fix.
- **The bake is the wall, not the GPU.** About 12 s per 100k bricks, single
  shot, before the first frame. There is no BVH — cost is linear in the
  triangles binned into each brick, and the only pruning is a per-triangle AABB
  reject.

## Unverified

- **The retry loop is exercised now**: the 1 km world takes three coarsening
  steps to land inside the brick budget. Untested is a model whose *brick grid*
  exceeds `PAGE_LIMIT` at every resolution the budget allows.
- **No file has ever been baked.** The built-in torus and the built-in 20 m map
  both come from Bevy's primitive meshers. `--model` loads glTF through Bevy's
  own loader and has not been run against a real file, so the mesh-extraction
  path (`RenderAssetUsages` retention, index formats, multi-primitive meshes) is
  unproven.
- **The generated world is not manifold.** Its parts interpenetrate — every box,
  cone and cylinder sinks into the terrain — so its interior sign is formally wrong
  wherever two hulls overlap. It renders correctly because rays hit the outer
  shell first, but it is not evidence that the sign logic handles overlap.
- **Rotation** of imported geometry is not a thing any more — `fit` only centres
  and scales. Anything else is the modelling tool's job.

- **Dynamics render but do not collide.** They are folded into the *shader's*
  field, so they are marched, lit and shadowed; `Adf::distance` on the CPU is the
  baked field only, so bodies still meet each other through
  `sphere_pair_correction` and the character passes through them.
- **Body size is capped by bake resolution**, `min(reach * 0.012, range * 0.9)`.
  A 1 km world at 1.1 m voxels cannot carry metre-scale physics.
- **The acceleration curves are straight lines.** The reference uses Unity
  `AnimationCurve`s whose keyframes are not in the published project; ours
  interpolates `TURN_FACTOR` to `ALIGNED_FACTOR`. Shape and intent match, the
  feel may not.
- **Squash-and-stretch, platform-relative motion and the dust particles** from
  the reference controller are not built.

## Deferred on purpose

- **Colour.** `ALBEDO` is one constant in `sdf.wgsl`. The atlas stores distance
  and nothing else; per-voxel colour would be a second 3D texture.
- **Nothing streams.** The whole model is baked before the first frame, 2.4 s for
  9216 triangles, and a model change is a restart. The clipmap in
  [plan/open-world](../plan/open-world.md) is what makes this incremental.
- **No CSG.** Two models cannot be combined; the field is whatever the triangles
  say.
- **Ambient occlusion, sky gradient** — considered with lighting, never built.
- **The bake is O(triangles in the brick) per voxel** with no BVH, so the cost is
  linear in model complexity. Fine to about 40k triangles.
