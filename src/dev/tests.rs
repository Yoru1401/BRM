#[cfg(test)]
mod sdf_tests {
    use crate::game::physics::*;
    use crate::game::world::*;
    use crate::sdf::field::*;
    use bevy::prelude::*;

    // ---------------------------------------------------------------- helpers

    fn placed(shape: SdfShape, placement: Transform, operation: CsgOperation) -> GpuShape {
        shape.to_gpu(
            &GlobalTransform::from(placement),
            None,
            Some(&operation),
            None,
        )
    }

    fn shaped(shape: SdfShape, placement: Transform, modifiers: Modifiers) -> GpuShape {
        shape.to_gpu(
            &GlobalTransform::from(placement),
            Some(&modifiers),
            None,
            None,
        )
    }

    fn union(radius: f32) -> CsgOperation {
        CsgOperation {
            radius,
            ..default()
        }
    }

    /// Mirrors `ray_march` in sdf.wgsl. `omega` of 1.0 is plain sphere tracing;
    /// `field` is whatever evaluator is under test, exact or gridded.
    fn march(
        field: &dyn Fn(Vec3) -> f32,
        confirm: &dyn Fn(Vec3) -> f32,
        origin: Vec3,
        direction: Vec3,
        omega: f32,
        threshold: f32,
        budget: u32,
    ) -> (f32, u32) {
        const STOP: f32 = 60.0;

        let mut travelled = 0.0;
        let mut steps = 0;
        let mut relaxation = omega.max(1.0);
        let mut previous_distance = 0.0;
        let mut step_length = 0.0;

        while steps < budget {
            let distance = field(origin + direction * travelled);
            let overshot = relaxation > 1.0 && (distance.abs() + previous_distance) < step_length;
            steps += 1;

            // A small distance is not proof of a surface: a grid-clamped field
            // reports the cell wall, not the geometry. Confirm before stopping.
            if !overshot
                && distance < threshold
                && confirm(origin + direction * travelled) < threshold
            {
                return (travelled + distance, steps);
            }
            if overshot {
                step_length *= 1.0 - relaxation;
                relaxation = 1.0;
            } else {
                step_length = distance * relaxation;
            }
            previous_distance = distance.abs();
            travelled += step_length;
            if travelled >= STOP {
                break;
            }
        }
        // Out of budget is a miss. Anything else shades a point in mid-air.
        (STOP, steps)
    }

    // -------------------------------------------------------------- authoring

    /// The authored scene has to survive spawning, not just compile. Without a
    /// `Transform` on the root, propagation never reaches the children and every
    /// shape packs at the origin, which draws as the whole world stacked in a
    /// heap and is invisible in any test that only checks one brush.
    #[test]
    fn authored_shapes_land_where_they_were_written() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::scene::ScenePlugin,
            TransformPlugin,
        ));
        app.world_mut().spawn_scene(world_scene()).unwrap();
        app.update();

        let mut placed: Vec<Vec3> = app
            .world_mut()
            .query_filtered::<&GlobalTransform, With<SdfShape>>()
            .iter(app.world())
            .map(|placement| placement.translation())
            .collect();
        assert!(
            placed.len() >= 8,
            "expected the authored brushes, got {}",
            placed.len()
        );

        placed.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).unwrap());
        placed.dedup_by(|a, b| a.distance(*b) < 1e-4);
        assert_eq!(
            placed.len(),
            app.world_mut()
                .query_filtered::<Entity, With<SdfShape>>()
                .iter(app.world())
                .count(),
            "every brush should sit where it was written, not stacked at the origin"
        );
    }

    // ----------------------------------------------- primitives and modifiers

    #[test]
    fn sphere_matches_closed_form() {
        let scene = [placed(SdfShape::Sphere, Transform::IDENTITY, union(0.0))];
        assert!((scene_distance(&scene, Vec3::new(3.0, 0.0, 0.0)) - 2.0).abs() < 1e-5);
        assert!((scene_distance(&scene, Vec3::ZERO) + 1.0).abs() < 1e-5);
    }

    #[test]
    fn box_matches_closed_form_outside_face_edge_and_inside() {
        let scene = [placed(SdfShape::Cube, Transform::IDENTITY, union(0.0))];
        // Straight out from a face.
        assert!((scene_distance(&scene, Vec3::new(3.0, 0.0, 0.0)) - 2.0).abs() < 1e-5);
        // Diagonally past an edge: sqrt(2) from the corner of the cross section.
        let diagonal = scene_distance(&scene, Vec3::new(2.0, 2.0, 0.0));
        assert!((diagonal - 2f32.sqrt()).abs() < 1e-5);
        // Dead centre is one half-extent inside.
        assert!((scene_distance(&scene, Vec3::ZERO) + 1.0).abs() < 1e-5);
    }

    #[test]
    fn uniform_scale_scales_the_distance() {
        let scene = [placed(
            SdfShape::Sphere,
            Transform::from_scale(Vec3::splat(2.0)),
            union(0.0),
        )];
        // Radius becomes 2, so a point 5 out is 3 away.
        assert!((scene_distance(&scene, Vec3::new(5.0, 0.0, 0.0)) - 3.0).abs() < 1e-5);
    }

    /// The cube brush is SDF Modeler's uber primitive, so its modifiers are not
    /// separate operations layered on a box - they are arguments that reshape
    /// it. These are the corners of that parameter space.
    #[test]
    fn cube_modifiers_match_the_editors_uber_primitive() {
        let cube = |modifiers| shaped(SdfShape::Cube, Transform::IDENTITY, modifiers);

        // Untouched, it is an ordinary unit box.
        let plain = cube(Modifiers::default());
        assert!((shape_distance(&plain, Vec3::new(2.0, 0.0, 0.0)) - 1.0).abs() < 1e-4);

        // Full round works on every edge at once, so a cube becomes a sphere.
        let ball = cube(Modifiers {
            round: 1.0,
            ..default()
        });
        for probe in [
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 3.0),
            Vec3::splat(2.0),
        ] {
            let measured = shape_distance(&ball, probe);
            assert!(
                (measured - (probe.length() - 1.0)).abs() < 1e-3,
                "at {probe} expected a sphere, got {measured}"
            );
        }

        // Full bevel rounds only the four vertical edges: a cylinder.
        let bevelled = cube(Modifiers {
            bevel: 1.0,
            ..default()
        });
        let corner = shape_distance(&bevelled, Vec3::new(1.5, 0.0, 1.5));
        assert!(
            (corner - (4.5f32.sqrt() - 1.0)).abs() < 1e-4,
            "got {corner}"
        );
        // The caps stay flat, so straight up is still one unit of box.
        assert!((shape_distance(&bevelled, Vec3::new(0.0, 2.0, 0.0)) - 1.0).abs() < 1e-4);

        // Full taper closes the top to a point while the base stays put - the
        // pack pre-shrinks the half sizes exactly so this holds.
        let tapered = cube(Modifiers {
            cone: 1.0,
            ..default()
        });
        assert!(shape_distance(&tapered, Vec3::new(0.0, 1.0, 0.0)).abs() < 1e-3);
        assert!(shape_distance(&tapered, Vec3::new(1.0, -1.0, 0.0)).abs() < 1e-3);
        assert!(shape_distance(&tapered, Vec3::new(0.8, 0.5, 0.0)) > 0.0);
        // The base stays a square with sharp corners. This is the whole reason
        // the taper scales the cross-section rather than using the uber
        // primitive's own, which would round them off.
        let base_corner = shape_distance(&tapered, Vec3::new(1.0, -1.0, 1.0));
        assert!(
            base_corner.abs() < 1e-3,
            "base corner should be sharp, got {base_corner}"
        );

        // The taper takes the same amount off every side, so a slab three times
        // as long tapers to a ridge, not to a smaller slab of the same shape.
        let ridge = shaped(
            SdfShape::Cube,
            Transform::from_scale(Vec3::new(3.0, 1.0, 1.0)),
            Modifiers {
                cone: 1.0,
                ..default()
            },
        );
        // Half sizes are (3, 1, 1), so the top loses 1 from every side and ends
        // as a line running along x from -2 to 2.
        assert!(shape_distance(&ridge, Vec3::new(2.0, 1.0, 0.0)).abs() < 1e-3);
        assert!(shape_distance(&ridge, Vec3::new(2.4, 1.0, 0.0)) > 0.0);
        // Just below the top it is still 2 wide in x. Scaling the footprint
        // instead would have kept the 3:1 ratio and left barely anything there,
        // so this is the assertion that tells the two apart.
        assert!(shape_distance(&ridge, Vec3::new(1.0, 0.9, 0.0)) < 0.0);

        // Taper and corner radius compete for the same footprint. Asking for
        // both at once has to clamp, not collapse the shape.
        let both = cube(Modifiers {
            cone: 1.0,
            bevel: 1.0,
            ..default()
        });
        assert!(shape_distance(&both, Vec3::new(0.0, 1.0, 0.0)).abs() < 1e-3);
        assert!(shape_distance(&both, Vec3::new(1.0, -1.0, 0.0)).abs() < 1e-3);

        // Thickness below 1 hollows it out without moving the outer surface.
        // Half strength leaves a hole exactly half the shape's width: on a unit
        // cube the wall runs from 0.5 out to 1.0.
        let hollow = cube(Modifiers {
            thickness: 0.5,
            ..default()
        });
        assert!(shape_distance(&hollow, Vec3::ZERO) > 0.0);
        assert!((shape_distance(&hollow, Vec3::new(2.0, 0.0, 0.0)) - 1.0).abs() < 1e-4);
        assert!(shape_distance(&hollow, Vec3::new(0.5, 0.0, 0.0)).abs() < 1e-3);
        assert!(shape_distance(&hollow, Vec3::new(0.75, 0.0, 0.0)) < 0.0);
        assert!(shape_distance(&hollow, Vec3::new(0.25, 0.0, 0.0)) > 0.0);
        // The bore runs straight through: open at both ends, no floor. A
        // closed shell would be invisible from outside anyway.
        assert!(shape_distance(&hollow, Vec3::new(0.0, 0.99, 0.0)) > 0.0);
        assert!(shape_distance(&hollow, Vec3::new(0.0, -0.99, 0.0)) > 0.0);

        // A thin plate becomes a frame: the bore goes through its height and
        // its wall is measured against the footprint, not the thickness.
        let plate = shaped(
            SdfShape::Cube,
            Transform::from_scale(Vec3::new(1.0, 0.1, 1.0)),
            Modifiers {
                thickness: 0.5,
                ..default()
            },
        );
        assert!(shape_distance(&plate, Vec3::ZERO) > 0.0);
        assert!(shape_distance(&plate, Vec3::new(0.75, 0.0, 0.0)) < 0.0);

        // Thickness 0 is the thinnest wall the shape can have, not a solid one.
        // The two used to collide, because solid was encoded as a zero wall.
        let paper = cube(Modifiers {
            thickness: 0.0,
            ..default()
        });
        assert!(shape_distance(&paper, Vec3::ZERO) > 0.0);
        assert!(shape_distance(&paper, Vec3::new(0.99, 0.0, 0.0)).abs() < 0.02);

        // Tapered, a shell is a funnel: the bore is a slit at the narrow end
        // and closes off entirely towards the wide one, so the base is solid.
        let funnel = cube(Modifiers {
            cone: 0.5,
            thickness: 0.3,
            ..default()
        });
        assert!(shape_distance(&funnel, Vec3::new(0.0, -0.5, 0.0)) < 0.0);
        assert!(shape_distance(&funnel, Vec3::new(0.0, 0.95, 0.0)) > 0.0);
    }

    /// Sharpen is a superellipsoid exponent, and at rest it has to leave the
    /// sphere alone, or every unmodified sphere in the world quietly changes
    /// shape.
    #[test]
    fn sharpen_at_rest_is_a_plain_sphere() {
        let sphere = shaped(SdfShape::Sphere, Transform::IDENTITY, Modifiers::default());
        assert!((shape_distance(&sphere, Vec3::new(3.0, 0.0, 0.0)) - 2.0).abs() < 1e-4);
        // Turning it up squares the shape off, so the diagonal gains material.
        let sharp = shaped(
            SdfShape::Sphere,
            Transform::IDENTITY,
            Modifiers {
                sharpen: 0.5,
                ..default()
            },
        );
        let diagonal = Vec3::splat(0.8);
        assert!(shape_distance(&sharp, diagonal) < shape_distance(&sphere, diagonal));
    }

    // ------------------------------------------------------------ blend modes

    /// The modes past add/subtract/intersect are what SDF Modeler leans on, and
    /// each is a different arrangement of the same three. Paint is the one that
    /// must not touch the field at all.
    #[test]
    fn blend_modes_follow_the_editors_definitions() {
        let mode = |mode| CsgOperation { mode, ..default() };
        // Deep inside the field, and inside the incoming shape too.
        let field = -1.0;
        let shape = -0.5;

        assert_eq!(blend(shape, field, &pack(mode(GPU_MODE_ADD)), false), field);
        assert_eq!(
            blend(shape, field, &pack(mode(GPU_MODE_INTERSECT)), false),
            shape
        );
        // Subtracting carves it back out, so the point ends up outside.
        assert_eq!(
            blend(shape, field, &pack(mode(GPU_MODE_SUBTRACT)), false),
            0.5
        );
        // Paint only recolours.
        assert_eq!(
            blend(shape, field, &pack(mode(GPU_MODE_PAINT)), false),
            field
        );
    }

    fn pack(operation: CsgOperation) -> GpuBlend {
        GpuBlend {
            mode: operation.mode,
            radius: operation.radius,
            strength: operation.strength,
            padding: 0.0,
        }
    }

    // ------------------------------------------------------ distance, exactly

    #[test]
    fn rotation_is_applied_in_the_shapes_own_frame() {
        let scene = [placed(
            SdfShape::Cube,
            Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4)),
            union(0.0),
        )];
        // Turning the cube 45 degrees points a vertical edge at +X. The sample
        // lands at (sqrt(2), 0, sqrt(2)) in the cube's frame, so two axes are
        // each sqrt(2) - 1 outside.
        let expected = ((2f32.sqrt() - 1.0).powi(2) * 2.0).sqrt();
        let measured = scene_distance(&scene, Vec3::new(2.0, 0.0, 0.0));
        assert!(
            (measured - expected).abs() < 1e-5,
            "expected {expected}, got {measured}"
        );
    }

    #[test]
    fn non_uniform_box_scale_stays_an_exact_distance() {
        let scene = [placed(
            SdfShape::Cube,
            Transform::from_scale(Vec3::new(4.0, 1.0, 1.0)),
            union(0.0),
        )];
        // Half extents become (4, 1, 1), so a point 6 out along X is exactly 2
        // away. The old smallest-axis correction under-reported this as 0.5.
        assert!((scene_distance(&scene, Vec3::new(6.0, 0.0, 0.0)) - 2.0).abs() < 1e-5);
    }

    /// Brute-force nearest point on an ellipsoid surface, by sampling it.
    /// Slow and dumb, which is what makes it trustworthy as a reference.
    fn true_distance_to_ellipsoid(probe: Vec3, radii: Vec3) -> f32 {
        let mut nearest = f32::MAX;
        for latitude_step in 0..=180 {
            let latitude = latitude_step as f32 * std::f32::consts::PI / 180.0;
            for longitude_step in 0..360 {
                let longitude = longitude_step as f32 * std::f32::consts::TAU / 360.0;
                let on_unit_sphere = Vec3::new(
                    latitude.sin() * longitude.cos(),
                    latitude.cos(),
                    latitude.sin() * longitude.sin(),
                );
                nearest = nearest.min(probe.distance(on_unit_sphere * radii));
            }
        }
        nearest
    }

    #[test]
    fn ellipsoid_estimate_never_overshoots() {
        // Overshooting is the one failure that matters: a march step longer
        // than the real gap walks straight through the surface.
        let radii = Vec3::new(2.0, 0.5, 1.0);
        for x in [-4.0, -1.5, 0.3, 2.5, 5.0] {
            for y in [-3.0, -0.7, 0.9, 4.0] {
                for z in [-2.5, 0.4, 3.0] {
                    let probe = Vec3::new(x, y, z);
                    let reported = ellipsoid_distance(probe, radii, 2.0);
                    if reported <= 0.0 {
                        continue; // inside; the surface sample says nothing useful
                    }
                    let truth = true_distance_to_ellipsoid(probe, radii);
                    assert!(
                        reported <= truth + 1e-3,
                        "overshot at {probe:?}: said {reported}, truth {truth}"
                    );
                }
            }
        }
    }

    #[test]
    fn elliptical_cylinder_reduces_to_a_round_one() {
        let round = cylinder_distance(Vec3::new(3.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 1.0));
        assert!((round - 2.0).abs() < 1e-5);
        // Stretched on X only: the same probe is now much closer to the wall.
        let stretched = cylinder_distance(Vec3::new(3.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 1.0));
        assert!((stretched - 1.0).abs() < 1e-5);
    }

    #[test]
    fn stretched_sphere_stays_conservative() {
        let scene = [placed(
            SdfShape::Sphere,
            Transform::from_scale(Vec3::new(4.0, 1.0, 1.0)),
            union(0.0),
        )];
        // An ellipsoid has no cheap exact SDF, so the reported distance may be
        // short - but it must never overshoot, or sphere tracing punches through.
        let reported = scene_distance(&scene, Vec3::new(0.0, 3.0, 0.0));
        assert!(reported > 0.0);
        assert!(
            reported <= 2.0 + 1e-5,
            "overshot the true distance: {reported}"
        );
    }

    #[test]
    fn cylinder_matches_closed_form_on_side_cap_and_corner() {
        let scene = [placed(
            SdfShape::Cylinder,
            Transform::from_scale(Vec3::new(1.0, 2.0, 1.0)),
            union(0.0),
        )];
        // Straight out from the curved side.
        assert!((scene_distance(&scene, Vec3::new(4.0, 0.0, 0.0)) - 3.0).abs() < 1e-5);
        // Straight out from a flat cap, along the axis.
        assert!((scene_distance(&scene, Vec3::new(0.0, 5.0, 0.0)) - 3.0).abs() < 1e-5);
        // Past the rim, so both offsets count.
        let corner = scene_distance(&scene, Vec3::new(2.0, 3.0, 0.0));
        assert!((corner - 2f32.sqrt()).abs() < 1e-5);
        // Inside, nearest wall is the curved one.
        assert!((scene_distance(&scene, Vec3::ZERO) + 1.0).abs() < 1e-5);
    }

    #[test]
    fn cylinder_height_and_radius_scale_independently() {
        let scene = [placed(
            SdfShape::Cylinder,
            Transform::from_scale(Vec3::new(2.0, 3.0, 2.0)),
            union(0.0),
        )];
        // Radius 2 and half height 3, both exact: the round axes still match.
        assert!((scene_distance(&scene, Vec3::new(5.0, 0.0, 0.0)) - 3.0).abs() < 1e-5);
        assert!((scene_distance(&scene, Vec3::new(0.0, 7.0, 0.0)) - 4.0).abs() < 1e-5);
    }

    // ---------------------------------------------------------------- physics

    #[test]
    fn sliding_turns_into_spin() {
        // Sliding along +X on a floor whose normal is +Y, no spin yet.
        let (velocity_change, spin_change) = contact_friction(
            Vec3::Y,
            Vec3::X,
            Vec3::ZERO,
            0.5,
            10.0,
            FRICTION_COEFFICIENT,
        );
        // Friction opposes the slide...
        assert!(velocity_change.x < 0.0);
        // ...and torques the ball forward, which is -Z for +X travel on +Y up.
        assert!(spin_change.z < 0.0);
    }

    #[test]
    fn rolling_without_slipping_is_left_alone() {
        // Contact point stationary: v + w x arm == 0, so there is no slip.
        let radius = 0.5;
        let velocity = Vec3::X;
        let spin = Vec3::new(0.0, 0.0, -velocity.x / radius);
        let (velocity_change, spin_change) =
            contact_friction(Vec3::Y, velocity, spin, radius, 10.0, FRICTION_COEFFICIENT);
        assert!(velocity_change.length() < 1e-5);
        assert!(spin_change.length() < 1e-5);
    }

    #[test]
    fn coulomb_caps_friction_on_a_weak_contact() {
        // Barely resting on the surface, so friction cannot kill a fast slide.
        let (gentle, _) = contact_friction(
            Vec3::Y,
            Vec3::X * 10.0,
            Vec3::ZERO,
            0.5,
            0.01,
            FRICTION_COEFFICIENT,
        );
        assert!((gentle.length() - FRICTION_COEFFICIENT * 0.01).abs() < 1e-6);
    }

    #[test]
    fn separate_spheres_do_not_interact() {
        assert!(
            sphere_pair_correction(
                Vec3::ZERO,
                1.0,
                Vec3::ZERO,
                Vec3::new(3.0, 0.0, 0.0),
                1.0,
                Vec3::ZERO
            )
            .is_none()
        );
    }

    #[test]
    fn overlapping_spheres_split_the_gap_and_stop_closing() {
        let (separation, velocity_change) = sphere_pair_correction(
            Vec3::ZERO,
            1.0,
            Vec3::X,
            Vec3::new(1.5, 0.0, 0.0),
            1.0,
            -Vec3::X,
        )
        .expect("these overlap by 0.5");

        // Half the overlap each, pushed apart along the line of centres.
        assert!((separation - Vec3::new(-0.25, 0.0, 0.0)).length() < 1e-5);
        // Closing at 2 along that line, so each sheds half of it.
        assert!((velocity_change - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn overlapping_but_separating_spheres_keep_their_velocity() {
        let (_, velocity_change) = sphere_pair_correction(
            Vec3::ZERO,
            1.0,
            -Vec3::X,
            Vec3::new(1.5, 0.0, 0.0),
            1.0,
            Vec3::X,
        )
        .expect("these overlap");
        assert_eq!(velocity_change, Vec3::ZERO);
    }

    // ------------------------------------------------------ blending, marched

    #[test]
    fn hard_union_is_the_nearer_of_the_two() {
        let scene = [
            placed(SdfShape::Sphere, Transform::IDENTITY, union(0.0)),
            placed(
                SdfShape::Sphere,
                Transform::from_xyz(4.0, 0.0, 0.0),
                union(0.0),
            ),
        ];
        assert!((scene_distance(&scene, Vec3::new(3.0, 0.0, 0.0)) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn blending_pulls_the_surface_outwards_between_two_shapes() {
        let apart = 1.6;
        let hard = [
            placed(SdfShape::Sphere, Transform::IDENTITY, union(0.0)),
            placed(
                SdfShape::Sphere,
                Transform::from_xyz(apart, 0.0, 0.0),
                union(0.0),
            ),
        ];
        let blended = [
            hard[0].clone(),
            placed(
                SdfShape::Sphere,
                Transform::from_xyz(apart, 0.0, 0.0),
                union(0.5),
            ),
        ];
        // Beside the seam, the blended field must report the surface as nearer.
        let probe = Vec3::new(apart * 0.5, 1.2, 0.0);
        assert!(scene_distance(&blended, probe) < scene_distance(&hard, probe));
    }

    #[test]
    fn subtract_carves_a_hole() {
        let scene = [
            placed(
                SdfShape::Cube,
                Transform::from_scale(Vec3::splat(2.0)),
                union(0.0),
            ),
            placed(
                SdfShape::Sphere,
                Transform::IDENTITY,
                CsgOperation {
                    mode: GPU_MODE_SUBTRACT,
                    ..default()
                },
            ),
        ];
        // The cube centre is now hollow, so the origin sits outside the solid.
        assert!(scene_distance(&scene, Vec3::ZERO) > 0.0);
        // A point in the remaining shell is still inside.
        assert!(scene_distance(&scene, Vec3::new(1.5, 0.0, 0.0)) < 0.0);
    }
    // ----------------------------------------------------------- the arguments

    /// A flag reader is trivial until it is wrong, and then every tuned value
    /// silently reads its default. Each case here is one way that happens.
    #[test]
    fn a_flag_reads_the_number_after_it_and_nothing_else() {
        use crate::args::value_in;

        let line: Vec<String> = ["idk", "bench", "spread:80", "--omega", "1.0", "--no-grid"]
            .iter()
            .map(|word| word.to_string())
            .collect();

        assert_eq!(value_in(&line, "--omega"), Some(1.0));
        // Absent is not zero. `unwrap_or(DEFAULT)` at every call site depends on
        // this, and a zero omega would be a march that never advances.
        assert_eq!(value_in(&line, "--grid"), None);
        // Present but followed by another flag, not a number.
        assert_eq!(value_in(&line, "--no-grid"), None);
        // Present as the last word, with nothing after it at all.
        let trailing = vec!["idk".to_string(), "--speed".to_string()];
        assert_eq!(value_in(&trailing, "--speed"), None);
        // A prefix of a real flag is not that flag.
        assert_eq!(value_in(&line, "--omeg"), None);
    }

    // ------------------------------------------------------------------ input

    /// A binding has to survive the whole press-hold-release cycle: the frame
    /// after a press is a *held* frame, not a second press, and that is exactly
    /// what a toggle bound to it depends on.
    #[test]
    fn a_bound_key_drives_its_action_for_one_press_only() {
        use crate::game::input::{Action, InputPlugin};

        let mut app = App::new();
        // Bevy's own InputPlugin would clear the keyboard state this test sets
        // by hand, so drive the raw resources directly instead.
        app.add_plugins((MinimalPlugins, InputPlugin))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>();

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.update();
        let actions = app.world().resource::<ButtonInput<Action>>();
        assert!(actions.just_pressed(Action::Forward));
        assert!(actions.pressed(Action::Forward));

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear(); // the key is down, but this frame is not a fresh press
        app.update();
        let actions = app.world().resource::<ButtonInput<Action>>();
        assert!(!actions.just_pressed(Action::Forward));
        assert!(actions.pressed(Action::Forward));

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyW);
        app.update();
        let actions = app.world().resource::<ButtonInput<Action>>();
        assert!(actions.just_released(Action::Forward));
        assert!(!actions.pressed(Action::Forward));
    }
    // ---------------------------------------------------------------- culling

    /// Culling is only allowed to save work, never to change the answer. The
    /// reference loop here blends every shape unconditionally; the real
    /// `scene_distance` skips the ones its box test rejects. They must agree
    /// **exactly** - a skipped ADD contributes nothing at all to `min` or to
    /// `union_smooth`, so this is bit-for-bit, not approximate.
    #[test]
    fn box_culling_never_changes_the_field() {
        fn uncalled(shapes: &[GpuShape], point: Vec3) -> f32 {
            let mut field = MAX_MARCH_DISTANCE;
            for (index, shape) in shapes.iter().enumerate() {
                let distance = shape_distance(shape, point);
                field = if index == 0 {
                    distance
                } else {
                    blend(distance, field, &shape.blend, shape.chamfer != 0)
                };
            }
            field
        }

        // A cheap deterministic generator beats a dependency, and a fixed seed
        // means a failure is reproducible.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / 16777216.0 // [0, 1)
        };
        macro_rules! spread {
            ($scale:expr) => {
                (next() - 0.5) * 2.0 * $scale
            };
        }

        for _ in 0..200 {
            let count = 2 + (next() * 8.0) as usize;
            let shapes: Vec<GpuShape> = (0..count)
                .map(|_| {
                    let kind = (next() * 3.0) as u32;
                    let shape = match kind {
                        0 => SdfShape::Sphere,
                        1 => SdfShape::Cube,
                        _ => SdfShape::Cylinder,
                    };
                    let placement = Transform {
                        translation: Vec3::new(spread!(4.0), spread!(4.0), spread!(4.0)),
                        rotation: Quat::from_euler(
                            EulerRot::XYZ,
                            spread!(3.14),
                            spread!(3.14),
                            spread!(3.14),
                        ),
                        scale: Vec3::new(
                            0.2 + next() * 2.0,
                            0.2 + next() * 2.0,
                            0.2 + next() * 2.0,
                        ),
                    };
                    let modifiers = Modifiers {
                        round: next(),
                        bevel: next(),
                        thickness: next(),
                        cone: next(),
                        sharpen: next() * 0.9,
                    };
                    // Every mode, so the modes that must never be culled are
                    // exercised as hard as ADD is.
                    let operation = CsgOperation {
                        mode: (next() * 9.0) as u32,
                        chamfer: next() < 0.5,
                        radius: next() * 0.8,
                        strength: next() * 0.5,
                    };
                    shape.to_gpu(
                        &GlobalTransform::from(placement),
                        Some(&modifiers),
                        Some(&operation),
                        None,
                    )
                })
                .collect();

            for _ in 0..20 {
                let point = Vec3::new(spread!(7.0), spread!(7.0), spread!(7.0));
                let culled = scene_distance(&shapes, point);
                let full = uncalled(&shapes, point);
                assert_eq!(
                    culled, full,
                    "culling changed the field at {point:?} over {count} shapes"
                );
            }
        }
    }
    /// The parity test above passes trivially if nothing is ever culled, so
    /// pin the predicate down directly: it must fire on a distant ADD, and must
    /// never fire on a mode whose formula has not been proven safe.
    #[test]
    fn the_cull_fires_on_a_distant_add_and_never_on_another_mode() {
        let far = Vec3::new(30.0, 0.0, 0.0);
        let near_field = 1.0;

        let added = placed(SdfShape::Cube, Transform::IDENTITY, union(0.0));
        assert!(shape_cannot_reach(&added, far, near_field));
        // Close enough to matter: the box is nearer than the field.
        assert!(!shape_cannot_reach(
            &added,
            Vec3::new(1.2, 0.0, 0.0),
            near_field
        ));

        for mode in [
            GPU_MODE_SUBTRACT,
            GPU_MODE_INTERSECT,
            GPU_MODE_PAINT,
            GPU_MODE_PUSH,
            GPU_MODE_AVOID,
            GPU_MODE_EMBOSS,
            GPU_MODE_DEBOSS,
            GPU_MODE_SHELL,
        ] {
            let other = placed(
                SdfShape::Cube,
                Transform::IDENTITY,
                CsgOperation { mode, ..default() },
            );
            assert!(
                !shape_cannot_reach(&other, far, near_field),
                "mode {mode} was culled without a proof that it is safe to"
            );
        }
    }

    /// The bound has to survive the shape the ellipsoid estimate is worst on: a
    /// long thin one, where the returned distance is a fraction of the true
    /// one. Sampling along the long axis is where a naive box bound breaks.
    #[test]
    fn the_cull_bound_holds_under_a_stretched_ellipsoid() {
        let shape = shaped(
            SdfShape::Sphere,
            Transform::from_scale(Vec3::new(4.0, 0.2, 0.2)),
            Modifiers::default(),
        );
        for step in 1..200 {
            let point = Vec3::new(step as f32 * 0.15, 0.3, -0.2);
            let bound =
                shape.cull_scale * cull_box_distance(point - shape.center, shape.cull_extent);
            assert!(
                bound <= shape_distance(&shape, point) + 1e-4,
                "bound {bound} overshot the estimate at {point:?}"
            );
        }
    }
    // ----------------------------------------------------------- bench scenes

    /// A count sweep only means something if the scenes cover the same pixels.
    /// Every brush must stay inside the slab, and the slab the scene fills must
    /// not grow with the count - which is exactly what the last measurement
    /// round got wrong.
    #[test]
    fn every_bench_count_fills_the_same_slab() {
        use crate::dev::bench::{SLAB_HALF_SIZE, cells_per_axis, grid_layout};

        for count in [1, 8, 20, 27, 64, 80, 125] {
            let layout = grid_layout(count);
            assert_eq!(layout.len(), count);

            let cell = SLAB_HALF_SIZE / cells_per_axis(count) as f32;
            for placement in &layout {
                assert_eq!(placement.scale, cell);
                let corner = placement.translation.abs() + cell;
                assert!(
                    corner.cmple(SLAB_HALF_SIZE + Vec3::splat(1e-4)).all(),
                    "count {count} put a brush corner at {corner:?}, outside {SLAB_HALF_SIZE:?}"
                );
            }

            // Full rows reach both ends, so the footprint is the slab itself
            // and not something that creeps outwards with the count.
            let widest = layout
                .iter()
                .map(|placement| placement.translation.x + cell.x)
                .fold(f32::MIN, f32::max);
            assert!(
                (widest - SLAB_HALF_SIZE.x).abs() < 1e-4,
                "count {count} stopped at {widest}"
            );
        }
    }
    /// The spread scene only measures what it claims to if its boxes stay
    /// apart: two that touch merge into one blended surface, which is one
    /// reject instead of two and a different shape to evaluate.
    #[test]
    fn spread_boxes_never_touch() {
        use crate::dev::bench::{SPREAD_HALF_SIZE, spread_layout};

        for count in [8, 20, 80, 125, 256] {
            let layout = spread_layout(count);
            assert_eq!(layout.len(), count);

            for placement in &layout {
                let corner = placement.translation.abs() + placement.scale;
                assert!(
                    corner.cmple(SPREAD_HALF_SIZE + Vec3::splat(1e-4)).all(),
                    "count {count} put a box corner at {corner:?}, outside the volume"
                );
            }

            // ponytail: O(n^2) over at most a few hundred boxes, in a test.
            for (index, one) in layout.iter().enumerate() {
                for other in &layout[index + 1..] {
                    let gap = (one.translation - other.translation).abs() - one.scale - other.scale;
                    assert!(
                        gap.cmpgt(Vec3::ZERO).any(),
                        "count {count} overlapped two boxes at {:?} and {:?}",
                        one.translation,
                        other.translation
                    );
                }
            }
        }
    }
    // --------------------------------------------------------------- marching

    /// Over-relaxation is only allowed to make the march *cheaper*, never to
    /// let it miss something. This mirrors `ray_march` in sdf.wgsl on the CPU
    /// (the shader cannot be run here) and compares a relaxed march against a
    /// plain one over random scenes and random rays.
    ///
    /// The failure that matters is one-sided: a relaxed march reporting a hit
    /// **further along the ray** than the plain one means a surface was jumped.
    #[test]
    fn over_relaxation_never_marches_past_a_surface() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / 16777216.0
        };
        macro_rules! spread {
            ($scale:expr) => {
                (next() - 0.5) * 2.0 * $scale
            };
        }

        let mut relaxed_steps = 0u32;
        let mut plain_steps = 0u32;
        let mut compared = 0u32;

        for _ in 0..60 {
            // Unions only. Subtract and intersect are `max`-based, and a max of
            // two bounds can *overestimate* near the seam - which breaks the
            // Lipschitz condition over-relaxation rests on, for plain tracing
            // too. Those seams are a known hazard, not something this test can
            // paper over.
            let shapes: Vec<GpuShape> = (0..2 + (next() * 6.0) as usize)
                .map(|_| {
                    let kind = (next() * 3.0) as u32;
                    let shape = match kind {
                        0 => SdfShape::Sphere,
                        1 => SdfShape::Cube,
                        _ => SdfShape::Cylinder,
                    };
                    let placement = Transform {
                        translation: Vec3::new(spread!(6.0), spread!(6.0), spread!(6.0)),
                        rotation: Quat::from_euler(
                            EulerRot::XYZ,
                            spread!(3.14),
                            spread!(3.14),
                            spread!(3.14),
                        ),
                        scale: Vec3::new(
                            0.3 + next() * 1.5,
                            0.3 + next() * 1.5,
                            0.3 + next() * 1.5,
                        ),
                    };
                    let operation = CsgOperation {
                        radius: next() * 0.5,
                        ..default()
                    };
                    shape.to_gpu(
                        &GlobalTransform::from(placement),
                        Some(&Modifiers::default()),
                        Some(&operation),
                        None,
                    )
                })
                .collect();

            for _ in 0..40 {
                let origin = Vec3::new(spread!(14.0), spread!(14.0), spread!(14.0));
                let direction = (Vec3::new(spread!(1.0), spread!(1.0), spread!(1.0))
                    - origin.normalize_or_zero() * 0.0)
                    .normalize_or_zero();
                if direction == Vec3::ZERO || scene_distance(&shapes, origin) < 0.0 {
                    continue; // starting inside is its own case, handled by the sign test
                }

                let evaluate = |point| scene_distance(&shapes, point);
                let (plain, plain_cost) =
                    march(&evaluate, &evaluate, origin, direction, 1.0, 0.001, 512);
                let (relaxed, relaxed_cost) =
                    march(&evaluate, &evaluate, origin, direction, 1.2, 0.001, 512);

                // Half a per-step threshold of slack: the two marches stop at
                // slightly different points on the same surface.
                assert!(
                    relaxed <= plain + 0.05,
                    "relaxed march ran {relaxed} past the plain hit at {plain}"
                );
                plain_steps += plain_cost;
                relaxed_steps += relaxed_cost;
                compared += 1;
            }
        }

        println!("plain {plain_steps} steps, relaxed {relaxed_steps} over {compared} rays");
        assert!(compared > 500, "only {compared} rays actually ran");
        // The whole point is fewer steps. If relaxation costs more than plain
        // tracing on random scenes, the fallback is thrashing.
        assert!(
            relaxed_steps < plain_steps,
            "relaxed spent {relaxed_steps} steps against plain's {plain_steps}"
        );
    }
    // --------------------------------------------------------------- the grid

    /// The grid is allowed to be pessimistic and must never be optimistic. A
    /// cell only knows its own shapes, so if it ever reports a *larger*
    /// distance than the exact field, a march using it steps through geometry.
    ///
    /// Every mode is in the scenes, including the ones that read the field
    /// globally - those must end up in every cell, and this is what catches it
    /// if they do not.
    #[test]
    fn the_grid_never_reports_more_than_the_exact_field() {
        use crate::sdf::field::{build_grid, scene_bounds, scene_distance_gridded};

        let mut state = 0xD1B5_4A32_D192_ED03u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / 16777216.0
        };
        macro_rules! spread {
            ($scale:expr) => {
                (next() - 0.5) * 2.0 * $scale
            };
        }

        for resolution in [1, 4, 16] {
            for _ in 0..40 {
                let shapes: Vec<GpuShape> = (0..2 + (next() * 8.0) as usize)
                    .map(|_| {
                        let shape = match (next() * 3.0) as u32 {
                            0 => SdfShape::Sphere,
                            1 => SdfShape::Cube,
                            _ => SdfShape::Cylinder,
                        };
                        let placement = Transform {
                            translation: Vec3::new(spread!(8.0), spread!(8.0), spread!(8.0)),
                            rotation: Quat::from_euler(
                                EulerRot::XYZ,
                                spread!(3.14),
                                spread!(3.14),
                                spread!(3.14),
                            ),
                            scale: Vec3::new(
                                0.3 + next() * 2.0,
                                0.3 + next() * 2.0,
                                0.3 + next() * 2.0,
                            ),
                        };
                        let operation = CsgOperation {
                            mode: (next() * 9.0) as u32,
                            chamfer: next() < 0.5,
                            radius: next() * 0.6,
                            strength: next() * 0.5,
                        };
                        shape.to_gpu(
                            &GlobalTransform::from(placement),
                            Some(&Modifiers::default()),
                            Some(&operation),
                            None,
                        )
                    })
                    .collect();

                let (bounds_min, bounds_max) = scene_bounds(&shapes);
                let grid = build_grid(&shapes, bounds_min, bounds_max, resolution);

                for _ in 0..60 {
                    let point = Vec3::new(spread!(12.0), spread!(12.0), spread!(12.0));
                    let exact = scene_distance(&shapes, point);
                    let gridded = scene_distance_gridded(&shapes, &grid, point);
                    assert!(
                        gridded <= exact + 1e-4,
                        "grid at resolution {resolution} reported {gridded} where the field is \
                         {exact}, at {point:?}"
                    );
                }
            }
        }
    }
    /// Soundness says the grid cannot report too much; this says the march that
    /// uses it lands in the same place. A grid that is merely conservative
    /// still has to draw the same picture.
    #[test]
    fn a_gridded_march_hits_what_the_exact_one_hits() {
        use crate::sdf::field::{build_grid, scene_bounds, scene_distance_gridded};

        let mut state = 0x1234_5678_9ABC_DEF1u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / 16777216.0
        };
        macro_rules! spread {
            ($scale:expr) => {
                (next() - 0.5) * 2.0 * $scale
            };
        }

        let mut compared = 0;
        let mut exact_steps = 0u32;
        let mut grid_steps = 0u32;

        for _ in 0..40 {
            let shapes: Vec<GpuShape> = (0..3 + (next() * 8.0) as usize)
                .map(|_| {
                    let shape = match (next() * 3.0) as u32 {
                        0 => SdfShape::Sphere,
                        1 => SdfShape::Cube,
                        _ => SdfShape::Cylinder,
                    };
                    let placement = Transform {
                        translation: Vec3::new(spread!(9.0), spread!(9.0), spread!(9.0)),
                        rotation: Quat::from_euler(
                            EulerRot::XYZ,
                            spread!(3.14),
                            spread!(3.14),
                            spread!(3.14),
                        ),
                        scale: Vec3::new(
                            0.4 + next() * 1.6,
                            0.4 + next() * 1.6,
                            0.4 + next() * 1.6,
                        ),
                    };
                    let operation = CsgOperation {
                        radius: next() * 0.5,
                        ..default()
                    };
                    shape.to_gpu(
                        &GlobalTransform::from(placement),
                        Some(&Modifiers::default()),
                        Some(&operation),
                        None,
                    )
                })
                .collect();

            let (bounds_min, bounds_max) = scene_bounds(&shapes);
            let grid = build_grid(&shapes, bounds_min, bounds_max, 16);
            let exact = |point| scene_distance(&shapes, point);
            let gridded = |point| scene_distance_gridded(&shapes, &grid, point);

            for _ in 0..40 {
                let origin = Vec3::new(spread!(16.0), spread!(16.0), spread!(16.0));
                let direction =
                    Vec3::new(spread!(1.0), spread!(1.0), spread!(1.0)).normalize_or_zero();
                if direction == Vec3::ZERO || exact(origin) < 0.0 {
                    continue;
                }

                // A fat threshold on purpose. The grid clamps to the cell
                // wall, and the wall margin is small - so with a hit test loose
                // enough to mistake the margin for geometry, an unconfirmed
                // gridded march stops in mid-air. This is the shader's own
                // situation: its threshold grows with distance.
                let (hit, cost) = march(&exact, &exact, origin, direction, 1.2, 0.05, 512);
                let (grid_hit, grid_cost) =
                    march(&gridded, &exact, origin, direction, 1.2, 0.05, 512);
                assert!(
                    (hit - grid_hit).abs() < 0.05,
                    "gridded march stopped at {grid_hit}, exact at {hit}"
                );
                exact_steps += cost;
                grid_steps += grid_cost;
                compared += 1;
            }
        }

        assert!(compared > 300, "only {compared} rays actually ran");
        println!("exact {exact_steps} steps, gridded {grid_steps} over {compared} rays");
    }
    /// Two faults that only show on a long ray *inside* the grid: cells that
    /// are not cubic, and cells that do not overlap.
    ///
    /// A flat world divided by the same count on every axis gives pancake
    /// cells, and the thinnest axis then sets the step length for every ray -
    /// on the bench scene that was 0.053 units at a time. A ray running along a
    /// cell wall crawls for a different reason. Either way it runs out of
    /// budget and draws as background: a slice of the world missing down the
    /// middle of the screen, which is what it did.
    ///
    /// The ray is aimed **straight down a cell plane at a target it must
    /// reach**. Two earlier versions of this test let the ray miss, and a miss
    /// agrees with a miss - they passed against both faults and measured
    /// nothing.
    #[test]
    fn a_long_ray_inside_the_grid_still_arrives() {
        use crate::sdf::field::{build_grid, scene_bounds, scene_distance_gridded};
        const SHADER_BUDGET: u32 = 128;

        let cube_at = |position: Vec3| {
            shaped(
                SdfShape::Cube,
                Transform {
                    translation: position,
                    scale: Vec3::splat(0.8),
                    ..default()
                },
                Modifiers::default(),
            )
        };
        // Wide and flat, like a level. The anchors alone fix the bounds, so the
        // cell planes do not move when the target is added.
        let anchors = [
            cube_at(Vec3::new(-20.0, 0.0, -20.0)),
            cube_at(Vec3::new(20.0, 0.0, 20.0)),
        ];
        let (bounds_min, bounds_max) = scene_bounds(&anchors);
        let planes = build_grid(&anchors, bounds_min, bounds_max, 16);

        for step in 2..planes.resolution.x - 2 {
            for nudge in [0.0f32, 0.002, -0.002] {
                let x = planes.origin.x + planes.cell_size.x * step as f32 + nudge;
                let mut shapes = anchors.to_vec();
                shapes.push(cube_at(Vec3::new(x, 0.0, -15.0)));

                let grid = build_grid(&shapes, bounds_min, bounds_max, 16);
                let exact = |point| scene_distance(&shapes, point);
                let gridded = |point| scene_distance_gridded(&shapes, &grid, point);

                let origin = Vec3::new(x, 0.0, 18.0);
                let direction = Vec3::new(0.0, 0.0, -1.0);
                let (hit, _) = march(&exact, &exact, origin, direction, 1.2, 0.01, SHADER_BUDGET);
                let (grid_hit, cost) = march(
                    &gridded,
                    &exact,
                    origin,
                    direction,
                    1.2,
                    0.01,
                    SHADER_BUDGET,
                );

                // Non-vacuous: the exact march has to actually reach the target.
                assert!(hit < 40.0, "the exact march missed its own target at x {x}");
                assert!(
                    (hit - grid_hit).abs() < 0.1,
                    "at x {x}: exact stopped at {hit}, gridded at {grid_hit} after {cost} steps"
                );
            }
        }
    }

    // ----------------------------------------------------------------- lights

    /// The spot cone is compared as cosines, and cosine runs backwards: the
    /// inner angle has the *larger* cosine. `smoothstep(cos_outer, cos_inner,
    /// alignment)` is only a falloff if that ordering holds, and it silently
    /// inverts the cone if it does not.
    #[test]
    fn a_spot_packs_a_cone_that_fades_outwards() {
        use crate::sdf::light::{Light, LightKind};

        let light = Light {
            kind: LightKind::Spot {
                inner: 0.25,
                outer: 0.45,
            },
            ..default()
        };
        let packed = light.to_gpu(&GlobalTransform::from(
            Transform::from_xyz(0.0, 5.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
        ));

        assert!(
            packed.cos_inner > packed.cos_outer,
            "inner {} should have the larger cosine, outer is {}",
            packed.cos_inner,
            packed.cos_outer
        );
        // -Z is forward, so a light above the origin looking at it points down.
        assert!(
            packed.direction.dot(Vec3::NEG_Y) > 0.99,
            "expected it to point down, got {:?}",
            packed.direction
        );

        // An outer angle inside the inner one would make the falloff run the
        // wrong way; it is pushed back out instead.
        let inverted = Light {
            kind: LightKind::Spot {
                inner: 0.5,
                outer: 0.1,
            },
            ..default()
        }
        .to_gpu(&GlobalTransform::IDENTITY);
        assert!(inverted.cos_inner > inverted.cos_outer);
    }
    /// The soft-shadow ratio needs a **true** distance, and the grid does not
    /// return one.
    ///
    /// `scene_distance_gridded` clamps its answer to the wall of the cell it
    /// landed in. That is sound for stepping - a ray never steps through
    /// anything - but Quilez's penumbra reads the same number as "an occluder
    /// is this close" and darkens for it. Cell walls are a cubic lattice, so
    /// the darkening is one too: square shadows, whatever shape cast them.
    ///
    /// The ray is aimed to miss by a wide margin and the exact field is
    /// required to call it lit, so the test cannot pass by both sides going
    /// dark together.
    #[test]
    fn the_soft_shadow_ratio_is_not_darkened_by_the_grid() {
        use crate::sdf::field::{build_grid, scene_bounds, scene_distance_gridded};

        const SOFTNESS: f32 = 12.0;
        const BIAS: f32 = 0.02;
        const STEPS: u32 = 48;

        // Mirrors `shadow_factor` in sdf.wgsl.
        let penumbra = |field: &dyn Fn(Vec3) -> f32, origin: Vec3, direction: Vec3, far: f32| {
            let mut shade = 1.0f32;
            let mut travelled = BIAS;
            for _ in 0..STEPS {
                if travelled >= far {
                    break;
                }
                let distance = field(origin + direction * travelled);
                if distance < 0.001 {
                    return 0.0;
                }
                shade = shade.min(SOFTNESS * distance / travelled);
                travelled += distance;
            }
            shade.clamp(0.0, 1.0)
        };

        let shapes = vec![
            shaped(
                SdfShape::Cube,
                Transform {
                    translation: Vec3::new(0.0, -0.5, 0.0),
                    scale: Vec3::new(20.0, 1.0, 20.0),
                    ..default()
                },
                Modifiers::default(),
            ),
            shaped(
                SdfShape::Sphere,
                Transform::from_xyz(0.0, 1.5, 0.0),
                Modifiers::default(),
            ),
        ];

        let (bounds_min, bounds_max) = scene_bounds(&shapes);
        let grid = build_grid(&shapes, bounds_min, bounds_max, 16);
        let exact = |point| scene_distance(&shapes, point);
        let gridded = |point| scene_distance_gridded(&shapes, &grid, point);

        // Straight up from open air above the floor, well clear of the
        // sphere. Nothing is in the way of any of these.
        let sun = Vec3::Y;
        let mut checked = 0;
        let mut worst: f32 = 0.0;
        for step in 0..24 {
            let across = 4.0 + step as f32 * 0.25;
            let origin = Vec3::new(across, 2.0, 0.0);
            let open = penumbra(&exact, origin, sun, 40.0);
            assert!(
                open > 0.9,
                "the exact field shadowed an open ray at x = {across}: {open}"
            );
            let through_grid = penumbra(&gridded, origin, sun, 40.0);
            worst = worst.max(open - through_grid);
            checked += 1;
        }

        assert!(checked == 24);

        // The one that matters. A sun low in the sky sends its shadow rays
        // *along* the level rather than out of it, so they stay inside the grid
        // for their whole length - and a flat level gives cells that are thin
        // in y, so the wall is a fraction of a unit away at every step.
        let grazing = Vec3::new(1.0, 0.25, 0.0).normalize();
        for step in 0..12 {
            let along = -9.0 + step as f32 * 0.5;
            let origin = Vec3::new(along, 1.0, 8.0);
            let open = penumbra(&exact, origin, grazing, 40.0);
            assert!(
                open > 0.9,
                "the exact field shadowed an open grazing ray at x = {along}: {open}"
            );
            worst = worst.max(open - penumbra(&gridded, origin, grazing, 40.0));
            checked += 1;
        }

        assert!(
            worst < 0.05,
            "the grid darkened an unoccluded ray by {worst} over {checked} rays;              the penumbra is reading cell walls as geometry"
        );
    }
    /// The shadow march reads a proxy built from cull boxes, never the field.
    /// Two things have to hold at once, and each alone is trivially satisfiable
    /// by a broken proxy.
    ///
    /// **It bounds the field from below**, so a shadow ray can only stop early
    /// - too much shadow, never a ray that walks through a caster. A proxy that
    /// returned zero everywhere would pass this.
    ///
    /// **It still lets light through** where the field does. A proxy that
    /// returned `MAX_MARCH_DISTANCE` everywhere would pass that.
    #[test]
    fn the_shadow_proxy_bounds_the_field_and_still_lets_light_through() {
        use crate::sdf::field::{
            build_grid, scene_bounds, scene_distance_gridded, shadow_proxy_distance,
        };

        const SOFTNESS: f32 = 12.0;
        const BIAS: f32 = 0.02;
        const STEPS: u32 = 48;

        // Mirrors `shadow_factor` in sdf.wgsl.
        let penumbra = |field: &dyn Fn(Vec3) -> f32, origin: Vec3, direction: Vec3, far: f32| {
            let mut shade = 1.0f32;
            let mut travelled = BIAS;
            for _ in 0..STEPS {
                if travelled >= far {
                    break;
                }
                let distance = field(origin + direction * travelled);
                if distance < 0.001 {
                    return 0.0;
                }
                shade = shade.min(SOFTNESS * distance / travelled);
                travelled += distance;
            }
            shade.clamp(0.0, 1.0)
        };

        let shapes = vec![
            shaped(
                SdfShape::Cube,
                Transform {
                    translation: Vec3::new(0.0, -0.5, 0.0),
                    scale: Vec3::new(20.0, 1.0, 20.0),
                    ..default()
                },
                Modifiers::default(),
            ),
            shaped(
                SdfShape::Sphere,
                Transform::from_xyz(0.0, 1.5, 0.0),
                Modifiers::default(),
            ),
            // Turned and rounded, so the bound has to be built in the shape's
            // own frame and has to survive a modifier eating into the box it
            // is measured against. An axis-aligned bound was one of the two
            // things wrong with the first version of this proxy.
            shaped(
                SdfShape::Cube,
                Transform {
                    translation: Vec3::new(-5.0, 1.0, 3.0),
                    rotation: Quat::from_rotation_y(0.6) * Quat::from_rotation_z(0.35),
                    scale: Vec3::new(3.0, 0.4, 1.2),
                },
                Modifiers {
                    round: 0.5,
                    ..default()
                },
            ),
            // Elliptical, so the round cross-section the proxy substitutes is
            // strictly wider than the shape and the bound is strictly loose.
            shaped(
                SdfShape::Cylinder,
                Transform {
                    translation: Vec3::new(5.0, 1.2, -2.0),
                    scale: Vec3::new(2.0, 1.2, 0.6),
                    ..default()
                },
                Modifiers::default(),
            ),
        ];

        let (bounds_min, bounds_max) = scene_bounds(&shapes);
        let grid = build_grid(&shapes, bounds_min, bounds_max, 16);
        let gridded = |point| scene_distance_gridded(&shapes, &grid, point);
        let proxy = |point| shadow_proxy_distance(&shapes, &grid, point);

        // Below the field wherever the field is outside a surface, sampled over
        // the whole scene box rather than along one ray - a bound that only
        // holds on the path a shadow happens to take is not a bound.
        //
        // Inside a solid the proxy sits *above* the field: a cull box reports
        // zero, never a negative depth. Out of scope on purpose. A shadow ray
        // starts at `surface_point + normal * SHADOW_BIAS` and stops the moment
        // the field reads under the surface threshold, so it never travels
        // through the inside of anything.
        let (mut outside, mut inside, mut loose) = (0, 0, 0);
        for xi in 0..13 {
            for yi in 0..13 {
                for zi in 0..13 {
                    let point = bounds_min
                        + (bounds_max - bounds_min) * Vec3::new(xi as f32, yi as f32, zi as f32)
                            / 12.0;
                    let (bound, field) = (proxy(point), gridded(point));
                    if field < 0.0 {
                        inside += 1;
                        continue;
                    }
                    assert!(
                        bound <= field + 1e-3,
                        "the proxy read {bound} where the field reads {field} at {point}"
                    );
                    if bound < field - 1e-3 {
                        loose += 1;
                    }
                    outside += 1;
                }
            }
        }
        // Or the bound above was never actually put to work.
        assert!(
            outside > inside,
            "only {outside} of {} sample points were outside the geometry",
            outside + inside
        );
        // And somewhere it has to be a real bound rather than a copy of the
        // field, or `<=` is being satisfied by equality and proves nothing
        // about the shapes the proxy substitutes.
        assert!(
            loose > 0,
            "the proxy equalled the field at every one of {outside} points"
        );

        // Under the sphere, looking at the sun. Occluded by the field, and the
        // proxy has to agree or the shadow is simply missing.
        let sun = Vec3::Y;
        let beneath = Vec3::new(0.0, 0.3, 0.0);
        assert_eq!(penumbra(&gridded, beneath, sun, 40.0), 0.0);
        assert_eq!(penumbra(&proxy, beneath, sun, 40.0), 0.0);

        // Out from under it, and still on the floor. The field calls these lit;
        // the proxy has to leave most of the light, or the level goes black.
        let mut checked = 0;
        for step in 0..12 {
            let across = 4.0 + step as f32 * 0.5;
            let origin = Vec3::new(across, 2.0, 0.0);
            let open = penumbra(&gridded, origin, sun, 40.0);
            assert!(open > 0.9, "the field shadowed an open ray at x = {across}");
            let through_proxy = penumbra(&proxy, origin, sun, 40.0);
            assert!(
                through_proxy > 0.9,
                "the proxy darkened an open ray at x = {across} to {through_proxy};                  its boxes reach further than the shapes that made them"
            );
            checked += 1;
        }
        assert_eq!(checked, 12);
    }

    // ----------------------------------------------- bodies leaving the world

    /// A body that misses the floor must be removed, not left falling.
    ///
    /// The renderer is what breaks otherwise: `scene_bounds` is one AABB over
    /// every shape, so a body accelerating downward forever drags the scene box
    /// with it and the acceleration grid - whose resolution comes from that box
    /// - collapses to a single cell across the level.
    #[test]
    fn a_body_that_leaves_the_world_is_removed() {
        let mut app = App::new();
        app.add_systems(Update, despawn_fallen_bodies);

        let body = |height: f32| {
            (
                SphereBody {
                    radius: 0.5,
                    velocity: Vec3::ZERO,
                    angular_velocity: Vec3::ZERO,
                    orientation: Quat::IDENTITY,
                    resting: false,
                },
                Transform::from_xyz(0.0, height, 0.0),
            )
        };
        let resting = app.world_mut().spawn(body(1.0)).id();
        let falling = app.world_mut().spawn(body(-1000.0)).id();
        // Far down, but still inside the world: a deep pit is not a fall.
        let deep = app.world_mut().spawn(body(-10.0)).id();

        app.update();

        assert!(app.world().get_entity(resting).is_ok());
        assert!(app.world().get_entity(deep).is_ok());
        assert!(
            app.world().get_entity(falling).is_err(),
            "a body 1000 units under the world was left in the scene"
        );
    }
}
