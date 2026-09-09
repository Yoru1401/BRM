---
id: reference/build-and-run
type: reference
title: Building, running, measuring
created: 2026-08-27
updated: 2026-09-06
tags: [workflow, build]
links: [feedback/working-style, history/wrong-turns, benchmark/2026-09-06-adf]
---

# Build and run

The project lives at `C:\Users\Flori\Desktop\GameDev\IDK`. Claude has a shell on
this machine: builds, tests, benches and screenshots all run here, and the shader
is compiled **by running the app** — a WGSL error appears in the log. The
identifier audit after a large shader edit is the fallback, not the gate.

```sh
cargo test --release                          # before every delivery, no exceptions
cargo clippy --release --all-targets          # clean as of 2026-09-06
cargo run --release                           # always release when measuring
cargo run --release -- --scene gym --play      # gym / zoo / museum
cargo run --release -- --model models/x.glb   # any glTF; omit for the open world
cargo run --release -- shot before.png        # one 1280x720 PNG, then exit
cargo run --release -- bench --repeat 3 --lights 3 --shadows 1   # read gpu_*, first run
```

`--size <m>` sets the widest extent the model is scaled to.
`--bodies <n>` sets how many spheres drop on it.
`--debug-view <n>`: 0 shaded, 1 march-step heatmap, 2 brick grid, 3 BVH boxes.
`H` cycles.

**View 2 tints by clipmap level** when `--levels` is above 1, so the boxes each
level covers are visible at a glance. **View 3 draws the top 128 BVH nodes** as
wireframes over the shaded image, coloured by depth - the root encloses
everything, and the subdivisions should tighten around geometry. A tree that
looks wrong here is a bake bug you can see before you measure it.

**Scripted edits go in a file, never a heredoc.** `python3 - <<'PY'` in this
shell silently mangles backslash escapes, so a replacement containing `\n`
matches nothing and reports success. Several edits on 2026-09-08 failed that way
before it was noticed. Write the script to the scratchpad and run it by path.

**`how_far_the_bake_strays_from_the_truth`** is the instrument that settles any
argument about the field itself: it bakes a small world and compares every sample
against brute force over every triangle, per part, reporting the worst
overestimate and where. Run it before believing the bake is wrong.

```sh
cargo test --release how_far_the_bake -- --ignored --nocapture
```
`--eye x y z` and `--look x y z` place the camera, so a reported view can be
reproduced exactly — the first thing to reach for when a screenshot shows
something the default camera does not.

**`--debug-view 1` and `2` are the two instruments.** The step heatmap answers
"is the budget the problem" in one run — it ruled the budget out of the ragged
silhouettes and pointed at over-relaxation instead. The brick view draws a
checkerboard of the page grid coloured by material, which is how brick size,
grid alignment and material bleed are read off directly.

**Run through cargo, never the exe.** Bevy resolves `assets/` next to the
executable unless cargo sets the manifest dir, so `./target/release/idk.exe`
logs one ERROR and silently renders an unshaded quad — which reads as a normal
bench result.

`shot` drops physics and the overlay, waits `dev::SHADER_WARMUP` of wall clock
for the pipeline plus 240 frames, writes the PNG and exits. **The bake happens
before any of that** — a run costs 2.4 s of CPU before the first frame, and the
log line `adf: N triangles, M bricks` is the proof it happened. Two builds differ only where the
shader does, which is what makes an A/B of a rendering change readable. **What
still needs Flori is the judgement, not the picture.**

## Hot reload

`file_watcher` is on: saving any shader in `assets/shaders/` reloads live, no
rebuild. They are separate assets and `RenderPlugin` holds a handle to each — see
[reference/bevy-0-19](bevy-0-19.md#a-shader-import-that-never-loads-stalls-the-pipeline-silently).

The model does **not** hot reload, and neither does the bake. Changing `--model`,
`--size` or any bake constant is a restart. Changing a shader is not.

**A shader edit misses naga_oil's cache and stalls the pipeline for about half a
second, during which Bevy draws nothing.** Both harnesses settle on
`dev::SHADER_WARMUP` wall-clock rather than a frame count because of it — a frame
count silently rescales with frame time.

## Measuring

Read [history/wrong-turns](../history/wrong-turns.md#measurement) first — every
rule there was paid for. In short: `--repeat 4`, read run 3 or later, interleave
A/B/A in one command, never compare across invocations, and distrust a block
whose own runs 3 and 4 differ by more than a few tenths.

Two facts the bench schedule depends on:

- **Two systems in the same schedule have no order between them.** `trim_lights`
  runs in `PostStartup` precisely because it must see lights that `Startup`
  spawned.
- **The quad does not exist until the bake finishes.** Everything that touches
  the material queries it with `Single`, which skips the system rather than
  panicking, so a system that runs before the bake simply does not run. That is
  the mechanism, not an accident.

## This vault is read automatically

`.claude/settings.local.json` holds a SessionStart hook (matcher
`startup|resume|clear|compact`) injecting `START-HERE.md` and `MEMORY.md` at the
top of every session. A cold session therefore starts knowing where things stand
instead of spending a dozen tool calls rediscovering it — **which is why those
two files must stay short and true.** Individual concepts load on demand.

`memory/` is gitignored, like the hook that points at it. A clone does not carry
this vault, and **nothing here is recoverable from git** — copy before deleting.

Conformant OKF v0.2 despite living at `memory/` rather than `.okf/`. Validate
before committing bundle changes:

```sh
python "$OKF/skills/validate/scripts/okf_validate.py" memory
```

## Window, and what is actually marched

`src/display.rs` owns all three, and every entry point uses it — a normal run,
`shot` and `bench` build their window the same way, so a measurement and a
screenshot cannot silently differ in size.

- `--res 540p|720p|900p|1080p|1200p|1440p|4k`, default `720p`. `--width` and
  `--height` override either axis.
- `--fullscreen` is borderless on the current monitor. The window then takes the
  monitor's size, so `--res` no longer decides anything.
- `--render-scale <0.1..1.0>` marches at a fraction of the window and upscales.
  Below 1.0 the main camera renders into an image and a second camera presents
  it, which is why the fine print below matters.

**`--render-scale` was the largest single lever in the analytic renderer**, and
marching is still roughly linear in pixels, so it should still trade resolution
for frame time directly. The table below is from 2026-09-03 against a renderer
that no longer exists — **re-measure before quoting it.**

`spread:80 --lights 1 --shadows 1 --res 1080p`, interleaved A/B/A, runs 3-4:

| render scale | median |
|---|---|
| 1.0, marching 1920x1080 | 18.61 / 18.77 |
| **0.667, marching 1280x720** | **11.12 / 11.14** |
| 1.0 again | 20.10 / 19.95 |

About **40%**, and the trailing A block drifted 7%, so read the size of the
effect rather than the exact figure.

Two things to know before using it:

- **The scaled pass sizes once**, on the first frame the window reports a
  physical size, and does not react to a resize.
- **The presenting camera carries `IsDefaultUiCamera`**, so the overlay is drawn
  at window resolution rather than being upscaled with the scene. Without that
  the text goes through the same blur as the geometry.
