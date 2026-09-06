use bevy::prelude::*;

use crate::game::scenes::SdfWorld;
use crate::sdf::brush::{Albedo, Brush, Modifiers};
use crate::sdf::field::{StaticBrushQuery, static_brushes_changed};

#[derive(Resource, Default)]
struct Answer {
    counted_before: usize,
    noticed: bool,
}

fn probe(
    root: Single<&Children, With<SdfWorld>>,
    statics: Query<StaticBrushQuery>,
    mut answer: ResMut<Answer>,
) {
    answer.noticed = static_brushes_changed(&root, &statics, answer.counted_before);
}

struct Harness {
    world: World,
    schedule: Schedule,
}

impl Harness {
    fn ask(&mut self, counted_before: usize) -> bool {
        self.world.resource_mut::<Answer>().counted_before = counted_before;
        self.schedule.run(&mut self.world);
        self.world.resource::<Answer>().noticed
    }
}

fn a_scene_of_two_statics() -> (Harness, Entity) {
    let mut world = World::new();
    world.init_resource::<Answer>();
    let statics: Vec<Entity> = [0.0f32, 3.0]
        .into_iter()
        .map(|x| {
            world
                .spawn((
                    Brush,
                    Transform::from_xyz(x, 0.0, 0.0),
                    GlobalTransform::default(),
                    Modifiers::default(),
                ))
                .id()
        })
        .collect();
    let second = statics[1];
    world
        .spawn((SdfWorld, Transform::default()))
        .add_children(&statics);

    let mut schedule = Schedule::default();
    schedule.add_systems(probe);
    (Harness { world, schedule }, second)
}

#[test]
fn a_settled_scene_is_not_repacked_but_every_kind_of_change_is_noticed() {
    let (mut scene, second) = a_scene_of_two_statics();

    assert!(scene.ask(0), "a scene seen for the first time must pack");
    assert!(
        !scene.ask(2),
        "nothing moved, so the statics must not be packed again",
    );

    scene
        .world
        .entity_mut(second)
        .insert(GlobalTransform::from_xyz(9.0, 0.0, 0.0));
    assert!(scene.ask(2), "a moved static must be noticed");
    assert!(!scene.ask(2), "and settle again afterwards");

    scene.world.entity_mut(second).insert(Albedo(Vec3::ONE));
    assert!(scene.ask(2), "a recoloured static must be noticed");
    assert!(!scene.ask(2), "and settle again afterwards");

    scene.world.entity_mut(second).despawn();
    assert!(scene.ask(2), "a despawned static must be noticed");
}
