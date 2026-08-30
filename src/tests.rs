#[cfg(test)]
mod sdf_tests {
    use bevy::prelude::*;
    use crate::field::*;
    use crate::physics::*;
    use crate::world::*;

    fn placed(shape: SdfShape, placement: Transform, operation: CsgOperation) -> GpuShape {
        shape.to_gpu(&GlobalTransform::from(placement), None, Some(&operation), None)
    }

    fn shaped(shape: SdfShape, placement: Transform, modifiers: Modifiers) -> GpuShape {
        shape.to_gpu(&GlobalTransform::from(placement), Some(&modifiers), None, None)
    }

    fn union(radius: f32) -> CsgOperation {
        CsgOperation {
            radius,
            ..default()
        }
    }

    /// The authored scene has to survive spawning, not just compile. Without a
    /// `Transform` on the root, propagation never reaches the children and every
    /// shape packs at the origin - which looks exactly like a broken importer.
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
        assert!(placed.len() >= 8, "expected the authored brushes, got {}", placed.len());

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

    #[test]
    fn sphere_matches_closed_form() {
        let scene = [placed(
            SdfShape::Sphere,
            Transform::IDENTITY,
            union(0.0),
        )];
        assert!((scene_distance(&scene, Vec3::new(3.0, 0.0, 0.0)) - 2.0).abs() < 1e-5);
        assert!((scene_distance(&scene, Vec3::ZERO) + 1.0).abs() < 1e-5);
    }

    #[test]
    fn box_matches_closed_form_outside_face_edge_and_inside() {
        let scene = [placed(
            SdfShape::Cube,
            Transform::IDENTITY,
            union(0.0),
        )];
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
        for probe in [Vec3::new(3.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 3.0), Vec3::splat(2.0)] {
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
        assert!((corner - (4.5f32.sqrt() - 1.0)).abs() < 1e-4, "got {corner}");
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
        assert!(base_corner.abs() < 1e-3, "base corner should be sharp, got {base_corner}");

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
    /// sphere alone or every imported sphere quietly changes shape.
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

    /// The modes past add/subtract/intersect are what SDF Modeler leans on, and
    /// each is a different arrangement of the same three. Paint is the one that
    /// must not touch the field at all.
    #[test]
    fn blend_modes_follow_the_editors_definitions() {
        let mode = |mode| CsgOperation {
            mode,
            ..default()
        };
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
        assert_eq!(blend(shape, field, &pack(mode(GPU_MODE_PAINT)), false), field);
    }

    fn pack(operation: CsgOperation) -> GpuBlend {
        GpuBlend {
            mode: operation.mode,
            radius: operation.radius,
            strength: operation.strength,
            padding: 0.0,
        }
    }

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

    #[test]
    fn sliding_turns_into_spin() {
        // Sliding along +X on a floor whose normal is +Y, no spin yet.
        let (velocity_change, spin_change) =
            contact_friction(Vec3::Y, Vec3::X, Vec3::ZERO, 0.5, 10.0);
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
            contact_friction(Vec3::Y, velocity, spin, radius, 10.0);
        assert!(velocity_change.length() < 1e-5);
        assert!(spin_change.length() < 1e-5);
    }

    #[test]
    fn coulomb_caps_friction_on_a_weak_contact() {
        // Barely resting on the surface, so friction cannot kill a fast slide.
        let (gentle, _) = contact_friction(Vec3::Y, Vec3::X * 10.0, Vec3::ZERO, 0.5, 0.01);
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

    #[test]
    fn hard_union_is_the_nearer_of_the_two() {
        let scene = [
            placed(
                SdfShape::Sphere,
                Transform::IDENTITY,
                union(0.0),
            ),
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
            placed(
                SdfShape::Sphere,
                Transform::IDENTITY,
                union(0.0),
            ),
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
    /// A binding has to survive the whole press-hold-release cycle: the frame
    /// after a press is a *held* frame, not a second press, and that is exactly
    /// what a toggle bound to it depends on.
    #[test]
    fn a_bound_key_drives_its_action_for_one_press_only() {
        use crate::input::{Action, InputPlugin};

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
}
