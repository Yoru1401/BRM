---
id: history/decisions-log
type: history
title: Answered questions, in order
created: 2026-08-27
updated: 2026-09-06
tags: [history, branch-points]
links: [feedback/working-style]
---

# Decision log

Flori's flow is a git repo: numbered questions, one at a time, and if something
learned later invalidates an earlier answer, name the question so it can be
reopened and branched from there. **This file exists to make that reopening
possible** — it is the list of branch points, not a history of the code.

Decisions whose reasoning still matters have their own file and are not repeated
here.

## Setup and renderer

- 3D, Bevy 0.19, empty project first; mouse look, then a benchmark scene
- Overlay shows **both** average fps and frametime
- Build order: renderer, then physics, then UI
- **Baseline first, then the optimisation**, so a gain is measurable rather than
  assumed. Standing rule, and it is what killed cone marching later
- Scene as a storage buffer of shapes; runtime toggles rather than rebuilds

## Authoring

- MagicaCSG first, then SDF Modeler — chosen because it ships its shaders, so
  primitives and blends were **copied, not inferred**. Both are out of the
  pipeline now; the field is its own. The only thing that survived either is
  modifier semantics, and that record went with the brush on 2026-09-06 — see
  [decision/single-brush](../decision/single-brush.md)
- **Authoring moved into `bsn!`** hours after the importer was finished — see
  [decision/authoring-in-code](../decision/authoring-in-code.md)
- Reduced to one box brush on 2026-09-03 —
  [decision/single-brush](../decision/single-brush.md)
- **Brushes and blend modes removed entirely on 2026-09-06.** Flori: no
  predefined shapes, no predefined operations, import an object rather than
  writing it in Rust and WGSL both. Geometry is a glTF mesh baked into a sparse
  brick volume — [decision/authoring-in-code](../decision/authoring-in-code.md).
  Three ways out were weighed: runtime WGSL codegen, an expression tape
  interpreted in the shader, and baking. **Baking won because it deletes the
  shader half of the problem rather than automating it.**
- **The uniform grid, the skip pyramid, the hierarchical coarse pass and the
  shadow proxy went with them**, all on 2026-09-06. The brick page table does
  what the first three did, and the fourth existed only because a second
  *analytic* evaluator cost the whole frame.
- **Dynamics left the field.** Physics bodies are ordinary Bevy meshes that sort
  against the marched geometry by depth. Reopen this if dynamic geometry ever
  has to blend with the world rather than sit in it.

## Physics

- Spheres against the static field, rotation and inertia, Coulomb friction,
  sleeping
- **Softbodies removed after being built** —
  [decision/physics-scope](../decision/physics-scope.md). Ask before rebuilding

## UI

- In-world panels tried, then **removed 2026-09-01**: the world is the field, and
  only the field. The screen-space stats overlay stays

## Process

- Talk caveman, ULTRA; memory in Open Knowledge Format
- Self-documenting code, ponytail for everything built, **no comments at all**
- "Make sure everytime that it works" — after two compile errors shipped at once
- **Every question ends with a recommendation and its reason**, not a neutral
  menu (added 2026-08-31)

## 2026-08-31, numbered

| Q | decision |
|---|---|
| Q82 | plugin split before anything else |
| Q83 | `input` actions before `settings` |
| Q85 | user pushes to GitHub themselves; no token here |
| Q86-87 | box reject with a **box**, not a sphere |
| Q88-89 | build `--no-cull` and `spread:` before judging the reject |
| Q91 | over-relaxation, fixed omega with a sweep flag |
| Q93 | `--repeat` before believing a 6% result |
| Q95 | grid resolution fixed at 16 with a `--grid` sweep, not derived |
| Q97 | **delete cone marching** — 4% against a whole vertex stage |
| Q99-101 | lights: point + directional + spot, soft shadows opt-in per light |

Q41's order of work (B rotation, C modifiers, A acceleration, D lighting, E the
game) is done through D. Q78, on the culling box following falling bodies, is
answered: the grid made it moot and `KILL_BELOW` removes the symptom.
