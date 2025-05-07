use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use gramophone::net::websocket::Heartbeater;
use std::time::Duration;

fn heartbeat(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("should initialize tokio runtime");

    let hbr = rt.block_on(async {
        let mut hbr = black_box(Heartbeater::new(Duration::from_secs(1)));
        black_box(0..1000).for_each(|_| {
            hbr.record_sent();
            hbr.acknowledged();
        });
        hbr
    });

    c.bench_function("heartbeater.acknowledged(...) (1000 latencies)", |b| {
        b.iter_batched(
            || {
                let mut hbr = hbr.into_stats();
                hbr.record_sent();
                hbr
            },
            |hbr| black_box(hbr).acknowledged(),
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, heartbeat);
criterion_main!(benches);
