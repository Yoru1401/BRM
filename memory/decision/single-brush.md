---
id: decision/single-brush
type: decision
title: One brush, then none
created: 2026-09-03
updated: 2026-09-06
tags: [authoring, field, removal]
links: [decision/authoring-in-code, reference/engine-field, history/wrong-turns]
---

# One brush, then none

**2026-09-03.** The field was reduced to a single brush — a box reshaped by
`Modifiers { round, bevel, thickness, cone }` — plus nine blend modes. A full
round was an exact sphere, a full bevel an exact cylinder. It worked, it was
tested, and it made the culling proof tractable: with one brush the only inexact
term left in the field was the taper, so `cull_scale` was a single closed form.

**2026-09-06. Removed entirely.** Flori: no predefined shapes and no predefined
operations, import an object instead of writing it twice.

## What the brush proved before it went

- **Modifier semantics guessed from a name are usually wrong.** `round` and
  `bevel` meant different things in the two authoring tools this field was
  reverse-engineered from. Thickness took four rounds.
- **Two of the nine blend modes fold the whole field.** `intersect` is
  `max(shape, field)`, and far away the shape is large and positive, so it *is*
  the result. A scene of intersections rendered almost nothing.
- **`op_subtract` and `op_intersect` overestimate near seams**, and
  over-relaxation amplifies it. The parity tests used unions only, deliberately.
- **A geometric bound is not a bound on the evaluator.** The box reject used the
  shape's real AABB, which overshoots what an evaluator that deliberately
  undershoots returns. That is what `cull_scale` was.
- **The reversal worth remembering.** Flori: *"They were correct at the start."*
  The original shell was right and three rounds went into replacing it.

## Why baking answers the question the brush could not

Writing geometry twice — once in Rust for physics, once in WGSL for rendering —
was the standing tax of an analytic field, and every new primitive doubled it.
Three ways out were weighed in
[plan/open-world](../plan/open-world.md#sending-distance-functions-to-the-gpu-without-shape-structs):
runtime WGSL codegen, an expression tape interpreted in the shader, and baking.

**Baking won because it deletes the shader half of the problem rather than
automating it.** The GPU never sees a distance function; it samples a volume.
Authoring becomes unconstrained Rust, or a mesh off disk, and the shader gets
*smaller* — `shapes.wgsl`, `operations.wgsl` and `scene.wgsl` are gone.

The price is in [reference/engine-field](../reference/engine-field.md): the field
is sampled, so it is approximate, and every approximation has to be pushed to the
safe side by hand.
