use std::sync::Arc;

use kurbo::Affine;
use merc_lts::LabelledTransitionSystem;
use vello::AaConfig;
use vello::RenderParams;
use vello::Renderer;
use vello::RendererOptions;
use vello::Scene;
use vello::peniko::Color;
use vello::peniko::Fill;
use vello::wgpu;

use crate::Viewer;
use crate::vello_scene::VelloScene;

/// Handles the rendering of LTS graphs using the vello GPU renderer.
pub struct VelloRenderer {
    scene: VelloScene,
    vello_scene: Scene,
    renderer: Renderer,
}

impl VelloRenderer {
    /// Creates a new renderer for the given LTS, for rendering on the given device.
    pub fn new(lts: Arc<LabelledTransitionSystem<String>>, device: &wgpu::Device) -> Result<Self, vello::Error> {
        let renderer = Renderer::new(
            device,
            RendererOptions {
                antialiasing_support: vello::AaSupport::area_only(),
                ..Default::default()
            },
        )?;

        Ok(VelloRenderer {
            scene: VelloScene::new(lts),
            vello_scene: Scene::new(),
            renderer,
        })
    }

    /// Renders the LTS graph into the given texture, which must be `Rgba8Unorm` with `STORAGE_BINDING` usage.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::TextureView,
        viewer: &Viewer,
        draw_actions: bool,
        state_radius: f32,
        view_x: f32,
        view_y: f32,
        screen_x: u32,
        screen_y: u32,
        zoom_level: f32,
        label_text_size: f32,
    ) -> Result<(), vello::Error> {
        self.scene.update_labels(label_text_size);
        let geometry = self.scene.geometry(viewer, draw_actions, state_radius);
        let view = VelloScene::view_transform(view_x, view_y, screen_x, screen_y, zoom_level);

        let black = Color::from_rgba8(0, 0, 0, 255);
        let white = Color::from_rgba8(255, 255, 255, 255);
        let green = Color::from_rgba8(100, 255, 100, 255);
        let stroke = kurbo::Stroke::new(1.0);

        let scene = &mut self.vello_scene;
        scene.reset();

        scene.stroke(&stroke, view, black, None, &geometry.edges);
        scene.fill(Fill::NonZero, view, black, None, &geometry.arrows);

        for (label, position) in &geometry.labels {
            let transform = view * Affine::translate((position.x as f64, position.y as f64));
            scene.fill(Fill::NonZero, transform, black, None, self.scene.label_outline(*label));
        }

        scene.fill(Fill::NonZero, view, white, None, &geometry.states);
        scene.fill(Fill::NonZero, view, green, None, &geometry.initial_state);
        scene.stroke(&stroke, view, black, None, &geometry.states);
        scene.stroke(&stroke, view, black, None, &geometry.initial_state);

        self.renderer.render_to_texture(
            device,
            queue,
            &self.vello_scene,
            texture,
            &RenderParams {
                base_color: white,
                width: screen_x,
                height: screen_y,
                antialiasing_method: AaConfig::Area,
            },
        )
    }
}
