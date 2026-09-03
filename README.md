# IDK

A Bevy game template built entirely on signed distance fields. Not a game yet —
the render world, the physics world and the UI, sharing one field.

Bevy 0.19.1, Rust edition 2024.

## What it does

One SDF, evaluated twice from the **same packed bytes**: on the GPU in
`assets/shaders/` for rendering, on the CPU in `src/sdf/field.rs` for physics.
There is no separate collision geometry to keep in step.

The shader is seven files — `sdf.wgsl` holds only the entry points and imports
`bindings`, `shapes`, `operations`, `scene`, `marching` and `lighting`.

The source carries no comments. What a name cannot say lives in `memory/`.

- **Rendering** — ray marching on a single frustum-fitted quad, one ray per
  pixel, against a uniform grid of per-cell shape lists. Over-relaxed steps
  (Keinert et al.). The fragment stage writes real depth, so ordinary Bevy 3D
  entities share the world and occlude correctly.
- **Geometry** — one entity per brush, authored in `bsn!`. Every brush is a
  box; `Transform` sets its size, `Modifiers { round, bevel, thickness, cone }`
  its shape, and `CsgOperation` + `Albedo` how it blends and looks. A full
  round is an exact sphere, a full bevel an exact cylinder, a cone a pyramid.
  Brushes blend in child order.
- **Physics** — sphere rigidbodies (a fully-rounded brush) against the field, with rotation, friction,
  a Coulomb limit and sleep.

## Run

```sh
cargo run --release
```

Debug builds are misleading: `debug-assertions` are profile-wide and put a
~2 ms floor under every frame.

| key | does |
|---|---|
| `WASD` / `Space` / `LShift`, right-drag | fly camera |
| `V` | hide the quad — the frame floor underneath |
| `H` | shaded / march-step heatmap |

## Benchmark

```sh
cargo run --release -- bench empty        # the march against an empty field
cargo run --release -- bench grid:20      # 20 boxes tiling a fixed slab
cargo run --release -- bench spread:80    # 80 boxes scattered over a level
cargo run --release -- bench spread:80 --no-grid --repeat 3
```

Prints one tab-separated line of min / median / p95 frame ms and exits. The
count scenes tile the same volume, so only the shape count changes - not the
screen coverage. Use `--repeat 4` and read run 3 or later: the first block is
still warming up, and can catch the shader before it has loaded.

## Flags

Every knob that used to need a recompile takes a flag, on any run — ordinary,
`bench` or `shot`. The module that owns a value reads its own flag; the default
stays a `const` beside it.

| flag | default | what |
|---|---|---|
| `--omega <n>` | 1.2 | march over-relaxation; 1.0 is plain sphere tracing |
| `--grid <n>` / `--no-grid` | 16 | acceleration grid cells along the longest axis |
| `--no-cull` | on | the per-shape box reject |
| `--shadow-steps <n>` | 48 | steps a shadow ray may take |
| `--speed <n>` | 5.0 | fly camera |
| `--sensitivity <n>` | 0.003 | mouse look |
| `--gravity <n>` | 9.81 | downward pull |
| `--friction <n>` | 0.6 | Coulomb limit at a contact |

```sh
cargo run --release -- --speed 12 --gravity 3
```

## Screenshot

```sh
cargo run --release -- shot out.png
```

The authored world, camera parked, physics and overlay off. Two builds differ
only where the shader does, which is what makes an A/B of a rendering change
readable.

## Layout

Three folders, by who is allowed to know about whom.

| module | owns |
|---|---|
| `sdf/field` | the plugin, `SdfScene`, packing to the GPU, the field on CPU |
| `sdf/brush` | the `Brush`, its modifiers and the bytes they pack into |
| `sdf/distance` | the rounded-box kernel |
| `sdf/blending` | the nine blend modes |
| `sdf/bounds` | scene bounds and the per-shape cull bound |
| `sdf/grid` | the acceleration grid and the shadow proxy |
| `sdf/render` | material, shader-module loading, quad fitting, debug views |
| `sdf/light` | point / directional / spot, opt-in soft shadows |
| `game/world` | the authored scene |
| `game/physics` | bodies, contacts, sleep |
| `game/input` | `Action`, `Bindings` |
| `game/overlay` | the stats overlay |
| `dev/benchmark` | generated scenes, frame timing |
| `dev/screenshot` | one deterministic frame to a PNG |
| `dev/tests` | the test suite |

Every module but `dev/tests` and `command_line` is a Bevy `Plugin`; `main.rs`
is ~50 lines that adds them. Nothing under
`sdf/` knows a game exists, so a `bench` run loads that folder alone - which is
what makes a frame time attributable to the renderer.

## Tests

```sh
cargo test
```

The field is checked against closed-form distances rather than against itself.
