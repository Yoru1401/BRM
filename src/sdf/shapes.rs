pub(crate) const SPHERE: u32 = 0;
pub(crate) const CAPSULE: u32 = 1;

pub(crate) const GENERATED_PATH: &str = "shaders/shapes.generated.wgsl";

struct Primitive {
    name: &'static str,
    distance: &'static str,
    normal: &'static str,
}

const PRIMITIVES: [Primitive; 2] = [
    Primitive {
        name: "sphere",
        distance: "return length(world_position - body.start) - body.radius;",
        normal: "return normalize(world_position - body.start);",
    },
    Primitive {
        name: "capsule",
        distance: "let along = body.end - body.start;
    let from_start = world_position - body.start;
    let fraction = clamp(dot(from_start, along) / max(dot(along, along), 1e-8), 0.0, 1.0);
    return length(from_start - along * fraction) - body.radius;",
        normal: "let along = body.end - body.start;
    let from_start = world_position - body.start;
    let fraction = clamp(dot(from_start, along) / max(dot(along, along), 1e-8), 0.0, 1.0);
    return normalize(from_start - along * fraction);",
    },
];

pub(crate) fn wgsl() -> String {
    let mut source = String::from(
        "#define_import_path idk::shapes\n#import \"shaders/bindings.wgsl\"::{Dynamic}\n\nconst SHAPE_KIND_SHIFT: u32 = 16u;\n\n",
    );

    for primitive in &PRIMITIVES {
        source.push_str(&emit("distance", "f32", primitive.name, primitive.distance));
        source.push_str(&emit(
            "normal",
            "vec3<f32>",
            primitive.name,
            primitive.normal,
        ));
    }

    source.push_str(&dispatch("distance", "f32", "1e30"));
    source.push_str(&dispatch("normal", "vec3<f32>", "vec3<f32>(0.0, 1.0, 0.0)"));
    source
}

fn emit(role: &str, returns: &str, name: &str, body: &str) -> String {
    format!("fn {name}_{role}(world_position: vec3<f32>, body: Dynamic) -> {returns} {{\n    {body}\n}}\n\n")
}

fn dispatch(role: &str, returns: &str, fallback: &str) -> String {
    let arms: String = PRIMITIVES
        .iter()
        .enumerate()
        .map(|(kind, primitive)| {
            format!(
                "        case {kind}u: {{ return {}_{role}(world_position, body); }}\n",
                primitive.name
            )
        })
        .collect();
    format!(
        "fn shape_{role}(world_position: vec3<f32>, body: Dynamic) -> {returns} {{\n    \
         switch body.flags >> SHAPE_KIND_SHIFT {{\n{arms}        \
         default: {{ return {fallback}; }}\n    }}\n}}\n\n"
    )
}
