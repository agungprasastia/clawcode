use criterion::{Criterion, criterion_group, criterion_main};

use clawcode::tui::{App, render_to_test_backend};

fn render_frame(c: &mut Criterion) {
    c.bench_function("render_frame_to_test_backend_120x40", |b| {
        b.iter(|| render_to_test_backend(&App::default(), 120, 40).unwrap())
    });
}

criterion_group!(benches, render_frame);
criterion_main!(benches);
