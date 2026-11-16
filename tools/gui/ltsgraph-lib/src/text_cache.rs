use cosmic_text::Attrs;
use cosmic_text::Buffer;
use cosmic_text::FontSystem;
use cosmic_text::Metrics;
use cosmic_text::Shaping;
use cosmic_text::SwashCache;
use tiny_skia::PathBuilder;
use tiny_skia::PixmapMut;
use tiny_skia::PixmapPaint;
use tiny_skia::Transform;

pub(crate) struct TextCache {
    /// A FontSystem provides access to detected system fonts, create one per application
    font_system: FontSystem,

    /// A SwashCache stores rasterized glyphs, create one per application
    swash_cache: SwashCache,
}

impl Default for TextCache {
    fn default() -> TextCache {
        TextCache::new()
    }
}

impl TextCache {
    pub(crate) fn new() -> TextCache {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();

        TextCache {
            font_system,
            swash_cache,
        }
    }

    /// Font metrics indicate the font size and line height of a buffer
    pub(crate) fn create_buffer(&mut self, text: &str, font_metrics: Metrics) -> Buffer {
        // A Buffer provides shaping and layout for a UTF-8 string, create one per text widget
        let mut buffer = Buffer::new(&mut self.font_system, font_metrics);

        // Set a size for the text buffer, in pixels
        buffer.set_size(None, None);

        // Attributes indicate what font to choose.
        let attrs = Attrs::new();

        // Add some text!
        buffer.set_text(text, &attrs, Shaping::Advanced, None);

        // Perform shaping as desired
        buffer.shape_until_scroll(&mut self.font_system, true);
        buffer
    }

    /// Resizes the font metrics of the buffer.
    pub(crate) fn resize(&mut self, buffer: &mut Buffer, font_metrics: Metrics) {
        buffer.set_metrics(font_metrics);
        buffer.shape_until_scroll(&mut self.font_system, true);
    }

    /// Returns the vector outline of the buffer (without bitmap glyphs), with the origin at the top left of the text.
    pub(crate) fn outline(&mut self, buffer: &Buffer) -> kurbo::BezPath {
        let mut path = kurbo::BezPath::new();
        let point = |x: f32, y: f32| kurbo::Point::new(x as f64, y as f64);

        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let physical_glyph = glyph.physical((0., 0.), 1.0);
                let Some(outline) = self
                    .swash_cache
                    .get_outline_commands(&mut self.font_system, physical_glyph.cache_key)
                else {
                    continue;
                };

                let mut glyph_path = kurbo::BezPath::new();
                for command in outline {
                    match *command {
                        cosmic_text::Command::MoveTo(p0) => glyph_path.move_to(point(p0.x, p0.y)),
                        cosmic_text::Command::LineTo(p0) => glyph_path.line_to(point(p0.x, p0.y)),
                        cosmic_text::Command::CurveTo(p0, p1, p2) => {
                            glyph_path.curve_to(point(p0.x, p0.y), point(p1.x, p1.y), point(p2.x, p2.y))
                        }
                        cosmic_text::Command::QuadTo(p0, p1) => {
                            glyph_path.quad_to(point(p0.x, p0.y), point(p1.x, p1.y))
                        }
                        cosmic_text::Command::Close => glyph_path.close_path(),
                    }
                }

                // Glyph outlines are y-up, while the layout is y-down.
                let transform = kurbo::Affine::translate((physical_glyph.x as f64, physical_glyph.y as f64))
                    * kurbo::Affine::scale_non_uniform(1.0, -1.0);
                path.extend((transform * glyph_path).elements().iter().copied());
            }
        }

        path
    }

    /// Draw the given cached text at the given location.
    pub(crate) fn draw(&mut self, buffer: &Buffer, pixmap: &mut PixmapMut, transform: Transform) {
        let paint = tiny_skia::Paint::default();

        // Draw the buffer
        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let physical_glyph = glyph.physical((0., 0.), 1.0);

                // Try to get the font outline, which we can draw directly with tiny-skia.
                if let Some(outline) = self
                    .swash_cache
                    .get_outline_commands(&mut self.font_system, physical_glyph.cache_key)
                {
                    let mut path_builder = PathBuilder::new();

                    for command in outline {
                        match *command {
                            cosmic_text::Command::MoveTo(p0) => {
                                path_builder.move_to(p0.x, p0.y);
                            }
                            cosmic_text::Command::LineTo(p0) => {
                                path_builder.line_to(p0.x, p0.y);
                            }
                            cosmic_text::Command::CurveTo(p0, p1, p2) => {
                                path_builder.cubic_to(p0.x, p0.y, p1.x, p1.y, p2.x, p2.y);
                            }
                            cosmic_text::Command::Close => {
                                path_builder.close();
                            }
                            cosmic_text::Command::QuadTo(p0, p1) => {
                                path_builder.quad_to(p0.x, p0.y, p1.x, p1.y);
                            }
                        }
                    }

                    if let Some(path) = path_builder.finish() {
                        pixmap.fill_path(
                            &path,
                            &paint,
                            tiny_skia::FillRule::Winding,
                            Transform::from_translate(physical_glyph.x as f32, physical_glyph.y as f32)
                                .pre_scale(1.0, -1.0)
                                .post_concat(transform),
                            None,
                        );
                    }
                } else {
                    // Otherwise render the rasterized image (bitmap/emoji glyphs) using skia.
                    if let Some(image) = self
                        .swash_cache
                        .get_image(&mut self.font_system, physical_glyph.cache_key)
                    {
                        // tiny-skia pixmaps are always premultiplied RGBA8, but swash hands back
                        // different layouts depending on the glyph. Normalize to RGBA8 so that
                        // mask (single-channel coverage) glyphs do not get misinterpreted as RGBA
                        // (which previously panicked the `from_bytes` unwrap on a length mismatch).
                        let mut rgba: Vec<u8> = match image.content {
                            cosmic_text::SwashContent::Mask => {
                                image.data.iter().flat_map(|&coverage| [0, 0, 0, coverage]).collect()
                            }
                            cosmic_text::SwashContent::SubpixelMask | cosmic_text::SwashContent::Color => {
                                image.data.clone()
                            }
                        };

                        if let Some(pixmap_image) =
                            PixmapMut::from_bytes(&mut rgba, image.placement.width, image.placement.height)
                        {
                            pixmap.draw_pixmap(0, 0, pixmap_image.as_ref(), &PixmapPaint::default(), transform, None);
                        }
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use cosmic_text::Metrics;
    use tiny_skia::Pixmap;
    use tiny_skia::PixmapMut;
    use tiny_skia::Transform;

    use crate::text_cache::TextCache;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_textcache() {
        let mut cache = TextCache::new();

        // Create a simple text label and resize it.
        let mut buffer = cache.create_buffer("A test label", Metrics::new(14.0, 14.0));
        cache.resize(&mut buffer, Metrics::new(50.0, 50.0));

        let mut pixel_buffer = Pixmap::new(800, 600).unwrap();
        cache.draw(
            &buffer,
            &mut PixmapMut::from_bytes(pixel_buffer.data_mut(), 800, 600).unwrap(),
            Transform::default(),
        );
    }
}
