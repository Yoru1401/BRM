---
id: reference/clipmap
type: reference
title: Nested levels, and the bound that must not be clamped
created: 2026-09-08
updated: 2026-09-08
tags: [clipmap, lod, field, soundness]
links: [reference/engine-field, history/wrong-turns, debt/open-threads]
---

# The clipmap

`--levels n` (1 to 4, **default 1**, which is byte-identical to having no
clipmap). Level 0 is finest and smallest; each coarser level doubles the box.
All levels are baked once, centred on the geometry's centre, and share one
atlas — bricks are 8³ whatever the level, so only the page table and the voxel
size differ. Page tables and the chamfer arrays are concatenated into one buffer
with a `page_at` offset per level.

Measured on the 1 km world, interleaved A/B/A in one command:

| | voxel | GPU median |
|---|---|---|
| `--levels 1` | 1.099 m | 5.07 / 4.74 ms |
| `--levels 4` | **0.092 m** near, 2.47 m far | 6.15 ms |

**Twelve times finer near the centre for about a quarter more GPU time**, at the
same brick count. The far field is *coarser* than a single level, because a fixed
budget split four ways has to come from somewhere. That is the trade, and it is
only a win when the camera is inside the world: the default fly camera views the
whole 1 km from outside, which is all far field, and there the clipmap looks
worse.

## The probe walks finest to coarsest

Every level's reading is a sound underestimate, so the rule is simple:

- **banded** — the value is real; return it, whatever level found it.
- **solid** — negative; return it.
- **clear** — keep the *largest* seen and carry on to the next level. Larger is
  tighter, and both are underestimates.

## The clamp, and the bug it caused

An empty brick's bound is `inscribed + coarse + range`, and the chamfer that
feeds it only knows about geometry inside **that level's** box. Geometry just
outside could be nearer, so the bound must be clamped to the distance to the
level's own wall before a coarser level is consulted.

**The coarsest level must not be clamped.** Its box is the geometry's bounds plus
margin, so nothing is outside it and the clamp only makes the bound needlessly
small. Applying it there cost three character tests — the ride spring settled at
0.43 m instead of 1.0, because the downward sphere trace could not reach the
ground in its iterations — and it made the renderer *look* fast: 1.46 ms against
the true 4.9, because rays hit the step budget and gave up. **A render that got
faster and darker at the same time was measuring rays that quit.**

## The stopping tolerance follows the level

`ray_march` stops below `max(SURFACE_THRESHOLD, travelled * precision)`. At 500 m
that is about 0.3 m, but the far level's voxel is 2.47 m, and a field cannot
resolve a tolerance finer than its own voxel — those rays burn the budget and
speckle. `baked_probe` therefore returns the sampled level's voxel as a third
component, and the marcher floors the tolerance at half of it.

## Still open

- **It does not follow the player.** Levels are baked once around the geometry's
  centre. Making them track the camera needs incremental re-bake, which the
  bake-once architecture does not have.
- **Thin seams show at level boundaries**, because the normal taps straddle a
  hard switch between resolutions. Blending across a transition band would fix
  it; nothing does yet.
- The level ratio is 3x, not 2x, because each level independently coarsens to fit
  `budget / levels`. Nothing requires 2x, but it is worth knowing.
