use criterion::{Criterion, criterion_group, criterion_main};

use clawcode::tui::{App, render_to_test_backend};

fn first_frame_in_process(c: &mut Criterion) {
    c.bench_function("first_frame_in_process_test_backend_120x40", |b| {
        b.iter(|| render_to_test_backend(&App::default(), 120, 40).unwrap())
    });
}

criterion_group!(benches, first_frame_in_process);
criterion_main!(benches);
