#[macro_use]
extern crate criterion;

use criterion::Criterion;

const JOBC: usize = 100000;

fn threadpool() {
    let pool = threadpool::ThreadPool::new(8);
    for _ in 0..JOBC {
        pool.execute(|| {
            let _ = 8 + 9;
        });
    }
    pool.join();
}

fn uvth_threadpool() {
    let pool = uvth::ThreadPool::new(8);
    for _ in 0..JOBC {
        pool.execute(|| {
            let _ = 8 + 9;
        });
    }
    pool.terminate();
    pool.join();
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("threadpool_crate", |b| b.iter(|| threadpool()));
    c.bench_function("threadpool_uvth", |b| b.iter(|| uvth_threadpool()));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
