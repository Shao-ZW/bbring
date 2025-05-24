// modified from crossbeam
use bbring::RingBuffer;
use crossbeam_utils::thread::scope;

use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn smoke() {
    let q = RingBuffer::<i32, 4, 2>::new();
    q.push(7).unwrap();
    assert_eq!(q.pop(), Some(7));
    q.push(8).unwrap();
    assert_eq!(q.pop(), Some(8));
    assert!(q.pop().is_none());
}

#[test]
fn spsc() {
    #[cfg(miri)]
    const COUNT: usize = 50;
    #[cfg(not(miri))]
    const COUNT: usize = 100_000;

    let q = RingBuffer::<usize, 4, 2>::new();
    scope(|scope| {
        scope.spawn(|_| {
            for i in 0..COUNT {
                loop {
                    if let Some(x) = q.pop() {
                        assert_eq!(x, i);
                        break;
                    }
                }
            }
            assert!(q.pop().is_none());
        });

        scope.spawn(|_| {
            for i in 0..COUNT {
                println!("{}", i);
                while q.push(i).is_err() {}
            }
        });
    })
    .unwrap();
}

#[test]
fn spmc() {
    #[cfg(miri)]
    const COUNT: usize = 50;
    #[cfg(not(miri))]
    const COUNT: usize = 25_000;
    const CTHREADS: usize = 4;

    let q = RingBuffer::<usize, 4, 2>::new();
    let v = (0..COUNT).map(|_| AtomicUsize::new(0)).collect::<Vec<_>>();

    scope(|scope| {
        for _ in 0..CTHREADS {
            scope.spawn(|_| {
                for _ in 0..COUNT {
                    let n = loop {
                        if let Some(x) = q.pop() {
                            break x;
                        }
                    };
                    v[n].fetch_add(1, Ordering::SeqCst);
                }
            });
        }
        scope.spawn(|_| {
            for _ in 0..CTHREADS {
                for i in 0..COUNT {
                    while q.push(i).is_err() {}
                }
            }
        });
    })
    .unwrap();

    for c in v {
        assert_eq!(c.load(Ordering::SeqCst), CTHREADS);
    }
}

#[test]
fn mpsc() {
    #[cfg(miri)]
    const COUNT: usize = 50;
    #[cfg(not(miri))]
    const COUNT: usize = 25_000;
    const PTHREADS: usize = 8;

    let q = RingBuffer::<usize, 4, 2>::new();
    let v = (0..COUNT).map(|_| AtomicUsize::new(0)).collect::<Vec<_>>();

    scope(|scope| {
        scope.spawn(|_| {
            for _ in 0..COUNT * PTHREADS {
                let n = loop {
                    if let Some(x) = q.pop() {
                        break x;
                    }
                };
                v[n].fetch_add(1, Ordering::SeqCst);
            }
        });
        for _ in 0..PTHREADS {
            scope.spawn(|_| {
                for i in 0..COUNT {
                    while q.push(i).is_err() {}
                }
            });
        }
    })
    .unwrap();

    for c in v {
        assert_eq!(c.load(Ordering::SeqCst), PTHREADS);
    }
}

#[test]
fn mpmc() {
    #[cfg(miri)]
    const COUNT: usize = 50;
    #[cfg(not(miri))]
    const COUNT: usize = 25_000;
    const THREADS: usize = 4;

    let q = RingBuffer::<usize, 4, 2>::new();
    let v = (0..COUNT).map(|_| AtomicUsize::new(0)).collect::<Vec<_>>();

    scope(|scope| {
        for _ in 0..THREADS {
            scope.spawn(|_| {
                for _ in 0..COUNT {
                    let n = loop {
                        if let Some(x) = q.pop() {
                            break x;
                        }
                    };
                    v[n].fetch_add(1, Ordering::SeqCst);
                }
            });
        }
        for _ in 0..THREADS {
            scope.spawn(|_| {
                for i in 0..COUNT {
                    while q.push(i).is_err() {}
                }
            });
        }
    })
    .unwrap();

    for c in v {
        assert_eq!(c.load(Ordering::SeqCst), THREADS);
    }
}
