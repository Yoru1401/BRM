---
id: project/idk
type: project
title: IDK — an SDF game template in Bevy
created: 2026-08-27
updated: 2026-09-06
tags: [bevy, sdf, raymarching, adf]
links: [decision/authoring-in-code, decision/single-brush, reference/engine-field]
---

# IDK

A **template**, not the game yet. Bevy 0.19.1, Rust edition 2024, WGSL.
Everything in the world is a signed distance field, and the field is **baked**.

Three parts:

1. **Render world** — one ray per pixel in the fragment stage against a sparse
   brick volume: a page table of `u32` per brick and one `R8Unorm` 3D texture
   atlas, reconstructed by hardware trilinear, over-relaxed.
2. **Physics world** — sphere rigidbodies reading the same bricks on the CPU,
   drawn as ordinary Bevy meshes that sort against the marched field by depth.
3. **UI** — a screen-space stats overlay, and nothing else.

Geometry is **imported**: a glTF mesh via `--model`, baked into the volume at
startup. There are no primitives and no CSG operations in the engine, in Rust or
in WGSL. The default torus exists only so the project runs with an empty
`assets/`.

## Order of work

Set by Flori (Q41), and followed in this order:

- **B** rotation and inertia — done
- **C** modifier coverage — done, reduced to one box brush, then **deleted
  entirely on 2026-09-06** ([decision/single-brush](../decision/single-brush.md))
- **A** spatial acceleration — done as a uniform grid, then deleted with the
  brush; the brick page table replaces it
- **D** lighting — done: point / directional / spot, opt-in soft shadows, and
  since baking they cost about 0.02 ms
- **E** start the actual game — **next, and nothing is blocking it**

The large-world work is planned separately in
[plan/open-world](../plan/open-world.md); baking was its step 4, brought forward.
