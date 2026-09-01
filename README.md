# IDK

A Bevy game template built entirely on signed distance fields. Not a game yet —
the render world, the physics world and the UI, sharing one field.

Bevy 0.19.1, Rust edition 2024.

## What it does

One SDF, evaluated twice from the **same packed bytes**: on the GPU in
`assets/shaders/sdf.wgsl` for rendering, on the CPU in `src/field.rs` for
physics. There is no separate collision geometry to keep in step.

- **Rendering** — ray marching on a single frustum-fitted quad, one ray per
  pixel, against a uniform grid of per-cell shape lists. Over-relaxed steps
  (Keinert et al.). The fragment stage writes real depth, so ordinary Bevy 3D
  entities share the world and occlude correctly.
- **Geometry** — one entity per brush, authored in `bsn!`. Brushes carry
  `SdfShape` + `Transform` + `Modifiers` + `CsgOperation` + `Albedo` and blend
  in child order. The modifier set (round, bevel, thickness, cone, sharpen) is
  a port of SDF Modeler's, matched against the editor shape by shape.
- **Physics** — sphere rigidbodies against the field, with rotation, friction,
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
cargo run --release -- bench spread:80      # 80 boxes scattered over a level
cargo run --release -- bench spread:80 --no-grid --repeat 3
```

Prints one tab-separated line of min / median / p95 frame ms and exits. The
count scenes tile the same volume, so only the shape count changes - not the
screen coverage.

## Layout

| module | owns |
|---|---|
| `field` | shapes, packing, the acceleration grid, the field on CPU. Depends on nothing |
| `render` | material, quad fitting, debug views |
| `world` | the authored scene |
| `input` | `Action`, `Bindings` |
| `physics` | bodies, contacts, sleep |
| `ui` | the stats overlay |

Each is a Bevy `Plugin`. `main.rs` is ~80 lines that adds the six of them.

## Tests

```sh
cargo test
```

The field is checked against closed-form distances rather than against itself.
