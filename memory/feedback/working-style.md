---
id: feedback/working-style
type: feedback
title: How to run this project
created: 2026-08-27
updated: 2026-09-01
tags: [process, collaboration]
links: [feedback/code-style]
---

# Working style

Stated by Flori at the start and reinforced since. These are not preferences to
weigh — they are how the project runs.

## Questions

- **One question at a time.** Ask, get an answer, continue. Never a batch.
- **Never assume.** No editing or deciding on Flori's behalf. Ideas are welcome
  as templates, never as decisions already taken.
- **Git-repo flow.** Questions are numbered. If something learned later
  invalidates an earlier answer, say *which question* it belongs to so Flori
  can go back and change that answer, branching from there.
- **Every question ends with a recommendation.** Not a neutral menu: say which
  option is best and why, then let the answer come. Stated 2026-08-31.
- **A wordless screenshot is not approval.** Read what it shows; if it is
  ambiguous, ask.

## Verification

- **Never deliver Rust that has not been compiled.** `cargo test` runs before
  every delivery, on this machine. The rule exists because two compile errors
  were once shipped in one message.
- **The shader is compiled by running it.** A WGSL error shows up in the log of
  `cargo run --release`. The identifier audit is the fallback, not the gate — a
  block replacement once silently dropped `surface_normal`.
- **Measure, do not infer.** Where a number can be measured — a field probe, a
  brute-force ground truth, a timing run — measure it. Several confident wrong
  answers came from reading grids by eye.
- **One measurement is not a measurement**, and neither is one block. Repeat,
  interleave, and read
  [history/wrong-turns](../history/wrong-turns.md#measurement) before quoting a
  difference.

## Tone

Caveman, ULTRA intensity. Drop articles, filler and hedging. No narrating tool
calls. Technical terms, API names and numbers stay verbatim.
