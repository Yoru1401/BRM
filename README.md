# IDK

A Bevy game template built entirely on signed distance fields. Not a game yet —
the render world, the physics world and the UI, sharing one field.

Bevy 0.19.1, Rust edition 2024.

## What it does

One SDF, evaluated twice from the **same packed bytes**: on the GPU in
`assets/shaders/sdf.wgsl` for rendering, on the CPU in `src/field.rs` for
physics. There is no separate collision geometry to keep in step.

- **Rendering** — cone-assisted ray marching on a single frustum-fitted quad.
  The vertex stage marches one coarse ray per cell, the fragment stage resumes
  per pixel and writes real depth, so ordinary Bevy 3D entities share the world
  and occlude correctly.
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
| `C` | cone marching on/off |
| `V` | hide the quad — the frame floor underneath |
| `H` | shaded / march-step heatmap |

## Layout

| module | owns |
|---|---|
| `field` | shapes, packing, the field on CPU. Depends on nothing |
| `render` | material, quad fitting, debug views |
| `world` | the authored scene |
| `input` | `Action`, `Bindings` |
| `physics` | bodies, contacts, sleep |
| `ui` | stats overlay, holograms |

Each is a Bevy `Plugin`. `main.rs` is ~80 lines that adds the six of them.

## Tests

```sh
cargo test
```

The field is checked against closed-form distances rather than against itself.
