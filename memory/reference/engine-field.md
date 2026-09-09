---
id: reference/engine-field
type: reference
title: The baked field's contracts and traps
created: 2026-08-27
updated: 2026-09-06
tags: [renderer, wgsl, gpu, adf]
links: [reference/bevy-0-19, benchmark/2026-09-06-adf, history/wrong-turns]
---

# Engine contracts

Only the rules that are not visible in the source. The code says what the field
computes; this says what it is not allowed to do.

## The field is a two-level sparse brick grid

`src/sdf/adf.rs` bakes triangles into it; `assets/shaders/adf.wgsl` reads it.
Nothing else knows what a shape is.

- **Brick**: 8³ voxels plus a one-voxel apron, so 10³ samples per brick. The
  apron is what lets hardware trilinear stay inside one brick's own region of
  the atlas — sample coordinates within a brick are clamped to `[1, 9]`.
- **Page table**: one `u32` per brick over a dense `bricks.x * y * z` grid,
  uploaded as a storage buffer padded to `PAGE_WORDS`. The top byte is a tag:
  below `0xfe` the whole word is an atlas slot index, `0xff` means empty and
  `0xfe` solid, and in those two cases the low 24 bits hold the brick's
  **clearance**.
- **Atlas**: one `R8Unorm` 3D texture of `slots³` brick slots. **Both the atlas
  and the page buffer are sized to the model at bake time** and never resized
  after — `slots` is the cube root of the brick count and reaches the shader as
  a uniform. A 1 km world is 47³ slots, 470³ texels, 99 MB.

## Both sentinels are load-bearing, and so is the clearance

`EMPTY` means *no triangle within `RANGE_VOXELS` of this brick* — the same band
the atlas encodes, so a brick is allocated exactly when it has something worth
storing. **`coarse` is a chamfer distance transform over the brick grid**, seeded
at zero on every allocated brick and swept twice in each direction with 26
neighbours weighted 1, √2, √3:

```
safe step = inscribed distance to this brick's wall + coarse[brick] + range
```

The proof: all surface lies inside allocated bricks, so the distance from a point
to the surface is at least its distance to the allocated region; a segment from
the point to the surface must cross this brick's wall, so the two add; and the
unallocated bricks in between hold no triangle within `range`, so that adds too.
One `f32` per brick, 1.6 MB on the 1 km world.

It replaced a **count** of bricks in Chebyshev metric, which quantised every step
to a brick multiple and underestimated a diagonal by up to √3. That is the whole
of the empty-space skipping — there is no separate acceleration structure.

## Two rules that look like precision compromises and are not

- **The `range` clamp.** A brick's triangle list only holds triangles within the
  bin's reach, so a distance computed from it is correct **only inside that
  reach**. Storing the true value beyond it — as a per-brick min/max window
  would — overestimates, and the march walks through walls. **The accurate band
  can never be wider than the bin's reach**, and widening the reach costs bricks
  and bake linearly: 4 to 8 voxels is +36% bricks, +36% atlas and 2x the bake, for
  a step count that is visibly lower and a frame time that is not.
- **The 0.866 voxel bias.** Half a voxel diagonal is the worst-case trilinear
  overestimate, and the worst case happens at creases. Measured on a random
  probe, 0.25 voxel looks safe; measured against a cylinder cap edge it is not.
  The cost is that every surface is inflated by 0.87 voxel, which is what merges
  anything closer together than twice that.

**Both scale only with the voxel.** They are the grid's real price, and the only
lever on them is resolution.

`SOLID` is the same emptiness *inside* the model, and it returns the negative of
that bound. It is found by a flood fill over empty bricks from the volume
border: what the outside cannot reach is interior. **Without it the centre of a
closed model reads positive**, which is invisible in the render (a ray hits the
shell first) and wrong for physics.

The two-brick margin around the model exists so the border bricks are always
`EMPTY` and the flood fill always has a seed.

## Estimates must never overshoot, and interpolation does

A march step longer than the real gap walks through the surface. Trilinear
interpolation between *exact* samples overestimates by up to half a voxel
diagonal in the worst case, so every texel stores `d - (voxel diagonal / 2)`.
The interpolant is then a guaranteed underestimate and the surface thickens by
that amount — 13.5 mm at the 15.6 mm voxels a 4 m model gets, about 0.3% of the
model.

Quantisation is the second overshoot risk. Values are clamped to `±RANGE_VOXELS`
(2 voxels) and rounded to 8 bits, so one quantum is 0.016 voxel — three orders
below the diagonal bias, which is why rounding rather than truncating is safe.

**Outside the encoded range the field is a bound, not a distance.** A clamped
texel, an `EMPTY` brick and a point outside the volume all return something safe
to step by and meaningless to *shade* with. `adf_probe` returns that distinction
in its second component, and `Adf::probe` mirrors it on the CPU. It is not
decoration:

- **Marching and normals may use a bound.** A march only needs safety, and a
  normal is only ever taken at a hit, where the value came from the band.
- **The penumbra may not.** `shadow_factor` tracks `min(softness * d / t)`, so a
  bound that undershoots the true distance darkens a pixel that should be lit.
  Bounds are piecewise constant per brick, so this renders as *brick-shaped
  patches and speckle across every shadowed surface*. Only banded samples are
  allowed to darken.
- **Contact resolution may not.** A bound crosses a body's radius long before the
  surface does, so bodies hover and the normal is the gradient of the bound.
  Physics resolves penetration only on banded samples. `bound >= radius` already
  proves no contact, and `bound < radius < range` can only happen inside an
  allocated brick, so the gate is exact.
- **Over-relaxation may not.** Keinert's rewind assumes true distances; on bounds
  it lets a grazing ray jump a ridge and report a miss. `OMEGA` is 1.0.

`RANGE_VOXELS` is therefore also the width of the penumbra: outside it, shadows
are hard by construction.

## Dynamics are folded, never baked

Moving geometry cannot live in a baked volume, so it is a **capsule list folded
by `min` after the lookup** — the seam rule from
[plan/open-world](../plan/open-world.md), built. `Dynamic { start, end, radius }`
is a component; anything that moves keeps one up to date, `sdf/dynamic.rs`
uploads them, and `adf_probe` takes the nearer of the baked value and the
capsules. A sphere is a capsule with `start == end`.

Consequences worth knowing:

- **Dynamics are exact**, so a folded sample is always flagged banded — it may
  drive the penumbra and it has a real normal.
- `min` of a bound and an exact distance is still an underestimate, so nothing
  about the march or the skip changes.
- **`min` is the only operator allowed.** Subtract, intersect and shell would all
  need the field before the fold, and the bake has already collapsed it.
- **The union must stay crisp, and that is a normal problem, not a field
  problem.** `min` gives a hard crease; a four-tap normal taken across the folded
  field averages both surfaces over a voxel and renders it as a fillet.
  `surface_normal` therefore decides once which field owns the hit and takes
  **either** the capsule's exact analytic gradient **or** four taps of
  `baked_probe`. Crisper *and* cheaper: four fewer dynamic-loop evaluations per
  shaded pixel, worth most of the fold's cost.
- The loop runs inside the march, the shadow march and every normal tap, so its
  cost is multiplied. A single bounding sphere over all dynamics rejects it in
  one test when the point is far — which is most of the screen.
- **Physics does not see the fold.** `Adf::distance` is the baked field only;
  bodies collide with each other through `sphere_pair_correction`, and the
  character does not collide with bodies at all.

## Two evaluators, still

`Adf::distance` in Rust and `adf_distance` in WGSL read the same bytes with the
same arithmetic. The one asymmetry is `adf_probe`: the shader also reports
whether a sample is banded, which only the penumbra uses and physics does not
need. That is far less drift surface than the analytic field had — no
shape struct, no blend modes — but nine constants are declared **twice**,
in `sdf/adf.rs` and in `bindings.wgsl`: `BRICK`, `APRON`, `SPAN`,
`RANGE_VOXELS`, `NORMAL_TAP`, `TAG_EMPTY`, `TAG_SOLID`, `TAG_SHIFT` and
`CLEARANCE_MASK`. The atlas layout used to be two more; it became a uniform when
the atlas started being sized to the model. Change one, change the other in the same
message. Nothing checks it.

## The atlas is written once, on purpose

Resizing or reallocating a GPU resource invalidates the bind group while the
material still points at the old one — it has cost this project a blank screen
twice. So the quad, its material, its page buffer and its atlas image are all
created **after** the bake, by `bake_when_ready`, and never touched again.
Everything that queries the quad uses `Single`, which simply skips its system
until the quad exists.

## Materials are a second volume, at voxel resolution

`paint` is an `R8Unorm` 3D texture beside the distance atlas: **8³ per brick, no
apron, nearest sampled**, half the memory of the distance atlas (50 MB where the
distances are 99). Each voxel holds the material of the triangle nearest to it,
which falls out of the bake for free — `signed_distance` already knows which
triangle won.

**It was per brick first, and that was wrong.** Packing the index into the page
word's spare bits cost nothing and needed no second volume, but a brick is 8
voxels, so every material bled up to 8 voxels past its own geometry — cones
painting the terrain around them, plainly visible in `--debug-view 2`. Per voxel
is exact and costs 50 MB and about 0.4 ms.

The march never touches `paint`: it is read once per shaded pixel, so it adds no
bandwidth to the inner loop. Dynamics carry their index in their own struct.

The palette is a storage buffer of `albedo` and `gloss`, and **it belongs to the
world, not to the engine.** `Adf::palette` is filled by whoever built the
geometry — `sdf/field.rs` for the generated world — so no material is named
anywhere in `sdf/render.rs` or in any shader. `gloss` drives a Blinn-Phong lobe
in `light_contribution`, which is the only reason two materials of the same
colour look different.

## The sign is per part, and the union is a min

A `Surface` carries, per triangle, a **material** and a **part**. A part is one
closed mesh. Each brick's triangle list is sorted by part at bin time, so
`signed_distance` walks it in runs: it finds the nearest triangle *within each
part*, signs that part with its own angle-weighted pseudonormal, and returns the
**minimum over parts**.

That is the exact SDF of a union of solids where it matters — outside, it is the
distance to the nearest surface; inside, it is a magnitude underestimate, which
is the safe direction. Treating every triangle as one soup instead gives wrong
signs wherever two closed meshes interpenetrate, and the generated world
interpenetrates everywhere: every box, cone and cylinder is sunk into the
terrain.

An earlier attempt used "sign = inside any part, magnitude = nearest triangle".
It looks equivalent and is not: at a point outside the terrain but inside a cone,
the magnitude collapses to zero on the *buried* terrain surface, planting a
spurious isosurface inside the union. Take the min of the signed values.

### A part can enclose a brick without appearing in it

`min` over the parts *present in the brick* is not the union: a brick near shape
B but deep inside shape A never sees A's triangles, so it reads positive where
the union is solid, and B's buried shell bakes as a real surface.

`buried_bricks` closes it. For each part it floods, at brick resolution and
within that part's own inflated AABB, over bricks the part does not touch;
whatever the flood cannot reach is inside that part. A brick inside a part that
is **absent from its own list** is left unallocated, and `seal_interior` then
marks it solid, because nothing outside can reach it either.

The flood is per part but bounded by that part's AABB, so 224 small structures
cost nothing and only the terrain floods the whole grid. It removes about 20% of
the bricks, so it pays for itself.

## Nothing smaller than a few voxels survives

A part thinner than about four voxels bakes as noise — interpenetrating shells
and shredded fragments — and no amount of care in the sign or the union changes
that. `bake` warns when any part is that thin, naming the thinnest, because the
symptom is otherwise indistinguishable from a logic bug and has cost two rounds
of chasing one.

## Shape maths is generated, never hand-written in WGSL

`sdf/shapes.rs` holds a table of primitives, each a name plus a WGSL body for its
distance and its exact normal. `wgsl()` emits the functions and a `switch` over
the kind packed in `Dynamic::flags`, and `RenderPlugin` registers the result as a
`Shader` asset at startup. Adding a primitive is one table entry; no hand-written
shader mentions a shape.

**A generated module must be imported by name.** `#import "shaders/x.wgsl"` is an
asset path and Bevy will try to load that file off disk — it does not look in
`Assets<Shader>` — so a module that exists only in memory has to declare
`#define_import_path idk::shapes` and be imported as `idk::shapes::{...}`. The
first attempt used the path form and logged `Path not found` twice per run.

## A miss is a flag, never a distance

`ray_march` reports whether it hit, and the fragment discards on that. It used to
compare the returned distance against a `MAX_MARCH_DISTANCE` sentinel, which
worked only while the world was smaller than the sentinel — see
[history/wrong-turns](../history/wrong-turns.md). `MAX_MARCH_DISTANCE` now means
nothing but "the distance to a directional light".

## The fragment stage writes real depth

Stock Bevy 3D entities share the world with the marched field and occlude
correctly against it, with no renderer support. The physics bodies are ordinary
`Mesh3d` spheres for exactly this reason: dynamics cost the field nothing.
