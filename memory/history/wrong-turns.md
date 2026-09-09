---
id: history/wrong-turns
type: history
title: Every wrong turn, and what actually found it
created: 2026-08-27
updated: 2026-09-06
tags: [history, debugging, failures]
links: [feedback/working-style, reference/engine-field, decision/cone-marching]
---

# Wrong turns

**Read this before believing a result.** Everything here cost at least one
round. Kept because the *method* that found each one is reusable, and because
most of them looked settled at the time.

## Three changes again, and the BVH gather came apart

2026-09-08. Stage 2 was meant to be one thing: a BVH over edit boxes. It became
three - query per brick instead of scattering per edit, `buried_bricks` deleted
because the fold cull looked like it subsumed it, and `near` redefined from
"brick holds an edit" to "brick was allocated". Every one of them was defensible
on its own. Together they were unreadable.

The damage, measured: museum **56978 bricks in 6.24 s becoming 85125 in 16.15 s**;
open world 90689 in 28.4 s becoming 99693 in 42.4 s; and four tests failing with
overestimates up to **101.87 voxels**.

The one useful signal was which tests failed. Every **solid** test passed -
including subtraction and the clipmap - and every **mesh** test failed. That
narrows it hard, and it still was not enough to guess from: three attempts at a
cause were all wrong.

Reverted the bake to the green commit and kept `bvh.rs` for the debug view only.
The tree is visibly well formed in view 3, so the build is probably sound and the
fault is in the gather, the burial removal, or `near` - which is exactly the
question a single change at a time would have answered.

**The rule keeps being the same one, and knowing it is not the same as obeying
it.** The vault has said "change one, run the truth probe, keep or drop it" since
2026-09-07. This is the second time it was written down and then not followed.

## Binning by box defeated burial, and the suite died two different deaths

2026-09-08. Subtraction needs edits binned by their **box**, not by a shell
around their surface — a brick deep inside a carved sphere is far from that
sphere's surface and must still know about it. Making that change broke the test
suite in a way that took three runs to read, because it broke it *differently*
each time: `STATUS_STACK_BUFFER_OVERRUN` once, a bare abort the next, and all 25
tests passing when run serially.

`buried_bricks` decides burial by asking whether a brick's list holds a part.
Bin by box and that is true for **every** brick inside a solid, so the flood
finds no seed, nothing is buried, and every interior brick gets its thousand
voxels evaluated. Serial runs survived it; parallel ones ran the machine out.

Two lessons, and the second cost more than the first:

- **A cull and a burial test that read the same data are one mechanism.** The
  shell test was doing burial's job at bin time. Removing it silently removed
  burial too.
- **Check for zombies before believing a resource failure.** A test process from
  a run killed earlier was still alive, holding the linker's output file and
  competing for the machine. `Get-Process idk` did not find it: the binary is
  named `idk-<hash>.exe`. It confounded at least one of the three runs.

## A render that got faster and darker at once

2026-09-08. The clipmap's first measurement read **1.46 ms** against 4.9 for a
single level — a rendering change that added work and appeared to halve the cost.
That should have been the first alarm. It was not; the screenshot looked dim and
washed out and I read that as the far field being coarser.

Both symptoms were one bug. An empty brick's bound was being clamped to its
level's box wall on *every* level including the coarsest, whose box already holds
all the geometry. The bounds came back far too small, rays hit the step budget
and quit, and quitting is cheap. Three character tests said the same thing in
plainer words — the ride spring settled at 0.43 m instead of 1.0 — and they are
what actually found it.

**A number that improves in the wrong direction is a bug report.** So is a
picture that gets darker. Neither is a result.

## Blaming the compositor for an instrument that measured the wrong thing

2026-09-08, later the same day. Every bench read 16.7 ms, exactly 60 Hz. The
diagnosis offered was that the compositor was forcing vsync and that the window
commit e6fa1b2 was the prime suspect, marked `[Likely]`. Three runs went into
testing that theory: `AutoNoVsync` instead of `Immediate`, then fullscreen, then
an A/B whose baseline half did not compile. All three were chasing the wrong
thing.

**The bench recorded `time.delta_secs()`** — wall-clock frame delta, which
includes the wait for present and therefore *cannot* read below the refresh
interval. The render cost was never in that number and no present mode was ever
going to put it there. Bevy's `RenderDiagnosticsPlugin` had the GPU timestamps
the whole time; nothing asked for them.

With the GPU column added, the 1 km world reads **4.0 ms**, agreeing with the
4.08 ms measured back in September and clearing the analytic-solids change of a
regression it was never guilty of.

Vsync is real, but secondary: it engages from the second `--repeat` block
onward. It masks the frame column, and it also inflates the GPU column by about
12%, because a GPU idling 12 ms out of every 16 drops its clocks. **Read the
first run of a repeat.**

**Before blaming the environment for a number, check what the number is
measuring.** A frame delta and a render cost are different quantities, and only
one of them was ever wanted here.

## Predicting a speed-up, then being unable to measure it

2026-09-08. Analytic primitives were offered with "much faster bake" attached.
The bake measured **28.4 s** afterwards, against **8.5 s** written in the vault
on 2026-09-06 — but that figure predates the paint volume, the chamfer coarse
pass and the per-part union, so the comparison is worth nothing and the
prediction was never tested. Three attempts to get an honest frame time also
failed:

- An interleaved A/B in one command **did not compile on the baseline half**.
  Reverting three files left the rest of an uncommitted round referencing
  `MAX_MATERIALS`. Half an A/B is not an A/B.
- Every bench since reports **16.7 ms, exactly 60 Hz**, in windowed and
  fullscreen, with `Immediate` and with `AutoNoVsync`. The 4.08 ms in
  [benchmark/2026-09-06-adf](../benchmark/2026-09-06-adf.md) was measured before
  `display.rs` existed, so **the "Own the window" commit is the prime suspect and
  the bench has been blind ever since.**

**Do not quote a stored number as the other half of a comparison.** An A/B is
one command, both halves built from the same tree, or it is not evidence.

## The truth probe measured the wrong truth

2026-09-08. `how_far_the_bake_strays_from_the_truth` reported zero overestimate
for days while Flori kept pointing at faceted spheres and cones. Both were right:
the probe compares the bake against **the same triangle soup the bake was given**,
so it is blind to the error between that soup and the shape it stands for.

The generated world hands the baker `Sphere::mesh().ico(3)`, `Cylinder::…
.resolution(20)` and a default-resolution `ConicalFrustum`. At the scale these
render, that is facets of **0.38 to 1.8 m against a 0.22 m voxel** — the mesh is
two to eight times coarser than the field sampling it. The flat panels on a
cylinder's rim are twenty of them, because the mesh has twenty sides.

**A ground-truth probe is only as good as its notion of ground truth.** Ours
proved the bake faithful and said nothing about whether the input deserved
faith.

## Rebuilding the bake around a sign-change shell, and reverting it

2026-09-08. Mike Turitzin's SDF engine video describes the same brick-map
architecture with two differences worth copying: a band of **half a voxel
diagonal** rather than four voxels, and bricks allocated only where the surface
**actually crosses** rather than anywhere near a triangle. Both were built.

**It did not work, and the whole round was reverted.** What was learned:

- **A sign-change test needs correct signs across the whole brick**, and the bin
  only guarantees them within the band. With a 3-voxel reach and 8-voxel bricks,
  interior nodes pick a wrong nearest triangle, the sign comes out wrong, and the
  brick is dropped. The terrain came out as scattered patches.
- Replacing the test with an **exact triangle/box overlap** (SAT) fixed that and
  gave a genuinely thinner shell: at the same brick budget the retry loop landed
  on **0.146 m voxels instead of 0.220** — the resolution win we were after.
- But the empty-space bound collapsed with it. `coarse + range` was sound only
  because "unallocated" used to mean "no triangle within range". Re-seeding the
  chamfer from *near* bricks restored soundness, and the render came back.
- The end state was still **2x slower** (8.4-9.9 ms against 4.1) and failed five
  soundness tests, including `the_bake_never_overestimates_the_distance` with
  overestimates of 3.5-4.4 voxels. Unbiased storage plus threshold-free hits was
  not sound at the resolutions this engine actually runs at.

**The rule this cost a session to relearn:** the vault already says *"when a
change makes things worse, check whether the first version was already
correct."* Four separate things changed at once — band width, allocation rule,
stored bias, hit criterion — and no single one could be measured. Change one, run
the truth probe, keep or drop it, then change the next.

## The clamp is load-bearing, and removing it walks rays through walls

2026-09-07. The `range` clamp makes every value beyond four voxels a flat
plateau, which caps the march step and is the reason a grazing ray crawls. The
obvious fix — store the true distance with a per-brick min/max window — made the
picture **worse**, and the coarse-voxel overlap test said why: a brick's triangle
list only contains triangles within the bin's reach, so a distance computed from
it is only correct **inside that reach**. Beyond it the nearest triangle may not
be in the list, and the value comes out **too large**. 0.61 m too large, at a
point one brick above a box.

An overestimate is the one error sphere tracing cannot survive. The clamp was
never a precision compromise; it is what makes the incomplete list safe.

**The accurate band can never be wider than the bin's reach.** Widening the reach
is the only honest way to lengthen steps, and it costs bricks and bake linearly.

## Half a voxel of bias is not enough, and a crease proves it

Same day. Every stored value carries `-0.866 * voxel` so trilinear interpolation
can never overestimate. That inflates every surface by 0.87 voxel — 19 cm at the
0.22 m voxels a 200 m world gets — which rounds concave creases and merges
anything closer together than twice that. Tempting to cut.

Measured against brute force on a 3000-triangle world, a bias of **0.25 voxel**
left a worst overestimate of only 0.09 voxel, and **0.5 voxel** left 0.01. Both
looked safe. Both are not: the cylinder-in-a-box test, which samples 200000
points and therefore actually hits a **cap edge**, fails at 0.5 with a 2.3 voxel
overestimate. Half a voxel diagonal is the worst case *at a crease*, and creases
are exactly where a random probe rarely lands.

**0.866 stands.** 0.87 voxel of surface inflation is the price of soundness, and
the only way to spend less of it in metres is smaller voxels.

## Shredded geometry was geometry smaller than a voxel

2026-09-07. Objects came out as interpenetrating shells of light and dark
fragments wherever two shapes met — reported as shapes "glitching through
themselves". Two real bake bugs were found and fixed while chasing it (buried
shells, below), and **neither made any difference to the picture.**

What answered it, again, was resolution: the same camera at 0.220 m and 0.098 m
voxels. At the finer bake the shredded blob is a speck. The generator was
authoring structures down to about **2 voxels across** — a cylinder of 0.56 m
radius baked at 0.22 m voxels, with a 0.19 m bake bias on top. Nothing can
represent that.

Two things came out of it that keep it from recurring: the bake now **warns when
any part is thinner than four voxels**, and the generated world no longer
authors anything that small.

**Both times a shape artefact has been chased this month it was resolution, and
both times the A/B that proved it took one run.** Render it twice before
theorising.

## A brick can be inside a shape whose triangles it never sees

Found by the test written to chase the shredding, and real even though it was not
the cause. A brick near shape B's surface but deep inside shape A holds only B's
triangles — A is farther than the bin's reach — so the union came out **positive
where it should have been solid**, and B's buried shell was baked as a real
surface inside A.

`buried_bricks` now runs a per-part flood at brick resolution, over each part's
own inflated AABB, and any brick that is inside a part absent from its own list
is left unallocated. `seal_interior` then marks it solid, because nothing outside
can reach it. On the test world that removed 20% of the bricks, so the bake got
faster as well as correct.

The proof it earns its place is in `the_two_parts_must_be_told_apart`: with the
part split, a point inside both shapes reads -1.428 against an exact -1.44; as
one triangle soup it reads +0.033.

## The staircase at a crease is the voxel, not a bug

2026-09-07. Cone bases met the ground in a scalloped skirt and the terrain lip
carried a sawtooth, both described as shapes "glitching through themselves". Two
plausible causes were fixed on suspicion — the per-part sign and the union rule —
and **neither changed the picture at all.**

What answered it was rendering the same camera twice, at 0.165 m and 0.110 m
voxels: the scallops vanish. The artefact scales with the voxel, so it is
trilinear reconstruction of a **crease** — the surfaces are smooth, only their
intersection line is not, and a sampled field resolves a crease at its own
resolution. `--bricks` is the knob; there is nothing to fix in the logic.

**Two speculative fixes went in before one measurement.** Both were improvements
on their own merits and both were kept, but the picture never moved, and the
A/B that settled it took one run.

## A magnitude taken from the wrong part

Same day, and the reason one of those fixes was wrong to begin with. The first
per-part union computed **sign** from "is the point inside any part" and
**magnitude** from "distance to the nearest triangle of any part". At a point
outside the terrain but inside a cone sunk into it, the nearest triangle is the
buried terrain, so the magnitude collapses to zero *there* — planting an
isosurface inside the union.

A union of solids is `min` of the signed distances, and nothing else. Splitting
sign from magnitude reads as an optimisation and is a different function.

## Over-relaxation eats holes in a field made of bounds

2026-09-06. Silhouettes against the sky were chewed with single-pixel holes,
worst where the terrain grazed the view. The suspects were the step budget and
the normal error; both were wrong.

`--omega 1.0` in one run made them vanish completely. Keinert over-relaxation
steps `distance * 1.2` and rewinds by `step * (1 - omega)` when two consecutive
unbounding spheres fail to overlap. The proof holds for *true* distances. This
field returns **bounds** — an empty-brick clearance, a texel clamped at the band
edge — and a 20% rewind is then not guaranteed to land before a surface the ray
has already jumped, so a grazing ray sails past a ridge and reports a miss.

**`OMEGA` is 1.0 by default now.** The flag stays. The empty-brick clearance skip
already does the long-range work relaxation used to do, and the step heatmap
after widening the band showed nothing near the 128-step budget.

The step heatmap (`--debug-view`, added the same day) is what ruled out the
budget in one run: nothing was red.

## Contact resolved against a bound leaves bodies hovering

Same day. Spheres never came to rest: `resting false` forever, speeds of several
metres per second, and normals like `(0.016, 0.9997, 0.016)` and
`(-0.248, -0.248, 0.936)` — components repeating with equal magnitudes, the
signature of a tetrahedron gradient taken on a brick bound rather than a surface.
Several pointed *downward*, so the push-out drove bodies into the ground.

A dropped sphere of radius 0.19 settled 0.62 above a slab, exactly where the
bound crossed its radius. The bound degrades to `range` at brick walls, where the
inscribed term goes to zero, so contact fired three bricks early.

**Contact now requires a banded sample.** `Adf::probe` returns the same
`(distance, banded)` pair the shader's `adf_probe` does, and penetration is only
resolved when the sample came from the encoded band — where the value is a real
distance and the normal is a real normal. Because a bound never *over*estimates,
`bound >= radius` already proves no contact, and `bound < radius < range` can only
happen inside an allocated brick. The gate is exact, not heuristic.

The same sphere now rests at `radius + half a voxel diagonal` — the bake bias,
predicted to within a voxel, which is what the test asserts.

**Third and fourth venue for "a bound is not a distance" in two days**, after the
shadow penumbra, the empty-brick skip and sweeping by `distance - radius`.

## A held button is not a button press

The jump used `held && since_jump_pressed >= JUMP_BUFFER` to start the timer, so
holding the key re-armed the buffer every 0.15 s and the character auto-hopped.
The reference reads `context.started`, a rising edge. `Character` remembers
`was_held` now.

Caught by a test asserting the character is back at ride height after a jump —
it was still mid-hop.

## An empty frame that was the camera standing inside a hill

2026-09-06, landing the character controller. `--play --size 120` rendered the
character and a physics ball floating in the clear colour: no world at all. The
character was suspected first, and its position was logged for a round — it was
standing exactly where the terrain said it should.

The camera was inside the hillside behind it. A ray starting inside geometry
samples a **negative** distance, `distance < close_enough` accepts it as a hit,
and `travelled += distance` moved the surface point *behind the eye*. Negative
depth, everything clipped, nothing drawn — and no `discard`, so not even a
recognisable failure.

Two fixes, both worth having: `ray_march` clamps a hit to `max(distance, 0)`, and
the follow camera sweeps out to its orbit position and stops short of geometry.

**The tell was that the character was fine.** When two things are new and one of
them is verifiably correct, the bug is in the other one — logging the character
for a round was the cost of not checking that first.

## A bound minus a radius is not room to move

Same day. Sweeping the character by `distance - radius` froze it in mid-air at
the exact height where `distance == radius`, spring still pushing, travelling
zero. The field returns a *bound* away from surfaces; near geometry that bound is
as small as `RANGE_VOXELS`, well under a half-metre radius, so the subtraction
went to zero long before anything was actually in the way.

Sphere tracing consumes a conservative bound by **iterating** it, never by
subtracting from it once. `swept` advances repeatedly and the radius is left to
the penetration push-out.

Third venue for the same lesson in one day, after the shadow penumbra and the
empty-brick skip. **Every time this field hands back a number, ask whether it is
a distance or a bound before doing arithmetic with it.**

## A plane in the sky, from encoding "miss" as a magic distance

2026-09-06, first 1 km bake. A huge pale plane hung above the horizon. Nothing
in the scene was up there.

`ray_march` returned `stop_distance` on a miss and the fragment tested
`travelled >= span.y` to decide whether to `discard`. For a ray that misses the
volume entirely, `adf_span` returns `exit < entry`, and the march was skipped —
leaving `travelled` at the sentinel `MAX_MARCH_DISTANCE`, which was **1000.0**.
A 1037 m world produces exits larger than that, so `1000 >= span.y` was false,
the discard never happened, and the shader shaded a point a kilometre down a ray
that had hit nothing.

It cannot happen in a world smaller than the sentinel, which is why 20 m and
316 m were both clean.

**The march now returns an explicit hit flag.** A miss is a boolean, never a
distance that happens to be large. `MAX_MARCH_DISTANCE` survives only as the
"infinity" a directional light is at.

## A bound is not a distance, and shading one looks like noise

2026-09-06. The baked map rendered with brick-shaped patches and fine speckle
over every shadowed surface. It was read as normal error — the reconstruction
had a measured 2.9° of it, so there was a plausible culprit already on file, and
two changes went in chasing it.

`--shadow-steps 0` settled it in one run: with the shadow march switched off the
image was **pristine**. Nothing about the bake, the normals or the quantiser was
ever wrong.

The real cause: `shadow_factor` fed `adf_distance` into the IQ penumbra term,
and the baked field returns a *bound* wherever it has no encoded value. A bound
undershoots, so it darkens pixels that should be lit, and it is piecewise
constant per brick, so it does so in brick-shaped blocks.

**When an artefact has the geometry of the acceleration structure rather than
the geometry of the scene, suspect the structure's bounds, not the field.** And
when a plausible culprit is already on file, that is a reason to run the cheap
A/B, not a reason to skip it.

## Three rounds of shader tuning against a bug the CPU could have named

2026-09-06, landing the ADF. The baked torus rendered with speckled shading on
the terminator. Three changes went in, each followed by a full run of the app:
the normal tap was widened, the encoded distance range was halved and switched
from truncation to rounding, and the shadow bias was scaled with the voxel. Two
of the three made the image *worse* or did nothing.

What answered it was a **CPU diagnostic that never touches the GPU**: bake a
torus, sample `Adf::normal` at 2000 surface points, compare to the analytic
torus normal, print the mean angle. One `cargo test`, ten seconds. It said 2.9°,
and that a mesh with four times the triangles does not improve it — so the error
is the trilinear reconstruction, not the tap, not the quantiser and not the
shadow.

**Both evaluators exist so that one of them can be interrogated cheaply. Use the
CPU one before changing the shader.**

The wider tap was reverted; rounding and the scaled bias were kept because they
are correct, not because they fixed anything.

## A result that lands exactly on the known floor

The first ADF bench read **2.62 ms median** where the analytic renderer read
10.19. It was also, to two decimals, the empty-scene floor this file already
records — the precise shape of the trap that cost five hours on 2026-09-06. It
was believed only after opening the screenshot and seeing a torus.

The honest reading is still not "the field is free": the torus covers about an
eighth of the screen, so the marching is smaller than the noise on the floor
**at that coverage**. A scene that fills the frame has not been measured.

## Tests that froze a constant

Changing `RANGE_VOXELS` from 4 to 2 broke two ADF tests that had nothing wrong
with them — they compared against `brick_size()` and probed a point 12 voxels
above a face, both of which silently meant "inside the encoded band" when the
band was four voxels wide. Neither failure was a bug in the bake.

This file already carries the rule from the brush era: **a test that encodes a
guess is worse than no test.** A test that encodes a *tunable* is the same
mistake with a longer fuse. Both now ask the field for its own range.

## An empty brick has no sign

The first bake gave the centre of a solid cube a distance of **+0.25**. Empty
bricks returned a positive skip bound, and the interior of a closed model is
made entirely of empty bricks. Invisible in the render — a ray hits the shell
long before — and wrong for every physics query.

Fixed with a flood fill from the volume border over empty bricks: what the
outside cannot reach is `SOLID` and returns the negative of the same bound. The
cube test caught it on the first run, which is the only reason it was not
shipped.

## Two blank frames agree perfectly

2026-09-06, landing the grid window. `scene.wgsl` referenced
`render_params.grid_indexed` while `bindings.wgsl` still called that member
`grid_padding` - an earlier rename had been reverted when the work was
descoped. An undefined struct member means no pipeline and nothing drawn.

It was not caught, because the check was **grid render vs `--no-grid` render,
compared pixel by pixel**. Both were empty, so the diff read **0.000% of pixels
differ** and was recorded as proof of correctness. This file already carries the
same lesson from the grid tests in August: *a miss agrees with a miss*.

The bench said so plainly and was misread too: `spread:80` cost 1.905 ms against
an `empty` floor of 1.928. A four-times speedup was briefly believed before the
floor was checked.

**Every image comparison now also asserts the image is not blank** - count the
distinct colours before trusting a diff. Two frames agreeing is only evidence
when at least one of them is known to contain something.

## Five hours lost to a screenshot taken 60 frames too early

2026-09-06. Taking dynamic bodies out of the acceleration grid needed a fold
after the cell-wall clamp in `scene_distance_gridded`. Every edit there rendered
an empty frame — including edits that were **provably no-ops**, such as turning
`return min(field, x);` into `field = min(field, x); return field;` where `field`
is a `var`. It reproduced on pristine HEAD with no Rust changes, deterministically,
across all four scenes, with no compile error at any log level.

It was written up as an unexplained toolchain anomaly and declared blocking.
**That was wrong.**

`SETTLE_FRAMES` was 240. A shader edit misses naga_oil's cache, the pipeline
recompiles, and Bevy draws nothing for that entity until it finishes — about
half a second, against 240 frames of a ~2 ms empty scene. The screenshot fired
just before the pipeline went live. A **comment** edit still hit the cache, so
it rendered, which is what made the failure look semantic rather than temporal.
Threshold measured afterwards: it fails at 240 and passes at 300.

**Both harnesses now settle on wall-clock, not frames** (`dev::SHADER_WARMUP`,
3 seconds), because compilation is wall-clock work and a frame count silently
rescales with frame time. Verified against a forced cache miss: the screenshot
renders, and a cold-cache `bench --repeat 3` now has run 1 at 3.354 ms against
3.441 and 3.314 — it used to read an empty screen as a plausible median.

### The five wrong turns, all instrumentation, none the bug

- **A screenshot was judged by `stat -c%s`.** A uniform grey frame and a uniform
  red one compress to the same size, so a probe that painted missed rays red
  read as "still broken" when it was likely working. **Open the image.**
- **The run helper wrote to one filename with output sent to `/dev/null`**, so a
  crashed run would have re-read the previous run's file and reported it as a
  result.
- **`--debug-view 1` was passed as a flag for two rounds.** No such flag exists;
  the debug view is a key toggle. It produced a confident wrong conclusion
  ("no pipeline"), which then survived several rounds as an assumption.
- **`run_system_once` was used to test Bevy change detection.** It builds a
  fresh system per call, so its last-run tick is always zero and everything
  reads as changed forever. Change detection can only be tested through a
  `Schedule` that persists between runs.
- **The measuring instrument was never itself suspected.** Five hypotheses about
  the shader were tested and discarded before the harness was questioned once.
  The clue was there the whole time: this file already recorded two occasions
  where a run measured an empty screen, and `debt/open-threads` carried the exact
  fix as an open item.

**The rule: when a change that cannot matter appears to matter, stop testing the
change and start testing the instrument.**

## Measurement

- **Measured against a capped clock.** `AutoNoVsync` fell back to Fifo, every
  fast median read 16.6 ms, and the conclusion "the cone pass costs 16%" was the
  exact opposite of the truth. Two conclusions were drawn from it before anyone
  noticed. Identical medians across unrelated scenes are an artefact until proven
  otherwise. `bench_window` asks for `Immediate` now and warns within 0.2 ms of
  60 Hz.
- **A bench run can measure nothing.** 600 frames of an unshaded quad print as a
  normal-looking 2.26 ms median. Twice on 2026-09-01, again on 2026-09-02 where
  run 1 read 2.482 and runs 2-3 read 12.0 and 13.0. The tell is `p95` sitting on
  the median with no warm-up spike. Not guarded.
- **Run 1 is not a measurement.** Inside one process the medians walk down as the
  GPU boosts: 10.4, 9.9, 9.8, and `p95` walks down harder. `--repeat 4`, read run
  3 or later.
- **One block is not a measurement either.** The shadow proxy first read 16.7 and
  was nearly rejected as 6 ms worse; interleaved A/B/A it was 11.3, twice, both A
  blocks agreeing to 0.05. The tell in the bad run was its *own* A block spreading
  2 ms across runs 3 and 4.
- **Cross-session numbers are not comparable.** The same binary read 23.8, 25.1,
  22.5 on `spread:80` in one session — 12% of drift with nothing changed. A
  before/after taken an hour apart is unreadable. Over-relaxation was nearly
  rejected because a fresh 75.98 was compared to an hour-old 64.171.
- **Contention is invisible in the median.** With 3-9% GPU load from other
  processes the medians swing bimodally on identical runs and both "47% faster"
  and "12% slower" came out of the same build. Read the `min`, or re-measure on a
  quiet machine.
- **Running the exe directly is not the same as `cargo run`.** Bevy resolves
  `assets/` next to the executable, logs one ERROR, renders nothing. The first
  measurement of 2026-09-01 was taken that way.
- **A screenshot is not a measurement.** Comparing 20-shape grids by eye across
  two projections produced three confident wrong answers in a row, and a wordless
  screenshot was once read as approval. What fixed it: render each shape in
  isolation with its parameters labelled, and probe the field numerically.

## Tests that passed against live bugs

- **A test satisfied by both sides failing is not a test.** Three versions of the
  grid march test were green against live bugs because the exact march and the
  gridded march both missed, and a miss agrees with a miss. The fixed version
  aims at a target the exact march *must* reach, and asserts that separately.
- **A conservative value can still be useless.** The grid's soundness test was
  green while the screen was blank, then striped, then missing a slice.
- **A test that encodes a guess is worse than no test.** Assertions have twice
  been deleted for freezing an assumption: a solid floor under a shell, a
  constant wall up a taper.
- **Uniform test shapes hide non-uniform bugs.** A wall measured against the
  wrong axis passed every cube test and failed on the first plate.
- **Verify the test fails on the bug.** Every guard in the suite was checked by
  breaking the thing it guards. Several passed at first and were rewritten.
- **Test both ends of a range.** `thickness = 0` collided with the solid
  sentinel and nothing covered either extreme.

## Silent renders — no error, nothing drawn

- **A shader import that never loads stalls the pipeline forever.** No error, no
  warning, full frame rate, clear colour. Cost most of the shader split. See
  [reference/bevy-0-19](../reference/bevy-0-19.md#a-shader-import-that-never-loads-stalls-the-pipeline-silently).
- **A second `Camera3d` breaks every `Single<..., With<Camera3d>>`.** `Single`
  matches nothing when two entities qualify, so `fit_quad`, `park_camera`,
  `fly_camera` and `update_stats` went dead at once and the quad was never
  fitted. There is a `MainCamera` marker now. **Adding a camera is a breaking
  change to every `Single` camera query in the crate.**
- **Resizing or reallocating a GPU resource invalidates the bind group.**
  `buffer.set_data()` with a different element count, and `image.resize()`, both
  leave the material pointing at the old resource: first a scene invisible until
  the window was resized, later a blank coarse pass. Fixed capacity, always
  upload full, pass the real count separately; create images once at the size
  they will keep.
- **A window does not always get the size it asked for.** A 1920x1080 window does
  not fit on a 1920x1080 monitor, the manager shrinks it, and anything sized from
  the *requested* resolution has the wrong aspect. Size from
  `Camera::physical_target_size` and index by UV.
- **Shipped a shader that had never drawn a frame.** The grid landed with three
  rendering bugs behind a green test — blank window, stripes, missing slice.
- **A large block replacement dropped `surface_normal`** and shipped, failing at
  pipeline compile. An identifier audit after every large WGSL edit is the
  fallback; running the app is the gate.

## Field and renderer

- **A geometric bound is not a bound on the evaluator.** The box reject used the
  shape's real AABB — correct geometry, wrong for an estimate that deliberately
  undershoots. Caught by the first parity run, 5.529 against 4.758. `cull_scale`
  exists for that.
- **Out of budget is a miss, not a surface.** The march returned wherever it
  stopped and the fragment stage shaded mid-air. Drew as banding across the whole
  image, and the grid made it common.
- **Outside the grid there is no cell to clamp to**, and **pancake cells** let
  the thinnest axis set every ray's step. A blank window and a missing slice. See
  [reference/engine-field](../reference/engine-field.md).
- **An interpolated value bounds nothing.** Writing a march result into a vertex
  varying hands it to neighbouring pixels: a `MAX_MARCH_DISTANCE` miss-park cut
  notches along silhouettes, then a meaningless travel distance dented them, then
  cell-wall distances filled the screen with stripes. Three of the worst
  rendering bugs in the project, one property, one feature. See
  [decision/cone-marching](../decision/cone-marching.md).
- **10x slowdown on one scene.** `distance_correction = min(scale)` multiplied
  every march step and thin slabs made it tiny. Scale is baked into the half
  extents now: 22.7 to 12.1 ms.
- **Two of the nine blend modes fold the whole field.** The museum scene first
  rendered almost nothing: `intersect` is `max(shape, field)`, and far away the
  shape is large and positive, so it *is* the result. See
  [decision/single-brush](../decision/single-brush.md).
- **Assumed the cost was where the work was.** Lights cost 14 ms with no casters
  and no marching — register pressure from a second evaluator inlined into the
  shadow march. The arithmetic said the loop body could not cost that, and doing
  the arithmetic was worth more than the guessing.

## Modifier semantics

- **Thickness took four rounds**: wrong magnitude, wrong basis axis, an invented
  floor, an invented top opening — and finally the real bug, `thickness = 0`
  colliding with the sentinel for solid. A zero-thickness shell is a real
  setting, not a sentinel. Along the way a *correct* mapping was replaced with a
  wrong one because plates looked solid for an unrelated reason.
- **Blend calibration, three wrong turns.** Assumed file units, then assumed
  relative to shape size, then tried to solve algebraically from two dial
  readings — underdetermined. What worked: a controlled scene of geometrically
  similar pairs, read off the editor rather than off a screenshot of the game.
- **Modifier semantics guessed from a name are usually wrong.** `round` and
  `bevel` meant different things in the two authoring tools this field was
  reverse-engineered from.
- **The reversal worth remembering.** Flori: *"They were correct at the start."*
  The original shell was right, and three rounds went into replacing it with a
  cup that never existed. When a change makes things worse, check whether the
  first version was already correct before tuning the second.

## Process

- **A central list of tunables rots against the code it configures.** A
  `settings` resource was planned; by the time it was built two of its constants
  had died with cone marching and three others were already runtime flags. Every
  module reads its own flag now, and the default stays a `const` beside it.
- **A whole ponytail audit produced zero applicable findings.** Each either
  hard-coded something or blocked a future modifier. Worth remembering before
  running another one here.
- **Two compile errors shipped in one message.** That is why `cargo test` runs
  before every delivery.
