---
id: reference/physics
type: reference
title: The physics world
created: 2026-08-27
updated: 2026-09-06
tags: [physics, rigidbody]
links: [reference/engine-field, decision/physics-scope]
---

# Physics

Sphere rigidbodies against the baked field. No separate collision geometry —
bodies call `Adf::distance`, the CPU mirror of what the shader samples, so the
collision surface and the drawn surface are the same data by construction.

**Bodies are ordinary Bevy meshes now.** They were brushes in the field until
2026-09-06; a baked static field cannot hold a moving object, and the fragment
stage writes real depth, so a stock `Mesh3d` sphere sorts correctly against the
marched geometry for free. Dynamics therefore cost the renderer nothing and
force no upload — which deletes the whole per-frame rebuild the old field paid
0.8 ms of CPU for.

## Why it is shaped this way

- **Gated by `static_field_is_ready`.** The model loads asynchronously and the
  bake takes seconds; without the gate, gravity runs against a default field
  that is empty everywhere and drops every body through the world.
- **The field a body reads is a bound, not a distance, outside the encoded
  band.** Two voxels out it clamps, and in an `EMPTY` brick it is the
  brick-skip bound. Both are conservative in the safe direction — clearance is
  understated, so a body stops early rather than late — but do not read a large
  clearance as a real one.
- **Deep interiors read `SOLID`**, a negative bound rather than a real
  penetration depth, so a body that ends up well inside geometry is pushed out
  gently rather than teleported.
- **Sleeping is not an optimisation of the solver.** Resting bodies jitter
  forever; with the field baked, that no longer costs an upload, only a solve.
- **`KILL_BELOW` despawns fallen bodies.** It mattered more when a falling body
  inflated the scene AABB and collapsed the grid; the baked volume is sized from
  the model alone, so this is now only housekeeping.

## Motion is swept, never stepped

`swept` sphere-traces the frame's travel through the field instead of adding it
to the position. Advancing repeatedly by `Adf::distance` — a proven underestimate
— means a body's **centre can never cross a surface**, whatever its speed, and
the penetration push-out only ever has to undo a fraction of a radius. Bodies
also take `SUBSTEPS` (4) integration steps per `FixedUpdate` tick.

**Sweeping by `distance - radius` does not work here and was tried first.** The
field returns a *bound* away from surfaces, and near geometry that bound can be
as small as `RANGE_VOXELS`, which is far below a character's radius. The result
was a character frozen in mid-air at the exact point where `distance == radius`,
with the spring still pushing. The bound has to be consumed by *iterating*, not
by subtracting from it once. Same lesson as the shadow penumbra, third venue —
see [history/wrong-turns](../history/wrong-turns.md).

## Contact needs a banded sample and a body no wider than the band

Two rules, both learned the hard way:

- **Resolve penetration only when `Adf::probe` says the sample is banded.**
  Outside the band the value is a bound and the normal is the gradient of a
  bound; bodies hover metres up and get pushed sideways or down.
- **Cap the radius at `range * 0.9`.** A body wider than the encoded band can
  never reach a banded sample before touching, so the gate above would starve it.
  `game/scene.rs` sizes bodies as `min(reach * 0.012, range * 0.9)`, which means
  **body size is limited by bake resolution** — a 1 km world at 1.1 m voxels
  cannot have metre-scale physics. That is the clipmap's problem, not a bug.

A dropped sphere rests at `radius + half a voxel diagonal` above the surface.
The extra term is the bake bias, and `a_dropped_sphere_rests_one_radius_above_the_slab`
asserts it to within a voxel — a prediction, not a recorded value.

## The floating character controller

`game/character.rs`, behind `--play`. A port of Joe Binns's stylised controller,
itself the Toyful Games *Very Very Valet* floating-capsule approach, onto the
baked field. Four springs and no ground collider:

- **Ride height.** `ground_below` sphere-traces down; while the trace is inside
  `PROBE_REACH`, a spring holds the capsule `RIDE_HEIGHT` above what it found:
  `spring = offset * 50 - closing * 5`, applied as `-gravity + down * spring`, so
  gravity is cancelled exactly while grounded. Steps and slopes are smoothed for
  free because nothing ever touches the ground.
- **Upright torque.** Shortest-arc quaternion from the current orientation to the
  target, as an angular spring `axis * angle * 40 - angular_velocity * 5`. The
  target is the yaw of the goal velocity, tilted into the acceleration by
  `LEAN`, which is what produces the lean and the wobble on stopping.
- **Movement.** A goal velocity ramped toward `input * MAX_SPEED` at
  `ACCELERATION * factor`, then a force of `(goal - velocity) / dt` clamped to
  `MAX_ACCEL_FORCE * factor`, with the vertical component removed. `factor`
  interpolates `TURN_FACTOR` (2.0, reversing) to `ALIGNED_FACTOR` (1.0, already
  moving that way) — the reference uses an `AnimationCurve` here whose keyframes
  are not in the published project, so this is a straight line of the same shape.
- **Jump.** Impulse 10 m/s with a 0.15 s input buffer and 0.25 s of coyote time.
  The ride spring is switched **off** on the way up, or it would fight the jump.
  Gravity is then scaled: `RISE_GRAVITY` 5x while rising, `FALL_GRAVITY` 10x
  falling, plus `LOW_JUMP` 2.5x while rising with the button released. Holding
  jump therefore reaches about 1.3x the height of a tap.

Three things about the port that are not in the reference:

- **The ground probe is a sphere trace, not a raycast.** The field gives it for
  free, and it costs one `Adf::distance` per iteration.
- **The camera collides.** `follow_camera` sweeps from the character toward the
  orbit position and stops short. Without it the camera ends up inside terrain,
  where the marcher hits at a negative distance and the frame comes out empty.
- **The capsule is in the field.** It is a `Dynamic`, folded by `min` after the
  baked lookup, so it is ray-marched, lit and shadowed like everything else — no
  Bevy mesh. It still does not collide with the physics bodies, because the CPU
  field is the baked one only.
- **Jump fires on the rising edge**, tracked in `was_held`. Reading the button as
  a level made holding it auto-hop every `JUMP_BUFFER`.
- **Not built:** squash-and-stretch, platform-relative motion, the dust
  particles.

## Friction

`contact_friction` is pure and tested. Contact arm is `-normal * radius`, slip is
the tangential contact velocity:

```
magnitude = min(2/7 * |slip|, mu * normal_impulse)
```

`2/7` is the tangential effective mass of a solid sphere; the Coulomb cap is what
makes a weak contact slide instead of gripping. Angular velocity gains
`arm x impulse / (0.4 r²)`. Three regimes are pinned: sliding turns into spin,
rolling without slipping is left alone, a weak contact is capped.

Body-body (`sphere_pair_correction`) is O(n²) pairs, fine at four bodies. When it
stops being fine it wants a broadphase of its own — the renderer no longer builds
one to borrow.
