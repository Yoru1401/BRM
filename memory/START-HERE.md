---
id: start-here
type: onboarding
title: Cold start — read this first
created: 2026-08-27
updated: 2026-09-06
tags: [onboarding, handover]
links: [index, project/idk, feedback/working-style, history/wrong-turns]
---

# Start here

You are picking up **IDK**, Flori's SDF game template in Bevy 0.19.1. This file
is injected into every session, so it stays short: state, and where to look.

## Read these three

1. [feedback/working-style](feedback/working-style.md) — not preference, the
   operating agreement. One question at a time, never assume, every question ends
   with a recommendation, compile before delivering, caveman tone.
2. [history/wrong-turns](history/wrong-turns.md) — **the failure ledger.** Every
   entry cost a round. Read the measurement section before quoting any number.
3. [debt/open-threads](debt/open-threads.md) — what is unverified or fragile.

## Where things stand, 2026-09-06

Nothing is blocking. Q41's order of work is done through **D**; **E, the game
itself, is next.**

- **The field is baked.** There are **no shapes and no CSG in the shader** — but
  since 2026-09-08 there are **exact primitives in Rust**, in `sdf/solid.rs`,
  evaluated on the CPU at bake time: box, sphere, cylinder, capped frustum,
  torus. They union with imported triangles by the same per-part `min`. Geometry
  is either those solids or a glTF mesh imported with `--model`, voxelised at
  startup into a sparse brick volume: a `u32` page
  table plus one `R8Unorm` 3D texture atlas, read by hardware trilinear. The
  brush, the nine blend modes, the uniform grid, the skip pyramid, the coarse
  pass and the shadow proxy were **all deleted on 2026-09-06**.
- **Five plugins** in three folders: `sdf/{adf,field,render,light}`,
  `game/{scene,physics,input,overlay}`, `dev/{benchmark,screenshot,tests}`.
  `main.rs` is 56 lines. A `bench` run loads `sdf/` and `game/scene.rs` only.
- **Dynamic geometry is folded, not baked.** `Dynamic { start, end, radius }`
  capsules are min-folded after the brick lookup, so physics bodies and the
  character are ray-marched, lit and shadowed like the world. `min` is the only
  operator allowed there.
- **Over-relaxation is off** (`OMEGA` 1.0): on a field of bounds it eats holes in
  grazing silhouettes.
- **Rendering**: **~5.0 ms at 720p on a 1 km open world** — 101348 bricks,
  1.10 m voxels, 99 MB atlas, four lights and a shadowed sun — against 10.19 for
  the analytic renderer on 24 brushes at a fraction of the coverage. Frame time
  barely moves between a 20 m map and a 1 km world. See
  [benchmark/2026-09-06-adf](benchmark/2026-09-06-adf.md).
- **The default model is a generated 1 km world**: sine-noise terrain closed with
  a skirt and a floor as a mesh, plus 220 structures and four arches as **exact
  solids**. 90689 bricks, 1.099 m voxels, baked in 28.4 s. `--model x.glb` replaces it; `--size`, `--bricks` and `--voxel`
  scale it. `game/scene.rs` adds four lights and six falling spheres, all sized
  from the baked bounds; the spheres are ordinary Bevy meshes, not field
  geometry.
- **A bound is not a distance.** Empty bricks, clamped texels and points outside
  the volume return something safe to *step* by and meaningless to *shade* with.
  `adf_probe` reports which; only banded samples may drive the penumbra. Ignoring
  that was the entire shadow bug of 2026-09-06.
- **`--scene gym | zoo | museum`** are documentation you walk through, all pure
  solids, all baking in seconds at 0.066-0.165 m voxels. See
  [reference/scenes](reference/scenes.md). They are the only regular exercise the
  solids-only bake path gets.
- **`--debug-view 3` draws the BVH**, and view 2 tints bricks by clipmap level.
  The tree is built for the view only - the bake does not query it yet.
- **The bake folds an ordered edit list.** Solids carry an `Op` — union,
  subtract, intersect, and smooth union/subtract — so the engine has CSG at bake
  time. Culling is per-operator and getting it wrong is silent: see
  [reference/edits](reference/edits.md).
- **`--levels n` is a clipmap**, default 1 (identical to none). Four levels buy
  12x finer voxels near the centre for about +26% GPU, and a coarser far field.
  See [reference/clipmap](reference/clipmap.md). It does **not** follow the
  player yet.
- **`--play` is a playable mode**: a floating-capsule character controller
  (ride-height spring, upright torque, acceleration curve, buffered jump with
  variable gravity) with a collision-aware follow camera. It defaults the world
  to 200 m so voxels are character-scale. See
  [reference/physics](reference/physics.md).
- **The code carries no comments at all.** Anything a name cannot carry lives in
  this vault. That is why a change is not finished until the vault matches.
- **25 tests**, `cargo test --release` before every delivery. `cargo clippy
  --release --all-targets` is clean.

## Rules that will bite you

- **Never deliver Rust that has not compiled.** `cargo test --release`, here, on
  this machine.
- **The shader is compiled by running it.** A WGSL error shows in the log of
  `cargo run --release`. Silence plus an empty screen is the *bad* sign — see
  [reference/bevy-0-19](reference/bevy-0-19.md#a-shader-import-that-never-loads-stalls-the-pipeline-silently).
- **Ask whether a number is a distance or a bound before doing arithmetic with
  it.** Three separate bugs in one day came from treating the field's
  conservative bound as a real distance — the shadow penumbra, the empty-brick
  skip, and sweeping a body by `distance - radius`.
- **A miss is a flag, not a big number.** Encoding "the ray hit nothing" as
  `MAX_MARCH_DISTANCE` put a phantom plane in the sky the first time the world
  was larger than the sentinel.
- **`--shadow-steps 0` isolates the shadow march** from everything else in one
  run. Use it before believing an artefact is the field.
- **Interrogate the CPU evaluator before touching the shader.** `Adf::distance`
  and `adf_distance` compute the same thing, and a `cargo test` answers in ten
  seconds what a render round answers in five minutes. Three rounds were lost to
  not doing this on 2026-09-06.
- **Nine constants are declared twice**, in `sdf/adf.rs` and `bindings.wgsl`.
  Change one, change the other, in the same message. Nothing checks it.
- **Read the bench's `gpu_*` columns, never `frame_*`.** The frame delta includes
  the wait for present and pins to 60 Hz from the second `--repeat` block on. The
  1 km world is **4.0 ms of GPU**, the museum 2.3 ms. Read the *first* run: a
  vsynced block downclocks the GPU and reads ~12% high.
- **A screenshot is not a measurement, and one run is not a measurement.**
  Interleave A/B/A in one command, never compare across invocations, and check
  that **both halves actually built** — on 2026-09-08 the baseline half of an
  A/B failed to compile and nearly got reported as a result. A bench
  result that lands on the empty-scene floor is a blank screen until a
  screenshot says otherwise.
- **Run through `cargo run --release --`, never the exe** — Bevy looks for
  `assets/` next to the executable and silently draws nothing.
- **`memory/` is gitignored.** Nothing here is recoverable from git.
