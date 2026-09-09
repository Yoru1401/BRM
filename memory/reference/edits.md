---
id: reference/edits
type: reference
title: The edit list, and what culling costs each operator
created: 2026-09-08
updated: 2026-09-08
tags: [csg, edits, bake, soundness]
links: [reference/engine-field, decision/authoring-in-code, debt/open-threads]
---

# Edits

A `Solid` carries an `Op`, and the bake folds the list **in part order** rather
than taking a `min` over parts. Union, Subtract, Intersect, SmoothUnion(k),
SmoothSubtract(k). Triangle meshes are always Union — a mesh part is one edit.

```
standing = MAX                       // an empty world
for each edit in order:
    standing = edit.op.fold(standing, edit.distance(point))
```

Material follows whichever edit **lowered** the value. Carving away does not
repaint what it cut.

## Culling is per-operator, and this is the part that bites

Mike Turitzin's engine culls edits through a BVH over their **AABBs** — volumes.
This engine used to bin solids by a **near-surface shell**, which is an
optimisation that is only valid for union:

| operator | culled edit contributes | safe to cull by |
|---|---|---|
| Union | nothing | shell or box |
| Subtract | nothing, *if the point is outside it* | **box only** |
| Intersect | "outside", never nothing | **never** — binned into every brick |
| Smooth\* | as their hard forms, plus the blend width | box only |

A brick deep inside a carved sphere has that sphere's *surface* far away. Shell
binning drops it, and the brick keeps geometry that should have been removed. So
the shell test is gone and everything bins by its box.

**Intersect edits go into every brick's list.** A list cannot express "this edit
was culled", and an absent intersector must still read as "outside". Cheap while
there are one or two of them; nothing warns if there are two hundred.

## Binning by box defeats burial, and one fold per brick fixes it

`buried_bricks` asks *"does this brick's list hold this part"*. Bin by box and
that is true for every brick inside a solid, so the flood finds no seed and
**nothing is ever buried**. Every interior brick then becomes a candidate and
gets its thousand voxels evaluated. On 2026-09-08 that took the parallel test
suite from passing to aborting, in two different ways on two runs.

The fix is a single fold at each brick's centre:

```
skip the brick when  abs(folded(centre)) > size * 0.866 + range + blur + voxel
```

Sound because the folded field is Lipschitz-1: if the centre is that far outside
the band, no point in the brick can be inside it. `blur` is the widest smooth
width in the scene, since a smooth min can bow past the true minimum.

**One fold per brick replaces a thousand.** It is conservative sign-change
allocation, and it is the direction the reverted 2026-09-07 round was pointing
in — this time as a single change, not four.

## Not done

- **Intersect has no test.** Union and Subtract do.
- A cavity fully enclosed by geometry is flooded as `SOLID` by `seal_interior`,
  because the flood starts outside and never reaches it. Harmless until someone
  stands in one.
- CSG on SDFs is not a true distance field near a carved rim: `max(a, -b)`
  overestimates there. Mike has the same limit. The 0.866 voxel bias absorbs
  some of it; nothing measures how much.
