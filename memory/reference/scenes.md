---
id: reference/scenes
type: reference
title: The gym, the zoo and the museum
created: 2026-09-08
updated: 2026-09-08
tags: [scenes, documentation, workflow]
links: [reference/code-map, reference/build-and-run, decision/authoring-in-code]
---

# Documentation you walk through

`--scene gym | zoo | museum`, built in `sdf/scenes.rs`. Anything else is the
1 km open world, which is what `play` means. The pattern is Ry Storm's: the
engine documents itself by being walkable, so nobody has to trust a wiki page
that went stale two refactors ago.

| scene | answers | default size |
|---|---|---|
| `gym` | what can the character controller actually do | 140 m |
| `zoo` | what does each material and each solid look like | 90 m |
| `museum` | how do the systems behave | 150 m |

**All three are pure solids — zero triangles.** That is why they bake in 2.6 to
4.2 s and get 0.066 to 0.165 m voxels, against 28.4 s and 1.099 m for the open
world. They are also the only regular exercise the solids-only path gets: before
them, `bake_at` returned an empty field for a world with no mesh, and nothing
noticed.

## What each holds

- **Gym** — graduated obstacles on four arms: risers 0.25 to 3.0 m, gaps 1 to
  6 m, ramps 10 to 50 degrees, clearance bars 1.2 to 3.0 m. Each is a separate
  block rather than a staircase, so you walk at them one at a time and **read the
  limit off the last one you clear**. No number is written down, which is the
  point — a constant in the README goes stale, a wall you cannot climb does not.
- **Zoo** — one sphere per palette entry in front, every `Shape` the baker knows
  behind, and a post exactly one character tall for scale.
- **Museum** — three pillars of different heights over a flat plate, so the
  penumbra widening with distance is visible in one glance; the falling bodies
  around a torus, showing dynamics folded into the field; and a row of spheres
  from 4 m down to 0.06 m, where the last two vanish because they are under a
  voxel. That row is the honest picture of the resolution limit.

## Rules

- **The legend lives in the overlay**, from `Scene::legend()`, not in world
  space. There is no 3D text in this engine and adding some for a debug scene
  would be a feature nobody asked for.
- **A scene's layout must fit its slab.** The gym's gap run originally ran to
  -82 m on a slab of half-extent 65 and its last platforms floated in the void.
  Nothing warns; look at the screenshot.
- **Bodies are per-scene.** Gym and zoo default to none, because spheres rolling
  through a measurement are noise.
