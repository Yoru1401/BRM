use bevy::prelude::*;

use crate::game::character::{Character, step_character};
use crate::sdf::adf::{self, Adf, Surface};

const STEP: f32 = 1.0 / 64.0;
const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);
const TOP: f32 = 2.0;

fn slab() -> Adf {
    let mut mesh = Mesh::from(Cuboid::new(24.0, 4.0, 24.0));
    mesh.duplicate_vertices();
    let mut triangles = adf::triangles_of(&mesh).0;
    for triangle in triangles.iter_mut() {
        for corner in triangle.iter_mut() {
            corner.y += TOP - 2.0;
        }
    }
    adf::bake(&Surface::new(&triangles, &[], &[]))
}

fn settled(field: &Adf, wish: Vec3, jump_for: usize, steps: usize) -> (Character, Vec3) {
    let mut character = Character::default();
    let mut position = Vec3::new(0.0, TOP + 4.0, 0.0);
    for tick in 0..steps {
        step_character(
            &mut character,
            &mut position,
            field,
            GRAVITY,
            wish,
            tick < jump_for,
            STEP,
        );
    }
    (character, position)
}

#[test]
fn the_ride_spring_settles_at_the_ride_height() {
    let field = slab();
    let (character, position) = settled(&field, Vec3::ZERO, 0, 400);

    let hover = position.y - TOP;
    assert!(
        (hover - 1.0).abs() < 0.15,
        "settled {hover} above the slab, wanted the 1.0 m ride height"
    );
    assert!(
        character.grounded && character.velocity.length() < 0.2,
        "still moving at {:?} after four hundred steps",
        character.velocity
    );
}

#[test]
fn walking_holds_the_ride_height_at_top_speed() {
    let field = slab();
    let mut character = Character::default();
    let mut position = Vec3::new(0.0, TOP + 4.0, 0.0);
    let walk = |character: &mut Character, position: &mut Vec3, wish, steps| {
        for _ in 0..steps {
            step_character(character, position, &field, GRAVITY, wish, false, STEP);
        }
    };
    walk(&mut character, &mut position, Vec3::ZERO, 200);
    let start = position.x;
    walk(&mut character, &mut position, Vec3::X, 64);

    let speed = character.velocity.with_y(0.0).length();
    assert!(speed > 7.0, "walked at {speed} m/s, wanted about 8");
    assert!(
        position.x - start > 5.0,
        "only travelled {} m in a second",
        position.x - start
    );
    assert!(
        (position.y - TOP - 1.0).abs() < 0.3,
        "walking lost the ride height: {}",
        position.y - TOP
    );
}

#[test]
fn a_held_jump_goes_higher_than_a_tapped_one() {
    let field = slab();
    let apex = |held: usize| {
        let mut character = Character::default();
        let mut position = Vec3::new(0.0, TOP + 1.0, 0.0);
        let mut highest = position.y;
        for tick in 0..120 {
            step_character(
                &mut character,
                &mut position,
                &field,
                GRAVITY,
                Vec3::ZERO,
                tick < held,
                STEP,
            );
            highest = highest.max(position.y);
        }
        (highest - TOP - 1.0, position.y - TOP)
    };

    let (tapped, tapped_end) = apex(2);
    let (full, full_end) = apex(60);

    assert!(tapped > 0.5, "a tap barely left the ride height: {tapped}");
    assert!(
        full > tapped * 1.15,
        "holding jump reached {full}, tapping reached {tapped}; the low-jump gravity did nothing"
    );
    for (name, landed) in [("tapped", tapped_end), ("held", full_end)] {
        assert!(
            (landed - 1.0).abs() < 0.2,
            "{name} jump did not come back to the ride height: {landed}"
        );
    }
}
