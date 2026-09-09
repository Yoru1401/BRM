---
id: reference/bevy-0-19
type: reference
title: Bevy 0.19 traps worth not rediscovering
created: 2026-08-27
updated: 2026-09-04
tags: [bevy, rust]
links: [reference/engine-field, history/wrong-turns]
---

# Bevy 0.19.1

## A shader import that never loads stalls the pipeline silently

Found 2026-09-03 splitting `sdf.wgsl` into modules. It cost most of the split.

Bevy resolves a `#import` only against shaders **that are already loaded**. A
shader whose import cannot be resolved is parked in
`ShaderCache::waiting_on_import` and simply waits — **no error, no warning,
nothing drawn, forever**. The window comes up, the app runs at full speed, and
the screen is the clear colour.

Both forms fail, differently:

- `#import idk::bindings::{...}` after `#define_import_path` is a
  `ShaderImport::Custom`. Nothing loads that file, so it never resolves.
- `#import "shaders/bindings.wgsl"::{...}` is a `ShaderImport::AssetPath`, at
  least *addressable* — but Bevy still does not load it for you.

The fix is to load every module shader yourself and **hold the handles alive**:
`RenderPlugin` loads `SHADER_MODULES` at `Startup`. Drop the handles, the assets
unload, the stall comes back.

**The tell** is a run with no shader error at all and a frame time at the empty
scene floor — 2-3 ms on `spread:80` where 10 ms is right. A real WGSL syntax or
scope error *is* reported loudly, so silence is the bad sign, not noise.

## Smaller ones

- **`Single<T, With<C>>` matches nothing when two entities qualify**, so the
  system silently does not run. Adding a second camera killed four systems at
  once. Marker components, not bare `With<Camera3d>`.
- `Assets::get_mut` returns `Mut<T>`, not `&mut T`.
- `AssetLoader` implementors need `TypePath`.
- A closure cannot borrow a `DiagnosticPath` the way a plain `fn` can.
- The `bsn!` macro shipped in 0.19; the `.bsn` **asset format did not** — no file
  loader, no hot reload for scenes.
- Shader variants do not need two files: a `Material` declares
  `#[bind_group_data(Key)]` and pushes a `ShaderDefVal` in `specialize`. Looked
  up 2026-09-02, **measured, and rejected** — see
  [reference/lights](lights.md#the-fix-and-why-it-is-sound). Recorded only so it
  is not re-derived as if it were new.

## Cargo

```toml
bevy = { version = "0.19.1", features = ["file_watcher"] }
[profile.dev.package."*"]
opt-level = 3
```

That last block speeds dependencies but leaves `debug-assertions` on for the
whole profile, so it is **not** a substitute for `--release` when measuring. A
cone-marching result was once lost inside exactly that.

## `ComputeTaskPool::get()` panics outside an App

Found 2026-09-06 parallelising the bake. `ComputeTaskPool::get()` is an
`expect` on an already-initialised pool: `DefaultPlugins` sets it up, `cargo
test` does not, and every test touching the bake panicked inside
`bevy_tasks/src/usages.rs` with no hint that the pool was the problem.

`ComputeTaskPool::get_or_init(TaskPool::default)` works in both. **Any code
reachable from a unit test must use `get_or_init`.**

## A 3D texture is bound the same way as a 2D one

`#[texture(N, dimension = "3d")]` beside `#[sampler(N + 1)]` on an
`AsBindGroup`, and `image.sampler = ImageSampler::linear()` for hardware
trilinear. Two things that bite:

- **Pick a filterable format.** `R32Float` is not filterable without the
  `float32-filterable` feature and `R16Unorm` needs `TEXTURE_FORMAT_16BIT_NORM`;
  both fail at bind-group creation, not at compile. `R8Unorm` is core and
  filterable, which is why the brick atlas is 8-bit.
- **Sample at texel centres.** `(texel + 0.5) / size`, or every fetch is half a
  texel off and the whole volume shifts by half a voxel.

## `DirectionalLight` has no `shadows_enabled` field

It did in earlier versions. Shadow configuration moved out of the component;
`DirectionalLight::default()` is the whole of it now.

## A shader that exists only in memory needs `#define_import_path`

Registering a generated module with `shaders.add(Shader::from_wgsl(source, path))`
puts it in `Assets<Shader>`, but `#import "that/path.wgsl"` is a
`ShaderImport::AssetPath` and Bevy answers it by **loading the file from disk**.
It is not found, the import never resolves, and the pipeline stalls in the usual
silent way.

The generated source must declare `#define_import_path idk::shapes` and be
imported as `#import idk::shapes::{...}`, which is a `ShaderImport::Custom` and
is resolved against shaders that are already loaded — which the generated one is.

The complement of the 2026-09-03 lesson: a `Custom` import fails when nothing
loads the file, and an `AssetPath` import fails when the file does not exist.
