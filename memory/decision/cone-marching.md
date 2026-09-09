---
id: decision/cone-marching
type: decision
title: Cone marching, and why it was deleted
created: 2026-08-27
updated: 2026-09-03
tags: [renderer, performance]
links: [reference/engine-field, benchmark/2026-09-06-adf]
---

> **Reversed 2026-08-31.** Cone marching is gone. The record below is why it
> existed, why it stopped being worth it, and what it cost while it did.

# Cone marching

Technique: <https://medium.com/@nabilnymansour/cone-marching-in-three-js-6d54eac17ad4>

A frustum-fitted quad subdivided `SUBDIV_Y = 32` times vertically. The vertex
stage marched a *cone* per vertex and stopped when the cone was no longer
empty; the fragment stage resumed from there. One draw call.

## Why it went

Measured on `spread:80` and `grid:80`, everything else on, `--repeat 5`, with
`PresentMode::Immediate`:

| scene | cone on | cone off | worth |
|---|---|---|---|
| spread:80 | 14.45 | 15.08 | 4.2% |
| grid:80 | 10.61 | 10.77 | 1.5%, inside block spread |

It was worth 27.6% before the acceleration grid. The grid made the per-pixel
march cheap, and a coarse pass in front of a cheap march is mostly overhead.

Against that 4%: a whole vertex stage, `SUBDIV_Y`, `CONE_PAD`,
`vertical_cell_count`, the `ConeResult` plumbing, a debug key, a subdivided
quad mesh, and **the reason `cone_march` could not use the grid** - which left
the exact O(N) field with a second caller.

## The real reason, though

A vertex value is **interpolated across its cell**, so it has to be
conservative for every pixel in that cell, not just for the vertex. That is a
condition nothing in a container can test, and it broke twice:

- vertices that missed the scene box parked at `MAX_MARCH_DISTANCE`, and the
  interpolation pushed neighbouring pixels past the surface - the purple-row
  dents
- vertices that stopped at grid cell walls returned wildly different distances,
  and the interpolation put pixels in between inside geometry - a screen full
  of stripes, 2026-08-31

Two of the three worst rendering bugs in this project came from the same
property of the same feature, for 4%.

## It came back, 2026-09-03

A blog post on recursive quad subdivision prompted a second attempt that meets
the condition below: conservative per-quad, point-sampled, no interpolation
anywhere. It renders pixel-identically and lives behind `--hierarchical`.
Whether it is *faster* is still unknown - the machine was too contended to tell,
and readings ranged from 47% faster to 12% slower on the same build. See
[benchmark/2026-09-06-adf](../benchmark/2026-09-06-adf.md). The coarse pass it
describes was deleted unmeasured on 2026-09-06 with the rest of the analytic
renderer; the brick page table skips empty space for free.

## If it comes back

**The grid did go away, on 2026-09-06, and the answer is still no.** Empty space
is now skipped by the page table for the cost of one `u32` read, which is the
job a coarse pass was for. The remaining condition is unchanged: a coarse pass
must be conservative **per-cell rather than per-vertex** — a bound over the whole cell's footprint
instead of one corner of it, point-sampled, with no interpolation step anywhere.
That is the condition `--hierarchical` meets.

Resolution does not rescue the old form either: the saving stays proportional to
the baseline while the interpolation hazard gets *worse*, since a fixed
subdivision means each cell covers 2.25x more pixels at 1080p and one
non-conservative vertex poisons all of them.
