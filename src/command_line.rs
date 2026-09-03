pub(crate) fn flag(name: &str) -> bool {
    std::env::args().any(|argument| argument == name)
}

pub(crate) fn value(name: &str) -> Option<f32> {
    value_in(&std::env::args().collect::<Vec<_>>(), name)
}

pub(crate) fn value_in(arguments: &[String], name: &str) -> Option<f32> {
    let at = arguments.iter().position(|argument| argument == name)?;
    arguments.get(at + 1)?.parse().ok()
}

pub(crate) fn text(name: &str) -> Option<String> {
    let arguments: Vec<String> = std::env::args().collect();
    let at = arguments.iter().position(|argument| argument == name)?;
    arguments.get(at + 1).cloned()
}

pub(crate) fn positional(index: usize) -> Option<String> {
    std::env::args().nth(index + 1)
}
