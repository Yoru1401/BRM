use bevy::prelude::*;

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
