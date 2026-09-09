---
id: decision/authoring-in-code
type: decision
title: The world is imported, not written
created: 2026-08-28
updated: 2026-09-06
tags: [authoring, import, baking]
links: [decision/single-brush, reference/engine-field, reference/build-and-run]
---

# Authoring

**2026-08-28.** An importer for SDF Modeler files was finished and then deleted
hours later: the world became `bsn!` scenes in Rust, because a scene file that
describes brushes is a second schema to keep in step with the field.

**2026-09-06. Reversed, and for the opposite reason.** The field no longer has
brushes to describe, so there is no schema to drift. Geometry is a **triangle
mesh, imported and baked**:

```sh
cargo run --release -- --model models/thing.glb
```

glTF only, first mesh of the first primitive, loaded through Bevy's own loader.
No `--model` generates a **1 km open world**: a sine-noise terrain heightfield
closed with a skirt and a floor, 220 scattered structures planted at ground
height, and four torus arches, all merged into **one mesh** before baking. It
exists so the project runs with nothing in `assets/` and so the field is
exercised at open-world scale. **It is a placeholder, not a feature.**

`--size` scales it (1000 m), `--bricks` sets the brick budget and therefore the
voxel size, `--voxel` pins the voxel size directly.

The importer path is `adf::triangles_of` plus `adf::fit`: positions and indices
out of the mesh, degenerate triangles dropped, then scaled so the widest extent
is `--size` (4 m) and centred on the origin. Anything Bevy can load is a model.

## What this costs

- **Everything is baked before the first frame.** The 1 km world is 105k
  triangles and 101k bricks and takes 8.5 s. Nothing streams, nothing is incremental, and a model change is a
  restart.
- **The world must be closed.** The generated terrain carries a skirt and a
  bottom face for exactly this reason; a heightfield alone would bake with no
  inside.
- **Sign needs a closed mesh.** The bake reads inside/outside from an
  angle-weighted pseudonormal at the closest point. An open or self-intersecting
  mesh gets a plausible-looking field with wrong interiors, and nothing warns.
- **There is no CSG.** Two models cannot be unioned, subtracted or blended;
  the field is whatever the triangles say. Combining is a modelling-tool job now,
  which is the point.


## The cost of taking "no predefined shapes" literally

2026-09-08. Flori asked why this engine holds less precision than Turitzin's,
which is editable and still sharp. The answer is the input, not the renderer.

His world is an ordered list of **analytic** primitives; a sphere is
`length(p - c) - r`, evaluated exactly at every grid point. Ours converts
everything to triangles first, so two approximations stack — the mesh's, then the
grid's — where his has only the grid's. And the mesh is currently the *coarser*
of the two by a factor of two to eight.

**That followed directly from the 2026-09-06 requirement**: no predefined shapes
or operations, import an object instead. His engine keeps its precision precisely
*because* it has predefined primitives. The two goals pull against each other,
and this project took the requirement literally.

The reconciliation is that the requirement was really about the **shader**:
nothing hand-written in WGSL, nothing the engine hard-codes about the world. An
analytic primitive evaluated **on the CPU at bake time** breaks neither. Meshes
stay, for actual imports.

**Done the same day.** `sdf/solid.rs` holds five exact shapes — box, sphere,
cylinder, capped frustum, torus — as a Rust enum with a placement. `Surface`
carries them alongside the triangles (`with_solids`), indices past
`triangles.len()` address them, and `signed_distance` evaluates a solid part
exactly instead of walking a triangle list. The generated world's 220 structures
and 4 arches are solids now; only the terrain heightfield is still a mesh.

Two consequences worth knowing:

- **Solids bin by a near-surface shell, not by their box.** A sphere's AABB is
  the whole ball, so binning by the box would allocate every interior brick and
  defeat `buried_bricks`. A brick is listed only when
  `abs(solid.distance(brick centre)) <= size * 0.866 + range + voxel` — sound,
  because a brick whose centre is farther than its own half-diagonal plus the
  band cannot hold a banded point.
- **The sphere's maths now exists twice**, once as Rust in `solid.rs` for the
  bake and once as a WGSL string in `shapes.rs` for the dynamics. That is
  deliberate: `shapes.rs` stores shader source, not evaluable maths, and the
  alternative is a WGSL-to-Rust transpiler.
