#[cfg(test)]
mod sdf_tests {
    use crate::game::physics::*;
    use crate::game::world::*;
    use crate::sdf::field::*;
    use bevy::prelude::*;

    fn placed(placement: Transform, operation: CsgOperation) -> GpuShape {
        pack_brush(
            &GlobalTransform::from(placement),
            None,
            Some(&operation),
            None,
        )
    }

    fn shaped(placement: Transform, modifiers: Modifiers) -> GpuShape {
        pack_brush(
            &GlobalTransform::from(placement),
            Some(&modifiers),
            None,
            None,
        )
    }

    fn sphere_modifiers() -> Modifiers {
        Modifiers {
            round: 1.0,
            ..default()
        }
    }

    fn union(radius: f32) -> CsgOperation {
        CsgOperation {
            radius,
            ..default()
        }
    }

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

        (STOP, steps)
    }

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
            .query_filtered::<&GlobalTransform, With<Brush>>()
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
                .query_filtered::<Entity, With<Brush>>()
                .iter(app.world())
                .count(),
            "every brush should sit where it was written, not stacked at the origin"
        );
    }

    #[test]
    fn the_kernel_matches_the_uberprim_it_replaced() {
        let mut seed: u64 = 0x5eed_1234;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 40) as f32 / 16_777_216.0
        };

        for _ in 0..5000 {
            let half = Vec3::new(0.2 + next() * 2.0, 0.2 + next() * 2.0, 0.2 + next() * 2.0);
            let footprint = half.x.min(half.z);

            let wall = next() * footprint;
            let side = next() * footprint;
            let cap = next() * half.y;
            let point = Vec3::new(
                (next() - 0.5) * 6.0,
                (next() - 0.5) * 6.0,
                (next() - 0.5) * 6.0,
            );

            let s = half.extend(wall);
            let r = Vec3::new(side, cap, 0.0);
            let old = legacy_combined_primitive(point, s, r);
            let new = rounded_box_in_legacy_terms(point, s, r);
            assert!(
                (old - new).abs() < 1e-4,
                "kernels disagree at {point:?} half {half:?} wall {wall} \
                 side {side} cap {cap}: old {old}, new {new}"
            );
        }
    }

    #[test]
    fn a_fully_rounded_box_is_an_exact_sphere() {
        let scene = [shaped(Transform::IDENTITY, sphere_modifiers())];
        for probe in [
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::new(2.0, -1.0, 0.0),
            Vec3::new(2.0, 2.0, 2.0),
        ] {
            let expected = probe.length() - 1.0;
            let actual = scene_distance(&scene, probe);
            assert!(
                (actual - expected).abs() < 1e-5,
                "at {probe}: {actual} against an exact sphere's {expected}"
            );
        }
        assert!((scene_distance(&scene, Vec3::ZERO) + 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_fully_bevelled_box_is_an_exact_cylinder() {
        let scene = [shaped(
            Transform::IDENTITY,
            Modifiers {
                bevel: 1.0,
                ..default()
            },
        )];

        assert!((scene_distance(&scene, Vec3::new(3.0, 0.0, 0.0)) - 2.0).abs() < 1e-5);

        assert!((scene_distance(&scene, Vec3::new(0.0, 3.0, 0.0)) - 2.0).abs() < 1e-5);

        let rim = scene_distance(&scene, Vec3::new(2.0, 2.0, 0.0));
        assert!((rim - 2f32.sqrt()).abs() < 1e-5);

        let across = scene_distance(&scene, Vec3::new(2.0, 0.0, 2.0));
        assert!((across - (8f32.sqrt() - 1.0)).abs() < 1e-5);
    }

    #[test]
    fn box_matches_closed_form_outside_face_edge_and_inside() {
        let scene = [placed(Transform::IDENTITY, union(0.0))];

        assert!((scene_distance(&scene, Vec3::new(3.0, 0.0, 0.0)) - 2.0).abs() < 1e-5);

        let diagonal = scene_distance(&scene, Vec3::new(2.0, 2.0, 0.0));
        assert!((diagonal - 2f32.sqrt()).abs() < 1e-5);

        assert!((scene_distance(&scene, Vec3::ZERO) + 1.0).abs() < 1e-5);
    }

    #[test]
    fn uniform_scale_scales_the_distance() {
        let scene = [shaped(
            Transform::from_scale(Vec3::splat(2.0)),
            sphere_modifiers(),
        )];

        assert!((scene_distance(&scene, Vec3::new(5.0, 0.0, 0.0)) - 3.0).abs() < 1e-5);
    }

    #[test]
    fn the_modifiers_reach_every_shape_the_field_needs() {
        let unit_box = |modifiers| shaped(Transform::IDENTITY, modifiers);

        let plain = unit_box(Modifiers::default());
        assert!((shape_distance(&plain, Vec3::new(2.0, 0.0, 0.0)) - 1.0).abs() < 1e-4);

        let ball = unit_box(Modifiers {
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

        let bevelled = unit_box(Modifiers {
            bevel: 1.0,
            ..default()
        });
        let corner = shape_distance(&bevelled, Vec3::new(1.5, 0.0, 1.5));
        assert!(
            (corner - (4.5f32.sqrt() - 1.0)).abs() < 1e-4,
            "got {corner}"
        );

        assert!((shape_distance(&bevelled, Vec3::new(0.0, 2.0, 0.0)) - 1.0).abs() < 1e-4);

        let tapered = unit_box(Modifiers {
            cone: 1.0,
            ..default()
        });
        assert!(shape_distance(&tapered, Vec3::new(0.0, 1.0, 0.0)).abs() < 1e-3);
        assert!(shape_distance(&tapered, Vec3::new(1.0, -1.0, 0.0)).abs() < 1e-3);
        assert!(shape_distance(&tapered, Vec3::new(0.8, 0.5, 0.0)) > 0.0);

        let base_corner = shape_distance(&tapered, Vec3::new(1.0, -1.0, 1.0));
        assert!(
            base_corner.abs() < 1e-3,
            "base corner should be sharp, got {base_corner}"
        );

        let ridge = shaped(
            Transform::from_scale(Vec3::new(3.0, 1.0, 1.0)),
            Modifiers {
                cone: 1.0,
                ..default()
            },
        );

        assert!(shape_distance(&ridge, Vec3::new(2.0, 1.0, 0.0)).abs() < 1e-3);
        assert!(shape_distance(&ridge, Vec3::new(2.4, 1.0, 0.0)) > 0.0);

        assert!(shape_distance(&ridge, Vec3::new(1.0, 0.9, 0.0)) < 0.0);

        let both = unit_box(Modifiers {
            cone: 1.0,
            bevel: 1.0,
            ..default()
        });
        assert!(shape_distance(&both, Vec3::new(0.0, 1.0, 0.0)).abs() < 1e-3);
        assert!(shape_distance(&both, Vec3::new(1.0, -1.0, 0.0)).abs() < 1e-3);

        let hollow = unit_box(Modifiers {
            thickness: 0.5,
            ..default()
        });
        assert!(shape_distance(&hollow, Vec3::ZERO) > 0.0);
        assert!((shape_distance(&hollow, Vec3::new(2.0, 0.0, 0.0)) - 1.0).abs() < 1e-4);
        assert!(shape_distance(&hollow, Vec3::new(0.5, 0.0, 0.0)).abs() < 1e-3);
        assert!(shape_distance(&hollow, Vec3::new(0.75, 0.0, 0.0)) < 0.0);
        assert!(shape_distance(&hollow, Vec3::new(0.25, 0.0, 0.0)) > 0.0);

        assert!(shape_distance(&hollow, Vec3::new(0.0, 0.99, 0.0)) > 0.0);
        assert!(shape_distance(&hollow, Vec3::new(0.0, -0.99, 0.0)) > 0.0);

        let plate = shaped(
            Transform::from_scale(Vec3::new(1.0, 0.1, 1.0)),
            Modifiers {
                thickness: 0.5,
                ..default()
            },
        );
        assert!(shape_distance(&plate, Vec3::ZERO) > 0.0);
        assert!(shape_distance(&plate, Vec3::new(0.75, 0.0, 0.0)) < 0.0);

        let paper = unit_box(Modifiers {
            thickness: 0.0,
            ..default()
        });
        assert!(shape_distance(&paper, Vec3::ZERO) > 0.0);
        assert!(shape_distance(&paper, Vec3::new(0.99, 0.0, 0.0)).abs() < 0.02);

        let funnel = unit_box(Modifiers {
            cone: 0.5,
            thickness: 0.3,
            ..default()
        });
        assert!(shape_distance(&funnel, Vec3::new(0.0, -0.5, 0.0)) < 0.0);
        assert!(shape_distance(&funnel, Vec3::new(0.0, 0.95, 0.0)) > 0.0);
    }

    #[test]
    fn blend_modes_are_arrangements_of_the_three_booleans() {
        let mode = |mode| CsgOperation { mode, ..default() };

        let field = -1.0;
        let shape = -0.5;

        assert_eq!(blend(shape, field, &pack(mode(GPU_MODE_ADD)), false), field);
        assert_eq!(
            blend(shape, field, &pack(mode(GPU_MODE_INTERSECT)), false),
            shape
        );

        assert_eq!(
            blend(shape, field, &pack(mode(GPU_MODE_SUBTRACT)), false),
            0.5
        );

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
            chamfer: u32::from(operation.chamfer),
        }
    }

    #[test]
    fn rotation_is_applied_in_the_shapes_own_frame() {
        let scene = [placed(
            Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4)),
            union(0.0),
        )];

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
            Transform::from_scale(Vec3::new(4.0, 1.0, 1.0)),
            union(0.0),
        )];

        assert!((scene_distance(&scene, Vec3::new(6.0, 0.0, 0.0)) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn a_bevelled_box_is_a_cylinder_exact_on_side_cap_and_corner() {
        let cylinder = |scale| {
            shaped(
                Transform::from_scale(scale),
                Modifiers {
                    bevel: 1.0,
                    ..default()
                },
            )
        };

        let scene = [cylinder(Vec3::new(1.0, 2.0, 1.0))];

        assert!((scene_distance(&scene, Vec3::new(4.0, 0.0, 0.0)) - 3.0).abs() < 1e-5);

        assert!((scene_distance(&scene, Vec3::new(0.0, 5.0, 0.0)) - 3.0).abs() < 1e-5);

        let corner = scene_distance(&scene, Vec3::new(2.0, 3.0, 0.0));
        assert!((corner - 2f32.sqrt()).abs() < 1e-5);

        assert!((scene_distance(&scene, Vec3::ZERO) + 1.0).abs() < 1e-5);

        let tall = [cylinder(Vec3::new(2.0, 3.0, 2.0))];
        assert!((scene_distance(&tall, Vec3::new(5.0, 0.0, 0.0)) - 3.0).abs() < 1e-5);
        assert!((scene_distance(&tall, Vec3::new(0.0, 7.0, 0.0)) - 4.0).abs() < 1e-5);
    }

    #[test]
    fn sliding_turns_into_spin() {
        let (velocity_change, spin_change) = contact_friction(
            Vec3::Y,
            Vec3::X,
            Vec3::ZERO,
            0.5,
            10.0,
            FRICTION_COEFFICIENT,
        );

        assert!(velocity_change.x < 0.0);

        assert!(spin_change.z < 0.0);
    }

    #[test]
    fn rolling_without_slipping_is_left_alone() {
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

        assert!((separation - Vec3::new(-0.25, 0.0, 0.0)).length() < 1e-5);

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
            placed(Transform::IDENTITY, union(0.0)),
            placed(Transform::from_xyz(4.0, 0.0, 0.0), union(0.0)),
        ];
        assert!((scene_distance(&scene, Vec3::new(3.0, 0.0, 0.0)) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn blending_pulls_the_surface_outwards_between_two_shapes() {
        let apart = 1.6;
        let hard = [
            placed(Transform::IDENTITY, union(0.0)),
            placed(Transform::from_xyz(apart, 0.0, 0.0), union(0.0)),
        ];
        let blended = [
            hard[0].clone(),
            placed(Transform::from_xyz(apart, 0.0, 0.0), union(0.5)),
        ];

        let probe = Vec3::new(apart * 0.5, 1.2, 0.0);
        assert!(scene_distance(&blended, probe) < scene_distance(&hard, probe));
    }

    #[test]
    fn subtract_carves_a_hole() {
        let scene = [
            placed(Transform::from_scale(Vec3::splat(2.0)), union(0.0)),
            placed(
                Transform::IDENTITY,
                CsgOperation {
                    mode: GPU_MODE_SUBTRACT,
                    ..default()
                },
            ),
        ];

        assert!(scene_distance(&scene, Vec3::ZERO) > 0.0);

        assert!(scene_distance(&scene, Vec3::new(1.5, 0.0, 0.0)) < 0.0);
    }

    #[test]
    fn a_flag_reads_the_number_after_it_and_nothing_else() {
        use crate::command_line::value_in;

        let line: Vec<String> = ["idk", "bench", "spread:80", "--omega", "1.0", "--no-grid"]
            .iter()
            .map(|word| word.to_string())
            .collect();

        assert_eq!(value_in(&line, "--omega"), Some(1.0));

        assert_eq!(value_in(&line, "--grid"), None);

        assert_eq!(value_in(&line, "--no-grid"), None);

        let trailing = vec!["idk".to_string(), "--speed".to_string()];
        assert_eq!(value_in(&trailing, "--speed"), None);

        assert_eq!(value_in(&line, "--omeg"), None);
    }

    #[test]
    fn a_bound_key_drives_its_action_for_one_press_only() {
        use crate::game::input::{Action, InputPlugin};

        let mut app = App::new();

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
            .clear();
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

    #[test]
    fn box_culling_never_changes_the_field() {
        fn uncalled(shapes: &[GpuShape], point: Vec3) -> f32 {
            let mut field = MAX_MARCH_DISTANCE;
            for (index, shape) in shapes.iter().enumerate() {
                let distance = shape_distance(shape, point);
                field = if index == 0 {
                    distance
                } else {
                    blend(distance, field, &shape.blend, shape.blend.chamfer != 0)
                };
            }
            field
        }

        let mut state = 0x2545_F491_4F6C_DD1Du64;
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

        for _ in 0..200 {
            let count = 2 + (next() * 8.0) as usize;
            let shapes: Vec<GpuShape> = (0..count)
                .map(|_| {
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
                    };

                    let operation = CsgOperation {
                        mode: (next() * 9.0) as u32,
                        chamfer: next() < 0.5,
                        radius: next() * 0.8,
                        strength: next() * 0.5,
                    };
                    pack_brush(
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

    #[test]
    fn the_cull_fires_on_a_distant_add_and_never_on_another_mode() {
        let far = Vec3::new(30.0, 0.0, 0.0);
        let near_field = 1.0;

        let added = placed(Transform::IDENTITY, union(0.0));
        assert!(shape_cannot_reach(&added, far, near_field));

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
            let other = placed(Transform::IDENTITY, CsgOperation { mode, ..default() });
            assert!(
                !shape_cannot_reach(&other, far, near_field),
                "mode {mode} was culled without a proof that it is safe to"
            );
        }
    }

    #[test]
    fn the_cull_bound_holds_under_a_taper() {
        let shape = shaped(
            Transform::from_scale(Vec3::new(2.0, 3.0, 2.0)),
            Modifiers {
                cone: 1.0,
                ..default()
            },
        );

        let mut checked = 0;
        for step in 1..200 {
            let point = Vec3::new(3.0 + step as f32 * 0.03, step as f32 * 0.04 - 3.0, -0.2);
            let estimate = shape_distance(&shape, point);
            if estimate <= 0.0 {
                continue;
            }
            let bound =
                shape.cull_scale * cull_box_distance(point - shape.center, shape.cull_extent);
            assert!(
                bound <= estimate + 1e-4,
                "bound {bound} overshot the estimate {estimate} at {point:?}"
            );
            checked += 1;
        }
        assert!(checked > 50, "only {checked} points were outside the taper");
    }

    #[test]
    fn every_bench_count_fills_the_same_slab() {
        use crate::dev::benchmark::{SLAB_HALF_SIZE, cells_per_axis, grid_layout};

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

    #[test]
    fn spread_boxes_never_touch() {
        use crate::dev::benchmark::{SPREAD_HALF_SIZE, spread_layout};

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
            let shapes: Vec<GpuShape> = (0..2 + (next() * 6.0) as usize)
                .map(|_| {
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
                    pack_brush(
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
                    continue;
                }

                let evaluate = |point| scene_distance(&shapes, point);
                let (plain, plain_cost) =
                    march(&evaluate, &evaluate, origin, direction, 1.0, 0.001, 512);
                let (relaxed, relaxed_cost) =
                    march(&evaluate, &evaluate, origin, direction, 1.2, 0.001, 512);

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

        assert!(
            relaxed_steps < plain_steps,
            "relaxed spent {relaxed_steps} steps against plain's {plain_steps}"
        );
    }

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
                        pack_brush(
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
                    pack_brush(
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

    #[test]
    fn a_long_ray_inside_the_grid_still_arrives() {
        use crate::sdf::field::{build_grid, scene_bounds, scene_distance_gridded};
        const SHADER_BUDGET: u32 = 128;

        let cube_at = |position: Vec3| {
            shaped(
                Transform {
                    translation: position,
                    scale: Vec3::splat(0.8),
                    ..default()
                },
                Modifiers::default(),
            )
        };

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

                assert!(hit < 40.0, "the exact march missed its own target at x {x}");
                assert!(
                    (hit - grid_hit).abs() < 0.1,
                    "at x {x}: exact stopped at {hit}, gridded at {grid_hit} after {cost} steps"
                );
            }
        }
    }

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

        assert!(
            packed.direction.dot(Vec3::NEG_Y) > 0.99,
            "expected it to point down, got {:?}",
            packed.direction
        );

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

    #[test]
    fn the_soft_shadow_ratio_is_not_darkened_by_the_grid() {
        use crate::sdf::field::{build_grid, scene_bounds, scene_distance_gridded};

        const SOFTNESS: f32 = 12.0;
        const BIAS: f32 = 0.02;
        const STEPS: u32 = 48;

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
                Transform {
                    translation: Vec3::new(0.0, -0.5, 0.0),
                    scale: Vec3::new(20.0, 1.0, 20.0),
                    ..default()
                },
                Modifiers::default(),
            ),
            shaped(Transform::from_xyz(0.0, 1.5, 0.0), sphere_modifiers()),
        ];

        let (bounds_min, bounds_max) = scene_bounds(&shapes);
        let grid = build_grid(&shapes, bounds_min, bounds_max, 16);
        let exact = |point| scene_distance(&shapes, point);
        let gridded = |point| scene_distance_gridded(&shapes, &grid, point);

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

    #[test]
    fn the_shadow_proxy_bounds_the_field_and_still_lets_light_through() {
        use crate::sdf::field::{
            build_grid, scene_bounds, scene_distance_gridded, shadow_proxy_distance,
        };

        const SOFTNESS: f32 = 12.0;
        const BIAS: f32 = 0.02;
        const STEPS: u32 = 48;

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
                Transform {
                    translation: Vec3::new(0.0, -0.5, 0.0),
                    scale: Vec3::new(20.0, 1.0, 20.0),
                    ..default()
                },
                Modifiers::default(),
            ),
            shaped(Transform::from_xyz(0.0, 1.5, 0.0), sphere_modifiers()),
            shaped(
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
            shaped(
                Transform {
                    translation: Vec3::new(5.0, 1.2, -2.0),
                    scale: Vec3::new(2.0, 1.2, 0.6),
                    ..default()
                },
                Modifiers {
                    cone: 0.7,
                    ..default()
                },
            ),
        ];

        let (bounds_min, bounds_max) = scene_bounds(&shapes);
        let grid = build_grid(&shapes, bounds_min, bounds_max, 16);
        let gridded = |point| scene_distance_gridded(&shapes, &grid, point);
        let proxy = |point| shadow_proxy_distance(&shapes, &grid, point);

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

        assert!(
            outside > inside,
            "only {outside} of {} sample points were outside the geometry",
            outside + inside
        );

        assert!(
            loose > 0,
            "the proxy equalled the field at every one of {outside} points"
        );

        let sun = Vec3::Y;
        let beneath = Vec3::new(0.0, 0.3, 0.0);
        assert_eq!(penumbra(&gridded, beneath, sun, 40.0), 0.0);
        assert_eq!(penumbra(&proxy, beneath, sun, 40.0), 0.0);

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
