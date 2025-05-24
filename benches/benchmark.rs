use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bbring::RingBuffer;
use crossbeam_queue::ArrayQueue;

const QUEUE_CAPACITY: usize = 4096;
const NUM_OPERATIONS: usize = 100_000_000;
const NUM_THREADS: usize = 10;

fn bench_spsc(c: &mut Criterion) {
    let mut group = c.benchmark_group("SPSC");
    group.throughput(Throughput::Elements(NUM_OPERATIONS as u64));

    group.bench_function("ArrayQueue_SPSC", |b| {
        b.iter(|| {
            let queue = Arc::new(ArrayQueue::new(QUEUE_CAPACITY));

            let q_clone = Arc::clone(&queue);
            let producer = thread::spawn(move || {
                for i in 0..NUM_OPERATIONS {
                    loop {
                        if q_clone.push(black_box(i)).is_ok() {
                            break;
                        } else {
                            thread::yield_now();
                        }
                    }
                }
            });

            let q_clone = Arc::clone(&queue);
            let consumer = thread::spawn(move || {
                let mut consumed_count = 0;
                while consumed_count < NUM_OPERATIONS {
                    if q_clone.pop().is_some() {
                        consumed_count += 1;
                    } else {
                        thread::yield_now();
                    }
                }
            });

            producer.join().unwrap();

            consumer.join().unwrap();
        });
    });

    group.bench_function("BBQ_SPSC", |b| {
        b.iter(|| {
            let queue = Arc::new(RingBuffer::<usize, 64, 64>::new());

            let q_clone = Arc::clone(&queue);
            let producer = thread::spawn(move || {
                for i in 0..NUM_OPERATIONS {
                    loop {
                        if q_clone.push(black_box(i)).is_ok() {
                            break;
                        } else {
                            thread::yield_now();
                        }
                    }
                }
            });

            let q_clone = Arc::clone(&queue);
            let consumer = thread::spawn(move || {
                let mut consumed_count = 0;
                while consumed_count < NUM_OPERATIONS {
                    if q_clone.pop().is_some() {
                        consumed_count += 1;
                    } else {
                        thread::yield_now();
                    }
                }
            });

            producer.join().unwrap();

            consumer.join().unwrap();
        });
    });

    group.finish();
}

fn bench_mpsc(c: &mut Criterion) {
    let mut group = c.benchmark_group("MPSC");
    group.throughput(Throughput::Elements(NUM_OPERATIONS as u64));

    group.bench_function("ArrayQueue_MPSC", |b| {
        b.iter(|| {
            let queue = Arc::new(ArrayQueue::new(QUEUE_CAPACITY));

            let producer_chunk_size = NUM_OPERATIONS / NUM_THREADS;
            let mut producers = Vec::new();
            for _ in 0..NUM_THREADS {
                let q_clone = Arc::clone(&queue);
                producers.push(thread::spawn(move || {
                    for i in 0..producer_chunk_size {
                        loop {
                            if q_clone.push(black_box(i)).is_ok() {
                                break;
                            }
                            thread::yield_now();
                        }
                    }
                }));
            }

            let q_clone = Arc::clone(&queue);
            let consumer = thread::spawn(move || {
                let mut consumed_count = 0;
                while consumed_count < NUM_OPERATIONS {
                    if q_clone.pop().is_some() {
                        consumed_count += 1;
                    } else {
                        thread::yield_now();
                    }
                }
            });

            for p in producers {
                p.join().unwrap();
            }
            consumer.join().unwrap();
        });
    });

    group.bench_function("BBQ_MPSC", |b| {
        b.iter(|| {
            let queue = Arc::new(RingBuffer::<usize, 64, 64>::new());

            let producer_chunk_size = NUM_OPERATIONS / NUM_THREADS;
            let mut producers = Vec::new();
            for _ in 0..NUM_THREADS {
                let q_clone = Arc::clone(&queue);
                producers.push(thread::spawn(move || {
                    for i in 0..producer_chunk_size {
                        loop {
                            if q_clone.push(black_box(i)).is_ok() {
                                break;
                            }
                            thread::yield_now();
                        }
                    }
                }));
            }

            let q_clone = Arc::clone(&queue);
            let consumer = thread::spawn(move || {
                let mut consumed_count = 0;
                while consumed_count < NUM_OPERATIONS {
                    if q_clone.pop().is_some() {
                        consumed_count += 1;
                    } else {
                        thread::yield_now();
                    }
                }
            });

            for p in producers {
                p.join().unwrap();
            }
            consumer.join().unwrap();
        });
    });

    group.finish();
}

fn bench_spmc(c: &mut Criterion) {
    let mut group = c.benchmark_group("SPMC");
    group.throughput(Throughput::Elements(NUM_OPERATIONS as u64));

    group.bench_function("ArrayQueue_SPMC", |b| {
        b.iter(|| {
            let queue = Arc::new(ArrayQueue::new(QUEUE_CAPACITY));

            let consumer_chunk_size = NUM_OPERATIONS / NUM_THREADS;
            let mut consumers = Vec::new();
            for _ in 0..NUM_THREADS {
                let q_clone = Arc::clone(&queue);
                consumers.push(thread::spawn(move || {
                    let mut consumed_count = 0;
                    while consumed_count < consumer_chunk_size {
                        if q_clone.pop().is_some() {
                            consumed_count += 1;
                        } else {
                            thread::yield_now();
                        }
                    }
                }));
            }

            let q_clone = Arc::clone(&queue);
            let producer = thread::spawn(move || {
                for i in 0..NUM_OPERATIONS {
                    loop {
                        if q_clone.push(black_box(i)).is_ok() {
                            break;
                        }
                        thread::yield_now();
                    }
                }
            });

            producer.join().unwrap();
            for c in consumers {
                c.join().unwrap();
            }
        });
    });

    group.bench_function("BBQ_SPMC", |b| {
        b.iter(|| {
            let queue = Arc::new(RingBuffer::<usize, 64, 64>::new());
            let consumer_chunk_size = NUM_OPERATIONS / NUM_THREADS;
            let mut consumers = Vec::new();
            for _ in 0..NUM_THREADS {
                let q_clone = Arc::clone(&queue);
                consumers.push(thread::spawn(move || {
                    let mut consumed_count = 0;
                    while consumed_count < consumer_chunk_size {
                        if q_clone.pop().is_some() {
                            consumed_count += 1;
                        } else {
                            thread::yield_now();
                        }
                    }
                }));
            }

            let q_clone = Arc::clone(&queue);
            let producer = thread::spawn(move || {
                for i in 0..NUM_OPERATIONS {
                    loop {
                        if q_clone.push(black_box(i)).is_ok() {
                            break;
                        }
                        thread::yield_now();
                    }
                }
            });

            producer.join().unwrap();
            for c in consumers {
                c.join().unwrap();
            }
        });
    });

    group.finish();
}

fn bench_mpmc(c: &mut Criterion) {
    let mut group = c.benchmark_group("MPMC");
    group.throughput(Throughput::Elements(NUM_OPERATIONS as u64));

    group.bench_function("ArrayQueue_MPMC", |b| {
        b.iter(|| {
            let queue = Arc::new(ArrayQueue::new(QUEUE_CAPACITY));

            let producer_chunk_size = NUM_OPERATIONS / NUM_THREADS;
            let consumer_chunk_size = NUM_OPERATIONS / NUM_THREADS;

            let mut producers = Vec::new();
            let mut consumers = Vec::new();

            for _ in 0..NUM_THREADS {
                let q_clone = Arc::clone(&queue);
                producers.push(thread::spawn(move || {
                    for i in 0..producer_chunk_size {
                        loop {
                            if q_clone.push(black_box(i)).is_ok() {
                                break;
                            }
                            thread::yield_now();
                        }
                    }
                }));
            }

            for _ in 0..NUM_THREADS {
                let q_clone = Arc::clone(&queue);
                consumers.push(thread::spawn(move || {
                    let mut consumed_count = 0;
                    while consumed_count < consumer_chunk_size {
                        if q_clone.pop().is_some() {
                            consumed_count += 1;
                        } else {
                            thread::yield_now();
                        }
                    }
                }));
            }

            for p in producers {
                p.join().unwrap();
            }
            for c in consumers {
                c.join().unwrap();
            }
        });
    });

    group.bench_function("BBQ_MPMC", |b| {
        b.iter(|| {
            let queue = Arc::new(RingBuffer::<usize, 64, 64>::new());

            let producer_chunk_size = NUM_OPERATIONS / NUM_THREADS;
            let consumer_chunk_size = NUM_OPERATIONS / NUM_THREADS;

            let mut producers = Vec::new();
            let mut consumers = Vec::new();

            for _ in 0..NUM_THREADS {
                let q_clone = Arc::clone(&queue);
                producers.push(thread::spawn(move || {
                    for i in 0..producer_chunk_size {
                        loop {
                            if q_clone.push(black_box(i)).is_ok() {
                                break;
                            }
                            thread::yield_now();
                        }
                    }
                }));
            }

            for _ in 0..NUM_THREADS {
                let q_clone = Arc::clone(&queue);
                consumers.push(thread::spawn(move || {
                    let mut consumed_count = 0;
                    while consumed_count < consumer_chunk_size {
                        if q_clone.pop().is_some() {
                            consumed_count += 1;
                        } else {
                            thread::yield_now();
                        }
                    }
                }));
            }

            for p in producers {
                p.join().unwrap();
            }
            for c in consumers {
                c.join().unwrap();
            }
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).measurement_time(Duration::from_secs(100));
    // targets = bench_spsc, bench_mpsc, bench_spmc, bench_mpmc
    // targets = bench_spsc
    // targets = bench_mpsc
    // targets = bench_spmc
    targets = bench_mpmc
}
criterion_main!(benches);
