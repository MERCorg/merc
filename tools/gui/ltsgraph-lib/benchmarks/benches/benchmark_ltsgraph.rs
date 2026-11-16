use std::hint::black_box;
use std::sync::Arc;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use merc_lts::read_aut;
use merc_ltsgraph_lib::GraphLayout;
use merc_ltsgraph_lib::SkiaRenderer;
use merc_ltsgraph_lib::VelloCpuRenderer;
use merc_ltsgraph_lib::Viewer;
use tiny_skia::Pixmap;
use tiny_skia::PixmapMut;

/// Render the alternating bit protocol with some settings, using the tiny-skia and vello CPU renderers.
pub fn criterion_benchmark_viewer(c: &mut Criterion) {
    let file = include_str!("../../../../../examples/lts/abp.aut");
    let lts = Arc::new(read_aut(file.as_bytes()).unwrap());

    let viewer = Viewer::new(lts.clone());
    let mut skia_renderer = SkiaRenderer::new(lts.clone());
    let mut vello_cpu_renderer = VelloCpuRenderer::new(lts);

    let mut pixel_buffer = Pixmap::new(800, 600).unwrap();

    for (name, draw_actions) in [("", true), (" (no text)", false)] {
        c.bench_function(&format!("ltsgraph viewer tiny-skia{name}"), |bencher| {
            bencher.iter(|| {
                skia_renderer.render(
                    &mut PixmapMut::from_bytes(pixel_buffer.data_mut(), 800, 600).unwrap(),
                    &viewer,
                    draw_actions,
                    5.0,
                    0.0,
                    0.0,
                    800,
                    600,
                    1.0,
                    14.0,
                );
            });
        });

        c.bench_function(&format!("ltsgraph viewer vello cpu{name}"), |bencher| {
            bencher.iter(|| {
                black_box(vello_cpu_renderer.render(&viewer, draw_actions, 5.0, 0.0, 0.0, 800, 600, 1.0, 14.0));
            });
        });
    }
}

/// Perform layouting the alternating bit protocol with some settings.
pub fn criterion_benchmark_layout(c: &mut Criterion) {
    let file = include_str!("../../../../../examples/lts/abp.aut");
    let lts = Arc::new(read_aut(file.as_bytes()).unwrap());

    let mut layout = GraphLayout::new(lts);

    c.bench_function("ltsgraph layout", |bencher| {
        bencher.iter(|| {
            layout.update(5.0, 1.0, 0.01);
        });
    });
}

criterion_group!(benches, criterion_benchmark_viewer, criterion_benchmark_layout,);
criterion_main!(benches);
