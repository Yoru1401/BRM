---
id: index
type: index
title: IDK project memory
created: 2026-08-27
updated: 2026-09-06
tags: [index, okf]
links: [start-here]
okf_version: "0.2"
---

# IDK — memory vault

**New session? Read [START-HERE](START-HERE.md) first.**

Open Knowledge Format. Every file is markdown with YAML frontmatter; `type` is
required. This index is the only place that lists everything, so a new file is
not findable until it is added here.

**What earns a file here:** a decision and its reasoning, a measured number and
how it was measured, a bug whose cause was non-obvious, a rule learned the hard
way. **Not** what the code already says — the source is the description of
itself. When something is removed from the code, the entry describing it is
deleted and whatever it *proved* moves into
[history/wrong-turns](history/wrong-turns.md).

## Onboarding

- [START-HERE](START-HERE.md) — state, and the rules that bite

## Project

- [project/idk](project/idk.md) — what is being built, and the order of work

## The failure record — read before believing anything

- [history/wrong-turns](history/wrong-turns.md) — every wrong turn, and what
  found it: measurement, tests that lied, silent renders, field bugs, process
- [history/decisions-log](history/decisions-log.md) — answered questions, in
  order, so one can be reopened and branched from

## Reference

- [reference/code-map](reference/code-map.md) — layout rules and the constants that have a reason
- [reference/build-and-run](reference/build-and-run.md) — commands, hot reload, how to measure
- [reference/edits](reference/edits.md) — the ordered fold, and what culling costs each operator
- [reference/clipmap](reference/clipmap.md) — nested levels, the trade they make, and the bound that must not be clamped
- [reference/scenes](reference/scenes.md) — the gym, the zoo and the museum: documentation you walk through
- [reference/engine-field](reference/engine-field.md) — the baked field's contracts: bricks, sentinels, the underestimate rule
- [reference/lights](reference/lights.md) — lights, and the proxy that baking made unnecessary
- [reference/physics](reference/physics.md) — bodies against the baked field
- [reference/bevy-0-19](reference/bevy-0-19.md) — engine traps worth not rediscovering

## Decisions

- [decision/authoring-in-code](decision/authoring-in-code.md) — **the world is imported and baked**, not written twice
- [decision/single-brush](decision/single-brush.md) — one brush, then none; what the brush proved on its way out
- [decision/cone-marching](decision/cone-marching.md) — built, measured, deleted; the condition for its return
- [decision/physics-scope](decision/physics-scope.md) — rigidbodies only, softbodies removed

## Plans

- [plan/open-world](plan/open-world.md) — the large world: what baking finished, and the streaming half that is left

## Benchmarks

- [benchmark/2026-09-06-adf](benchmark/2026-09-06-adf.md) — 10.19 ms to 2.62, what the bake costs, and what the analytic renderer measured

## Working agreements

- [feedback/working-style](feedback/working-style.md) — how to run this project
- [feedback/code-style](feedback/code-style.md) — how the code is written

## Open

- [debt/open-threads](debt/open-threads.md) — unverified, deferred, known-fragile
