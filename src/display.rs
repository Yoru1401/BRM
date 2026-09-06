use bevy::{
    asset::RenderAssetUsages,
    camera::{ImageRenderTarget, RenderTarget, visibility::RenderLayers},
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode, WindowResolution},
};

use crate::command_line;
use crate::sdf::render::MainCamera;

const PRESENT_LAYER: usize = 2;

struct Preset {
    name: &'static str,
    width: u32,
    height: u32,
}

const PRESETS: [Preset; 7] = [
    Preset {
        name: "540p",
        width: 960,
        height: 540,
    },
    Preset {
        name: "720p",
        width: 1280,
        height: 720,
    },
    Preset {
        name: "900p",
        width: 1600,
        height: 900,
    },
    Preset {
        name: "1080p",
        width: 1920,
        height: 1080,
    },
    Preset {
        name: "1200p",
        width: 1920,
        height: 1200,
    },
    Preset {
        name: "1440p",
        width: 2560,
        height: 1440,
    },
    Preset {
        name: "4k",
        width: 3840,
        height: 2160,
    },
];

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct Display {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) fullscreen: bool,
    pub(crate) render_scale: f32,
}

impl Default for Display {
    fn default() -> Self {
        let named = command_line::text("--res").map(|asked| {
            let found = PRESETS.iter().find(|preset| preset.name == asked);
            match found {
                Some(preset) => (preset.width, preset.height),
                None => {
                    let names: Vec<&str> = PRESETS.iter().map(|preset| preset.name).collect();
                    eprintln!(
                        "res: unknown {asked:?}, expected one of {}",
                        names.join(", ")
                    );
                    std::process::exit(2);
                }
            }
        });
        let (width, height) = named.unwrap_or((1280, 720));

        Display {
            width: command_line::value("--width").map_or(width, |asked| asked as u32),
            height: command_line::value("--height").map_or(height, |asked| asked as u32),
            fullscreen: command_line::flag("--fullscreen"),
            render_scale: command_line::value("--render-scale")
                .unwrap_or(1.0)
                .clamp(0.1, 1.0),
        }
    }
}

impl Display {
    pub(crate) fn window(&self, title: &str, present_mode: PresentMode) -> Window {
        Window {
            present_mode,
            title: title.into(),
            resolution: WindowResolution::new(self.width, self.height),
            mode: match self.fullscreen {
                true => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                false => WindowMode::Windowed,
            },
            ..default()
        }
    }

    fn render_size(&self, target: UVec2) -> UVec2 {
        (target.as_vec2() * self.render_scale)
            .round()
            .as_uvec2()
            .max(UVec2::ONE)
    }
}

fn present_image(size: UVec2) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::linear();
    image
}

#[derive(Component)]
struct Presenter;

pub(crate) struct RenderScalePlugin(pub(crate) Display);

impl Plugin for RenderScalePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0)
            .add_systems(Update, start_scaled_pass);
    }
}

fn start_scaled_pass(
    mut commands: Commands,
    display: Res<Display>,
    mut images: ResMut<Assets<Image>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<Entity, (With<MainCamera>, Without<Presenter>)>,
    presenting: Query<(), With<Presenter>>,
) {
    if !presenting.is_empty() {
        return;
    }
    let target = UVec2::new(window.physical_width(), window.physical_height());
    if target.min_element() == 0 {
        return;
    }

    let rendered = display.render_size(target);
    let handle = images.add(present_image(rendered));

    commands
        .entity(*camera)
        .insert(RenderTarget::Image(ImageRenderTarget {
            handle: handle.clone(),
            scale_factor: 1.0,
        }));

    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            ..default()
        },
        RenderLayers::layer(PRESENT_LAYER),
        IsDefaultUiCamera,
        Presenter,
    ));

    commands.spawn((
        Sprite {
            image: handle,
            custom_size: Some(target.as_vec2()),
            ..default()
        },
        RenderLayers::layer(PRESENT_LAYER),
    ));

    let scale = display.render_scale;
    info!("render scale {scale}: marching {rendered} into a {target} window");
}
