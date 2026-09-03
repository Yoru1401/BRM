use bevy::prelude::*;

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
