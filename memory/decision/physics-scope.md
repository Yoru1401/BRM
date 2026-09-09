---
id: decision/physics-scope
type: decision
title: Rigidbodies only — softbodies removed
created: 2026-08-27
updated: 2026-08-27
tags: [physics, scope]
links: [reference/physics]
---

# Softbodies: built, then removed

Softbodies were in the original three-part plan. They were implemented, and then
Flori said to remove them. The revert was clean — no traces, no orphaned
constants, tests back to their previous count.

Recorded because the project brief still reads "rigidbodies, softbodies and
static objects", and a future session should not resurrect them from that
sentence alone. **Ask before rebuilding softbodies.**

## 2026-09-06: a character controller, still no solver

`--play` adds a floating-capsule character controller ported from Joe Binns's
stylised controller. It is **not** a rigidbody in the existing sense: it has its
own integration, it is not part of the field, and nothing can stand on it.

That is deliberate. The whole appeal of the floating-capsule approach is that it
needs no contact solver — four springs and a downward probe, which the baked
field answers for free. Adding it did not require a physics engine and does not
argue for one.

Reopen this if two characters ever have to push each other.
