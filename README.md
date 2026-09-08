# IDK

A Bevy game template built entirely on signed distance fields. Not a game yet —
the render world, the physics world and the UI, sharing one field.

Bevy 0.19.1, Rust edition 2024.

## What it does

**The field is baked, and no shape is written by hand in WGSL.** You hand it
exact solids, a triangle mesh, or both; it voxelises them into a sparse brick
volume at startup, and everything after that reads the volume.

Solids live in `src/sdf/solid.rs` - box, sphere, cylinder, capped frustum,
torus - and are evaluated **on the CPU at bake time**, never in a shader. That
is a precision decision: a mesh sphere bakes as the icosphere it is, and at the
tessellation the generated world used, its facets were two to eight times
coarser than the voxels sampling them. Meshes remain, for imports.

- **Bake** - `src/sdf/adf.rs`. Triangles and solids are binned per brick, each brick holds
  8³ voxels plus a one-voxel apron, and every voxel stores the signed distance to
  the surface, biased down by half a voxel diagonal so the trilinear
  reconstruction can never overestimate. A mesh's sign comes from an
  angle-weighted pseudonormal at the closest point; a solid's is exact. Solids
  bin by a near-surface shell rather than by their box, because a sphere's box
  is the whole ball. Bricks with no geometry near them are not allocated; the ones
  inside the model are marked solid by a flood fill from outside.
- **Store** — a `u32` page table, one entry per brick, plus one `R8Unorm` 3D
  texture atlas. Both are sized to the model at bake time and uploaded once; a
  1 km world is 1 MB of page and 86 MB of atlas. A page word is either an atlas
  slot or a tag; beside it, one `f32` per brick holds a **chamfer distance
  transform** over the brick grid - the distance to the nearest brick that holds
  any geometry.
- **Render** — ray marching on a single frustum-fitted quad, one ray per pixel.
  A step is one page read and one hardware-trilinear fetch. Empty bricks return
  `inscribed + coarse + range`, a proven underestimate, and that is the whole of
  the empty-space acceleration.
  Over-relaxation is available (`--omega`) but off by default: on a field of
  bounds it eats holes in grazing silhouettes. The fragment stage writes real
  depth, so ordinary Bevy 3D entities share the world and occlude correctly.
- **Dynamics** — anything that moves is a `Dynamic { start, end, radius }`
  capsule, uploaded per frame and folded into the field by `min` after the brick
  lookup. Physics bodies and the character are therefore ray-marched, lit and
  shadowed like the world, with no mesh of their own. `min` is the only operator
  a baked field admits: subtract or intersect would need the field before the
  bake collapsed it.
- **Physics** — sphere rigidbodies against `Adf::distance`, the CPU mirror of the
  same bytes, with rotation, friction, a Coulomb limit and sleep. Motion is
  swept: the frame's travel is sphere-traced through the field, so a centre can
  never cross a surface however fast it is going. Penetration is resolved only
  against a *banded* sample, where the field is a real distance rather than the
  conservative bound it returns elsewhere — without that, bodies hover metres up
  on a bound that crossed their radius early.
- **Character** — a floating-capsule controller behind `--play`, ported from
  [Joe Binns's stylised controller](https://joebinns.com/stylised-character-controller)
  and the Toyful Games approach behind it. No ground collider: a spring holds it
  at a ride height above a downward sphere trace, an angular spring keeps it
  upright and leans it into acceleration, movement is a clamped acceleration
  toward a ramped goal velocity, and the jump has an input buffer, coyote time
  and variable gravity so holding the button jumps higher.
- **Materials** — 16 palette entries of albedo and gloss, sampled **per voxel**
  from a second volume beside the distance atlas (8³ per brick, half the memory,
  read once per shaded pixel so the march costs nothing extra). A dynamic carries
  its own index. `gloss` drives a Blinn-Phong lobe, so two materials of the same
  colour still read differently. The palette belongs to whoever built the
  geometry — no material is named in the engine or in any shader.
- **Lights** — point, directional and spot, with opt-in shadows that march the
  real field. Three lights with a shadow caster cost 0.02 ms more than one light
  with none. Only samples from the encoded band may drive the penumbra, so
  shadows are hard beyond four voxels: a bound undershoots, and shading one
  paints brick-shaped patches across every shadowed surface.

The **dynamics'** shapes are generated too: their maths lives in a table in
`src/sdf/shapes.rs`, which emits the distance and exact-normal functions plus a
`switch` over them and registers the result as a shader module at startup.
Adding one is a single table entry. Bake-time solids and shader dynamics are
separate tables on purpose - `shapes.rs` stores shader *source*, which the CPU
cannot evaluate.

The hand-written shader is five files — `sdf.wgsl` holds only the entry points
and imports `bindings`, `adf`, `marching` and `lighting`.

The source carries no comments. What a name cannot say lives in `memory/`.

## Run

```sh
cargo run --release
cargo run --release -- --play
cargo run --release -- --scene gym --play
cargo run --release -- --scene zoo
cargo run --release -- --model models/thing.glb
```

`--play` spawns the character, switches to a collision-aware follow camera and
defaults the world to 200 m so the voxels are character-scale. `WASD` moves
relative to the camera, `Space` jumps, right-drag orbits.

### Scenes

`--scene` picks what gets baked. Three of them are documentation you walk
through, after the
[gym / zoo / museum](https://rystorm.com/blog/gyms-zoos-museums-your-documentation-should-be-in-game)
pattern. Each is built entirely from exact solids - zero triangles - and prints
its own legend in the overlay.

| scene | shows | try |
|---|---|---|
| `gym` | what the controller can do: risers 0.25-3.0 m, gaps 1-6 m, ramps 10-50 degrees, clearance bars 1.2-3.0 m | `--scene gym --play` |
| `zoo` | every material on an identical sphere, every solid the baker knows, and a post one character tall for scale | `--scene zoo` |
| `museum` | penumbra widening with distance, bodies folded into the field, spheres shrinking past the voxel | `--scene museum` |

Nothing is labelled in world space: the legend is the overlay, and the obstacles
are ordered so you read the limit off the last one you clear.

Without `--scene` or `--model` it generates the **1 km open world** - sine-noise
terrain closed with a skirt and a floor as a mesh, plus 220 structures and four
arches as exact solids.

```
adf: 75266 triangles, 90689 bricks of 150000, 1.099 m voxels,
     1037 m across, 86 MB atlas, 44 MB paint, 1 MB page, baked in 28.4 s
```

The documentation scenes bake in 2.6-4.2 s and get far finer voxels for it:
0.066 m in the zoo, 0.154 m in the gym, 0.165 m in the museum.

**4.0 ms of GPU time at 720p** on the 1 km open world with four lights, a
shadowed sun, dynamic shapes and per-voxel materials; 2.3 ms in the museum. Read
the `gpu_*` columns, never the `frame_*` ones - the frame delta is what the
compositor allowed, and it settles onto the 60 Hz refresh from the second repeat
onward. Because a vsynced repeat leaves the GPU idle 12 ms out of every 16, its
clocks drop and its `gpu_median` reads about 12% high: **the first run of a
`--repeat` is the honest one.**

Debug builds are misleading for a different reason: `debug-assertions` are
profile-wide and put a ~2 ms floor under every frame.

| key | does |
|---|---|
| `WASD` / `Space` / `LShift`, right-drag | fly camera |
| `V` | hide the quad — the frame floor underneath |
| `H` | cycle shaded / march-step heatmap / brick grid |

## Benchmark

```sh
cargo run --release -- bench --repeat 3
cargo run --release -- bench --repeat 3 --lights 3 --shadows 1
```

Prints one tab-separated line per run and exits. Two sets of columns:

- **`gpu_*`** - GPU timestamps around `main_opaque_pass_3d`, from Bevy's
  `RenderDiagnosticsPlugin`. This is the render, and it is what a rendering
  change moves.
- **`frame_*`** - wall-clock frame delta. Includes the wait for present, so it
  reads the refresh rate whenever the compositor holds vsync. Useful only to see
  *that* it happened.

`--repeat 3` runs three blocks. **Read the first**: later blocks are usually
vsynced, and a GPU that idles most of each frame downclocks and reads high. If
the adapter has no timestamp query the `gpu_*` columns are `NaN` and the bench
says so.

## Flags

Every knob takes a flag, on any run — ordinary, `bench` or `shot`. The module
that owns a value reads its own flag; the default stays a `const` beside it.

| flag | default | what |
|---|---|---|
| `--scene <name>` | `play` | `gym`, `zoo`, `museum`, or the open world |
| `--levels <n>` | 1 | clipmap levels, 1 to 4; each coarser level doubles the box |
| `--model <path>` | the test map | glTF mesh to bake, first mesh, first primitive |
| `--size <m>` | per scene | widest extent the geometry is scaled to |
| `--bricks <n>` | 150000 | brick budget; the bake coarsens the voxel until it fits |
| `--voxel <m>` | derived | pin the voxel size instead of deriving it |
| `--bodies <n>` | 6 | spheres dropped on it |
| `--play` | off | character controller and follow camera; defaults `--size` to 200 |
| `--omega <n>` | 1.0 | march over-relaxation; 1.0 is plain sphere tracing |
| `--shadow-steps <n>` | 48 | steps a shadow ray may take; `0` turns shadows off, which is the A/B that isolates them |
| `--detail <n>` | 1.0 | march stopping tolerance, in pixels |
| `--debug-view <n>` | 0 | 0 shaded, 1 march-step heatmap, 2 brick grid |
| `--eye x y z` / `--look x y z` | scene-fitted | place the camera, to reproduce a reported view |
| `--res <name>` | 720p | `540p` … `4k`; `--width` / `--height` override |
| `--render-scale <n>` | 1.0 | march at a fraction of the window and upscale |
| `--speed <n>` | 5.0 | fly camera |
| `--sensitivity <n>` | 0.003 | mouse look |
| `--gravity <n>` | 9.81 | downward pull |
| `--friction <n>` | 0.6 | Coulomb limit at a contact |

```sh
cargo run --release -- --model models/thing.glb --size 8 --bodies 0
```

## Screenshot

```sh
cargo run --release -- shot out.png
```

Camera parked, physics and overlay off. Two builds differ only where the shader
does, which is what makes an A/B of a rendering change readable.

## Layout

Three folders, by who is allowed to know about whom.

| module | owns |
|---|---|
| `sdf/adf` | the bake, the brick layout, the CPU sampler |
| `sdf/field` | loading the model, baking once, spawning the quad |
| `sdf/render` | material, shader-module loading, quad fitting, debug views |
| `sdf/light` | point / directional / spot, opt-in soft shadows |
| `sdf/dynamic` | moving shapes, folded into the field by min |
| `sdf/solid` | exact shapes the bake evaluates; add one here and nowhere else |
| `sdf/scenes` | the gym, the zoo, the museum, and their legends |
| `sdf/shapes` | the dynamics' shape table, and the WGSL it generates |
| `game/scene` | lights and bodies, per scene |
| `display` | window, resolution, render scale |
| `game/character` | the floating-capsule controller and its camera |
| `game/physics` | bodies, contacts, sleep |
| `game/input` | `Action`, `Bindings` |
| `game/overlay` | the stats overlay |
| `dev/benchmark` | frame timing |
| `dev/screenshot` | one deterministic frame to a PNG |
| `dev/tests` | the test suite |

Every module but `dev/tests` and `command_line` is a Bevy `Plugin`; `main.rs` is
56 lines that adds them. Nothing under `sdf/` knows a game exists, so a `bench`
run loads that folder alone — which is what makes a frame time attributable to
the renderer.

## Tests

```sh
cargo test --release
```

The bake is checked against closed-form distances: a cube's field must never
overestimate, its interior must read negative, its normals must point out of the
nearest face. Two ignored diagnostics do the measuring. The first compares the bake against
brute force over the **ideal shapes** - not against the triangles it was handed,
which is what let it report zero error for days while spheres came out faceted.
The second measures normal drift against an analytic torus.

```sh
cargo test --release how_far_the_bake_strays_from_the_truth -- --ignored --nocapture
cargo test --release how_much_of_the_normal_error_is_the_mesh -- --ignored --nocapture
```

## Known limits

- One volume, baked whole before the first frame: about 12 s per 100k bricks.
  Nothing streams, and a model change is a restart. **This is the practical cap
  on world size, not memory and not the GPU.**
- The page table is dense — one word per brick over the whole volume — so memory
  grows with world volume. A 4 km world at 1 m voxels is refused.
- Sign needs a closed mesh. An open or self-intersecting one bakes without a
  warning and gets wrong interiors.
- Normals carry ~2.9° of mean error from the trilinear reconstruction and 8-bit
  quantisation. The fix is a 16-bit atlas; it has not been done.
- Soft shadows are hard shadows: the penumbra is only as wide as the encoded
  band, four voxels.
- No CSG. Two models cannot be combined; the field is whatever the triangles say.
- Dynamics render but do not collide with each other through the field: the CPU
  field is the baked one only.
- Nothing thinner than about four voxels survives the bake; it comes out as
  shredded fragments. The bake warns when a part is that thin.
- Where two surfaces meet, the crease is reconstructed at voxel resolution and
  reads as a scalloped edge. It scales with the voxel, so `--bricks` buys it back.
- **The clipmap does not follow the player.** `--levels n` bakes nested levels
  once, around the geometry's centre. Tracking the camera needs incremental
  re-bake, which bake-once does not have. Thin seams also show where levels meet,
  because the normal taps straddle a hard resolution switch.
- **Materials melt across close junctions.** Every surface is inflated by the
  0.866-voxel bias, so two of them closer together than about 1.7 voxels fuse,
  and the fused blob takes its material from whichever source is nearest. Exact
  input does not help; only a smaller voxel does.
- A glTF's own materials are ignored: an imported model is all material 0.
- Body radius is capped at `range * 0.9`, so **physics scale is limited by bake
  resolution** — a 1 km world at 1.1 m voxels cannot carry metre-scale bodies.
