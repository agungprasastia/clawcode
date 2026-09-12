use clawcode::persistence::{Db, WriterHandle};
use clawcode::provider::{ProviderStream, StreamEvent};
use clawcode::tui::{App, UiEvent, UiEventQueue, render_to_test_backend};
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn app_start_and_render(c: &mut Criterion) {
    c.bench_function("startup_local_app_and_first_render", |b| {
        b.iter(|| {
            let app = App::default();
            render_to_test_backend(black_box(&app), 120, 40).unwrap();
        });
    });
}

fn stream_coalescing(c: &mut Criterion) {
    c.bench_function("stream_coalescing_1024_deltas", |b| {
        b.iter(|| {
            let (sender, mut stream) = ProviderStream::channel(16);
            for _ in 0..1024 {
                sender.send(StreamEvent::TextDelta("delta".into())).unwrap();
            }
            sender.flush().unwrap();
            black_box(stream.next());
        });
    });
}

fn sqlite_writer_batch(c: &mut Criterion) {
    c.bench_function("sqlite_writer_batch_64_messages", |b| {
        b.iter(|| {
            let db = Db::open_in_memory().unwrap();
            let session = db.create_session("benchmark").unwrap();
            let writer = WriterHandle::spawn(db);
            for _ in 0..64 {
                writer
                    .try_append(session.id, "user", "benchmark message")
                    .unwrap();
            }
            writer.flush();
            black_box(writer.shutdown());
        });
    });
}

fn redraw_and_bounded_queue(c: &mut Criterion) {
    c.bench_function("redraw_120x40_with_bounded_stream", |b| {
        b.iter(|| {
            let mut app = App::default();
            let mut queue = UiEventQueue::new(64);
            for _ in 0..2048 {
                queue.push(UiEvent::StreamDelta("stream delta".into()));
            }
            app.apply_pending(&mut queue);
            render_to_test_backend(black_box(&app), 120, 40).unwrap();
        });
    });
}

criterion_group!(
    benches,
    app_start_and_render,
    stream_coalescing,
    sqlite_writer_batch,
    redraw_and_bounded_queue
);
criterion_main!(benches);
