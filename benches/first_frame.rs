use criterion::{Criterion, criterion_group, criterion_main};

use clawcode::tui::{App, render_to_test_backend};

fn first_frame(c: &mut Criterion) {
    c.bench_function("first_frame_120x40", |b| {
        b.iter(|| render_to_test_backend(&App::default(), 120, 40).unwrap())
    });
}

criterion_group!(benches, first_frame);
criterion_main!(benches);
