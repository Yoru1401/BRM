---
id: feedback/code-style
type: feedback
title: How the code is written
created: 2026-08-27
updated: 2026-09-03
tags: [style, ponytail]
links: [feedback/working-style]
---

# Code style

## No comments

**The code carries no comments at all.** Not inline `//`, not `///` doc
comments, not `//!` module docs. 1324 comment lines were deleted on 2026-09-03
at Flori's instruction, and the rule is standing: names carry the meaning, and
anything a name cannot carry goes in this vault instead.

What that means in practice when writing code here:

- A function that would need a comment needs a better name, or needs splitting
  until each part has an obvious one.
- A constant that would need a comment explaining its value gets that
  explanation in the vault, under the reference file for its subsystem.
- A `ponytail:` shortcut marker has nowhere to live in the source now. Put it
  in [debt/open-threads](../debt/open-threads.md) instead, which is where it was
  being harvested to anyway.

Before deleting a comment that states a **measured number, a date, or a trap**,
check the vault holds it. The same applies to deleting *code*: the brush, the
grid and the shadow proxy all went on 2026-09-06 and what they proved was moved
into [history/wrong-turns](../history/wrong-turns.md) and
[decision/single-brush](../decision/single-brush.md) first. See
[reference/engine-field](../reference/engine-field.md) and
[reference/build-and-run](../reference/build-and-run.md).

## Self-documenting

Names carry the meaning, and they are spelled out: `command_line`, not `args`;
`benchmark`, not `bench`; `screenshot`, not `shot`; `overlay`, not `ui`. `sdf`
survives because it is the domain's own acronym.

Struct fields are named scalars rather than packed vectors wherever the layout
allows it — `RenderParams` has `brick_size`, `voxel`, `tan_half_fov`, not `s`
and `r`. Both evaluators index it by name, and that is the only thing standing
between them and silent drift.

## Ponytail

Laziest solution that works. Climb the ladder and stop at the first rung that
holds. No speculative abstraction, no scaffolding for later.

Deliberate shortcuts with a known ceiling are recorded in
[debt/open-threads](../debt/open-threads.md), naming the ceiling and the upgrade
path.

## Tests

Non-trivial logic leaves one runnable check behind. What matters here:

- A test should encode **what the field does**, not what the parameter names
  suggest. Several assertions had to be deleted because they had frozen an
  assumption: a solid floor under a shell, a constant wall up a taper.
- **Uniform test shapes hide non-uniform bugs.** A wall measured against the
  wrong axis passed every cube test and failed on the first plate.
- Sentinel values need their boundary tested. `thickness = 0` collided with the
  "solid" sentinel and no test covered either end of the range.
- A test that can be satisfied by *both* sides failing is not a test.
  `the_bake_never_overestimates_the_distance` would pass on an all-zero atlas,
  so it also counts how many samples *tracked* the true field and fails if too
  few did.
- **A test must not freeze a tunable either.** Two ADF tests broke when
  `RANGE_VOXELS` changed, not because the bake broke but because they had
  hard-coded the old band. Ask the field for its own `range()`.
- **A test that pins a new implementation to an old one is scaffolding.** It
  expires the moment the new one is trusted and it cannot catch a fault both
  share. `the_kernel_matches_the_uberprim_it_replaced` carried the kernel swap
  over 5000 random inputs and was deleted with the kernel it compared against;
  what guards the kernel now is closed-form exactness against analytic answers,
  which is the stronger claim.

The full ledger of tests that lied is in
[history/wrong-turns](../history/wrong-turns.md#tests-that-passed-against-live-bugs).
