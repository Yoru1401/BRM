---
id: reference/lights
type: reference
title: Lights, and the proxy that baking made unnecessary
created: 2026-08-31
updated: 2026-09-06
tags: [renderer, lighting, wgsl]
links: [reference/engine-field, benchmark/2026-09-06-adf, history/wrong-turns]
---

# Lights

A light is an entity — `Light` + `Transform` — packed into a fixed-size storage
buffer the shader walks per shaded pixel.

- **-Z is forward**, the convention Bevy's own lights and cameras use, so
  `Transform::looking_at` aims a spot the way you expect.
- **Range is a cull, not a dim.** The falloff reaches zero *at* `range`, so an
  out-of-range light costs a length and a compare.
- **Spot angles are packed as cosines**, so the cone test is a dot product with
  no trig per pixel. Cosine runs backwards: `cos_inner > cos_outer`, and getting
  it wrong inverts the cone silently. `to_gpu` forces the ordering.
- **Shadows are opt-in per light**, default off.
- A shadow ray that runs out of steps reads as **lit**, the forgiving direction.

Soft shadows are Inigo Quilez's penumbra trick: march toward the light, track the
smallest ratio of field distance to travelled distance. A ray squeezing past an
edge returns a partial value, so the penumbra costs nothing beyond the march.
`softness` scales the ratio — higher is sharper.

**Only a real distance may darken.** The baked field returns a *bound* wherever
it has no encoded value — an empty brick, a clamped texel, a point outside the
volume — and a bound undershoots. Fed to the penumbra it shadows pixels that
should be lit, and because bounds are piecewise constant per brick it does so in
**brick-shaped patches and speckle across every shadowed surface**. That was the
whole of the shadow bug on 2026-09-06. `adf_probe` reports whether a sample came
from the encoded band; `shadow_factor` skips the penumbra update when it did not.

The consequence: **the penumbra is only as wide as `RANGE_VOXELS`**, four voxels.
Beyond that shadows are hard by construction and `softness` does nothing. Wider
soft shadows need a wider encoded band, which costs march step length inside
allocated bricks. Nobody has asked for them yet.

## The shadow bias has to clear the field's own thickening

`shadow_bias()` is `max(SHADOW_BIAS, voxel * 2)`. The baked field's surface sits
half a voxel diagonal *outside* the true one by construction
([reference/engine-field](engine-field.md)), so a fixed 0.02 m bias is not
guaranteed to start the shadow ray outside the shell once voxels are coarse.
Scaling it with the voxel is the only part of the lighting code that knows the
field is baked.

Measured, it changed almost nothing on the 15.6 mm torus: the speckling on the
terminator is normal error, not shadow acne. Kept because it is correct, not
because it fixed a symptom.

## The 14 ms that was not marching, and why it is gone

**2026-09-04, analytic field.** Adding one light to `spread:80` cost **14 ms**,
and the cause was not the marching, the light count or the loop: `shape_distance`
appeared a **second time** in the shader, inside the shadow march. Registers are
allocated for it whether or not a caster exists, occupancy halves, and the whole
frame slows — the main camera march included.

Baseline `--lights 0` was 9.8. Same session, `--repeat 4`, run 3 or later:

| what the shadow march evaluates | `--shadows 0` | `--shadows 1` |
|---|---|---|
| the field, grid walk + `shape_distance` | 23.8 | 24.4 |
| grid walk + `cull_box_distance` only | 10.1 | 10.1 |
| nothing — `shadow_factor` stubbed `return 1.0` | - | 11.0 |

- **Trip count is irrelevant.** `--shadow-steps 12` read the same as 48.
- **The cost is presence, not execution.** `--shadows 0` never called
  `shadow_factor` and still paid 13.9 of the 14.

**The general lesson, and the reason this file still exists: on this GPU a second
evaluator in a shader costs the whole frame, whether or not it runs.**

The answer at the time was a *shadow proxy* — a cheaper, deliberately wrong
occluder that ignored modifiers, so a heavy taper cast the shadow of a plain box
and a subtracted hole let no light through.

**2026-09-06: the proxy is deleted.** A texture fetch is not a second evaluator
in the sense above, so `shadow_factor` now marches the real field. Measured on
the baked torus: three lights with one shadow caster cost **0.02 ms** more than
one light with none, and the 20 m map reads the same 2.62 ms as the torus.
Shadows are correct through holes and cavities for the first time, and they are
effectively free.

## `--shadow-steps 0` is the A/B that isolates them

A shadow ray takes no steps, `shadow_factor` returns 1.0, and every other cost
in the frame is unchanged. Two screenshots at 0 and 48 separate "the field is
wrong" from "the shadow march is wrong" in one run. On 2026-09-06 it did exactly
that: the geometry that looked speckled was pristine at 0 steps, so nothing about
the bake, the normals or the quantiser was ever at fault.
