use std::sync::Arc;

use kurbo::Affine;
use merc_lts::LabelledTransitionSystem;
use vello_cpu::Pixmap;
use vello_cpu::RenderContext;
use vello_cpu::Resources;
use vello_cpu::color::AlphaColor;
use vello_cpu::color::Srgb;
use vello_cpu::peniko::Fill;

use crate::Viewer;
use crate::vello_scene::VelloScene;

/// Handles the rendering of LTS graphs using the vello CPU renderer.
pub struct VelloCpuRenderer {
    scene: VelloScene,
    context: RenderContext,
    resources: Resources,
    pixmap: Pixmap,
}

impl VelloCpuRenderer {
    /// Creates a new renderer for the given LTS
    pub fn new(lts: Arc<LabelledTransitionSystem<String>>) -> Self {
        VelloCpuRenderer {
            scene: VelloScene::new(lts),
            context: RenderContext::new(1, 1),
            resources: Resources::new(),
            pixmap: Pixmap::new(1, 1),
        }
    }

    /// Renders the LTS graph and returns the pixels as tightly packed RGBA8 (premultiplied) bytes.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        viewer: &Viewer,
        draw_actions: bool,
        state_radius: f32,
        view_x: f32,
        view_y: f32,
        screen_x: u32,
        screen_y: u32,
        zoom_level: f32,
        label_text_size: f32,
    ) -> &[u8] {
        let (width, height) = (
            screen_x.clamp(1, u16::MAX as u32) as u16,
            screen_y.clamp(1, u16::MAX as u32) as u16,
        );
        if self.context.width() != width || self.context.height() != height {
            self.context = RenderContext::new(width, height);
            self.pixmap = Pixmap::new(width, height);
        } else {
            self.context.reset();
        }

        self.scene.update_labels(label_text_size);
        let geometry = self.scene.geometry(viewer, draw_actions, state_radius);
        let view = VelloScene::view_transform(view_x, view_y, screen_x, screen_y, zoom_level);

        let black = AlphaColor::<Srgb>::from_rgba8(0, 0, 0, 255);
        let white = AlphaColor::<Srgb>::from_rgba8(255, 255, 255, 255);
        let green = AlphaColor::<Srgb>::from_rgba8(100, 255, 100, 255);

        let ctx = &mut self.context;
        ctx.set_fill_rule(Fill::NonZero);
        ctx.set_stroke(kurbo::Stroke::new(1.0));

        // The white background
        ctx.set_transform(Affine::IDENTITY);
        ctx.set_paint(white);
        ctx.fill_rect(&kurbo::Rect::new(0.0, 0.0, screen_x as f64, screen_y as f64));

        ctx.set_transform(view);
        ctx.set_paint(black);
        ctx.stroke_path(&geometry.edges);
        ctx.fill_path(&geometry.arrows);

        for (label, position) in &geometry.labels {
            ctx.set_transform(view * Affine::translate((position.x as f64, position.y as f64)));
            ctx.fill_path(self.scene.label_outline(*label));
        }

        ctx.set_transform(view);
        ctx.set_paint(white);
        ctx.fill_path(&geometry.states);
        ctx.set_paint(green);
        ctx.fill_path(&geometry.initial_state);
        ctx.set_paint(black);
        ctx.stroke_path(&geometry.states);
        ctx.stroke_path(&geometry.initial_state);

        ctx.flush();
        ctx.render(&mut self.pixmap, &mut self.resources);

        self.pixmap.data_as_u8_slice()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use merc_lts::read_aut;

    use crate::VelloCpuRenderer;
    use crate::viewer::Viewer;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_vello_cpu_renderer() {
        let file = include_str!("../../../../examples/lts/abp.aut");
        let lts = Arc::new(read_aut(file.as_bytes()).unwrap());

        let viewer = Viewer::new(lts.clone());
        let mut renderer = VelloCpuRenderer::new(lts);

        let pixels = renderer.render(&viewer, true, 5.0, 0.0, 0.0, 800, 600, 1.0, 14.0);
        assert_eq!(pixels.len(), 800 * 600 * 4);
    }
}
