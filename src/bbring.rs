use crossbeam_utils::{Backoff, CachePadded};

use core::cell::UnsafeCell;
use core::cmp::max;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

// BLOCK_NUM and SLOT_NUM must be power of 2
// only implement retry-new mode now
pub struct RingBuffer<T, const BLOCK_NUM: usize, const SLOT_NUM: usize> {
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    blocks: [Block<T, SLOT_NUM>; BLOCK_NUM],

    one_lap: usize,
}

unsafe impl<T: Send, const BLOCK_NUM: usize, const SLOT_NUM: usize> Send
    for RingBuffer<T, BLOCK_NUM, SLOT_NUM>
{
}
unsafe impl<T: Send, const BLOCK_NUM: usize, const SLOT_NUM: usize> Sync
    for RingBuffer<T, BLOCK_NUM, SLOT_NUM>
{
}

struct Block<T, const SLOT_NUM: usize> {
    allocated: CachePadded<AtomicUsize>,
    committed: CachePadded<AtomicUsize>, // Actually counter
    reserved: CachePadded<AtomicUsize>,
    consumed: CachePadded<AtomicUsize>, // Actually counter
    slots: [UnsafeCell<MaybeUninit<T>>; SLOT_NUM],

    one_lap: usize,
}

enum CommitResult<T> {
    Success,
    BlockDone(T),
}

enum ConsumeResult<T> {
    NoEntry,
    NotAvaliable,
    BlockDone,
    Success(T),
}

enum AdvanceHeadResult {
    Success,
    NoEntry,
    NotAvaliable,
}

enum AdvanceTailReault {
    NoEntry,
    Success,
}

impl<T, const BLOCK_NUM: usize, const SLOT_NUM: usize> RingBuffer<T, BLOCK_NUM, SLOT_NUM> {
    pub fn new() -> Self {
        // better error handle
        if !BLOCK_NUM.is_power_of_two() || !SLOT_NUM.is_power_of_two() {
            panic!("must be power of two")
        }

        // may be bug! the overflow of cursor index is a problem
        let one_lap = max(BLOCK_NUM, SLOT_NUM << 1);

        Self {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            blocks: core::array::from_fn(|i| {
                if i == 0 {
                    Block::<T, SLOT_NUM>::new(one_lap, 0)
                } else {
                    Block::<T, SLOT_NUM>::new(one_lap, SLOT_NUM)
                }
            }),
            one_lap,
        }
    }

    pub fn push(&self, mut value: T) -> Result<(), T> {
        // let backoff = Backoff::new();

        loop {
            let head = self.head.load(Ordering::SeqCst);
            let blk_idx = head & (self.one_lap - 1);

            match self.blocks[blk_idx].try_commit(value) {
                CommitResult::Success => return Ok(()),
                CommitResult::BlockDone(val) => {
                    value = val;
                    match self.advance_head(head) {
                        AdvanceHeadResult::NoEntry => return Err(value),
                        AdvanceHeadResult::NotAvaliable => {
                            // backoff.spin();
                            // backoff.snooze();
                        }
                        AdvanceHeadResult::Success => {}
                    }
                }
            }
        }
    }

    pub fn pop(&self) -> Option<T> {
        // let backoff = Backoff::new();

        loop {
            let tail = self.tail.load(Ordering::SeqCst);
            let blk_idx = tail & (self.one_lap - 1);

            match self.blocks[blk_idx].try_consume() {
                ConsumeResult::BlockDone => match self.advance_tail(tail) {
                    AdvanceTailReault::NoEntry => return None,
                    AdvanceTailReault::Success => {}
                },
                ConsumeResult::NoEntry => return None,
                ConsumeResult::NotAvaliable => {
                    // backoff.spin();
                    // backoff.snooze();
                }
                ConsumeResult::Success(val) => return Some(val),
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        todo!()
    }

    pub fn is_full(&self) -> bool {
        todo!()
    }

    pub fn len(&self) -> usize {
        todo!()
    }

    pub fn capacity(&self) -> usize {
        BLOCK_NUM * SLOT_NUM
    }

    fn advance_head(&self, old_head: usize) -> AdvanceHeadResult {
        let old_blk_idx = old_head & (self.one_lap - 1);
        let old_head_vsn = old_head & !(self.one_lap - 1);

        let next_blk = &self.blocks[(old_blk_idx + 1) % BLOCK_NUM];

        let next_blk_consumed = next_blk.consumed.load(Ordering::SeqCst);
        let consumed_cnt = next_blk_consumed & (self.one_lap - 1);
        let consumed_vsn = next_blk_consumed & !(self.one_lap - 1);

        // buggy! what if old_head_vsn overflow
        if consumed_vsn < old_head_vsn || (consumed_vsn == old_head_vsn && consumed_cnt != SLOT_NUM)
        {
            let next_blk_reserved = next_blk.reserved.load(Ordering::SeqCst);
            let reserved_idx = next_blk_reserved & (self.one_lap - 1);

            if reserved_idx == consumed_cnt {
                return AdvanceHeadResult::NoEntry;
            } else {
                return AdvanceHeadResult::NotAvaliable;
            }
        }

        next_blk
            .committed
            .fetch_max(old_head_vsn.wrapping_add(self.one_lap), Ordering::SeqCst);
        next_blk
            .allocated
            .fetch_max(old_head_vsn.wrapping_add(self.one_lap), Ordering::SeqCst);

        let new_head = if old_blk_idx + 1 < BLOCK_NUM {
            // Same lap, incremented index.
            old_head + 1
        } else {
            // One lap forward, index wraps around to zero.
            old_head_vsn.wrapping_add(self.one_lap)
        };
        self.head.fetch_max(new_head, Ordering::SeqCst);
        AdvanceHeadResult::Success
    }

    fn advance_tail(&self, old_tail: usize) -> AdvanceTailReault {
        let old_blk_idx = old_tail & (self.one_lap - 1);
        let old_tail_vsn = old_tail & !(self.one_lap - 1);

        let next_blk = &self.blocks[(old_blk_idx + 1) % BLOCK_NUM];
        let next_blk_committed = next_blk.committed.load(Ordering::SeqCst);
        let committed_vsn = next_blk_committed & !(self.one_lap - 1);

        if committed_vsn != old_tail_vsn.wrapping_add(self.one_lap) {
            return AdvanceTailReault::NoEntry;
        }

        next_blk
            .consumed
            .fetch_max(old_tail_vsn.wrapping_add(self.one_lap), Ordering::SeqCst);
        next_blk
            .reserved
            .fetch_max(old_tail_vsn.wrapping_add(self.one_lap), Ordering::SeqCst);

        let new_tail = if old_blk_idx + 1 < BLOCK_NUM {
            // Same lap, incremented index.
            old_tail + 1
        } else {
            // One lap forward, index wraps around to zero.
            old_tail_vsn.wrapping_add(self.one_lap)
        };
        self.tail.fetch_max(new_tail, Ordering::SeqCst);
        AdvanceTailReault::Success
    }
}

impl<T, const SLOT_NUM: usize> Block<T, SLOT_NUM> {
    fn new(one_lap: usize, initial: usize) -> Self {
        Self {
            allocated: CachePadded::new(AtomicUsize::new(initial)),
            committed: CachePadded::new(AtomicUsize::new(initial)),
            reserved: CachePadded::new(AtomicUsize::new(initial)),
            consumed: CachePadded::new(AtomicUsize::new(initial)),
            slots: core::array::from_fn(|_| UnsafeCell::new(MaybeUninit::uninit())),
            one_lap,
        }
    }

    fn try_commit(&self, value: T) -> CommitResult<T> {
        // // annoyying part is here
        // // In fact in retry-new mode you the allocated-version actually not matter
        // // what need prevent is for example, ringbuffer config is BLOCK_NUM = 4, SLOT_NUM = 2,
        // // so the one_clp is 4, if 4 threads get this func the same time

        // let allocated = self.allocated.load(Ordering::SeqCst);
        // let committed = self.committed.load(Ordering::SeqCst);

        // if allocated & (self.one_lap - 1) >= SLOT_NUM {
        //     return CommitResult::BlockDone(value);
        // }

        // let allocated_vsn = allocated & !(self.one_lap - 1);

        // let prev_allocated = self.allocated.fetch_add(1, Ordering::SeqCst);
        // let prev_allocated_idx = prev_allocated & (self.one_lap - 1);
        // let prev_allocated_vsn = prev_allocated & !(self.one_lap - 1);

        // if prev_allocated_idx >= SLOT_NUM {
        //     return CommitResult::BlockDone(value);
        // }

        // // this is also buggy!
        // // what if the version add 1 and overflow to 0 but better than last version
        // // Not quite sure about the correctness in this way!!! check and check
        // if prev_allocated_vsn > allocated_vsn {
        //     // version need backoff here !!!
        //     println!("fuck");
        //     panic!();
        //     return CommitResult::BlockDone(value);
        // }

        // unsafe {
        //     self.slots[prev_allocated_idx]
        //         .get()
        //         .write(MaybeUninit::new(value));
        // }

        // let commm = self.committed.fetch_add(1, Ordering::SeqCst);
        // println!(
        //     "try commit {} {} {} {} success old commited {} ",
        //     self.id, prev_allocated_idx, prev_allocated_vsn, allocated_vsn, commm
        // );
        // return CommitResult::Success;

        // Attention!!!
        // The performace must be worse than FAA but for the correctness i use this first
        loop {
            let allocated = self.allocated.load(Ordering::SeqCst);
            let allocated_idx = allocated & (self.one_lap - 1);

            if allocated_idx >= SLOT_NUM {
                return CommitResult::BlockDone(value);
            }

            if self.allocated.fetch_max(allocated + 1, Ordering::SeqCst) == allocated {
                unsafe {
                    self.slots[allocated_idx]
                        .get()
                        .write(MaybeUninit::new(value));
                }
                self.committed.fetch_add(1, Ordering::SeqCst);
                return CommitResult::Success;
            }
        }
    }

    fn try_consume(&self) -> ConsumeResult<T> {
        loop {
            let reserved = self.reserved.load(Ordering::SeqCst);
            let reserved_idx = reserved & (self.one_lap - 1);

            if reserved_idx < SLOT_NUM {
                let committed = self.committed.load(Ordering::SeqCst);
                let committed_cnt = committed & (self.one_lap - 1);

                if reserved_idx == committed_cnt {
                    return ConsumeResult::NoEntry;
                }

                if committed_cnt != SLOT_NUM {
                    let allocated = self.allocated.load(Ordering::SeqCst);
                    let allocated_idx = allocated & (self.one_lap - 1);

                    if allocated_idx != committed_cnt {
                        return ConsumeResult::NotAvaliable;
                    }
                }

                if self.reserved.fetch_max(reserved + 1, Ordering::SeqCst) == reserved {
                    let data = unsafe { self.slots[reserved_idx].get().read().assume_init() };
                    self.consumed.fetch_add(1, Ordering::SeqCst);
                    return ConsumeResult::Success(data);
                }
            } else {
                return ConsumeResult::BlockDone;
            }
        }
    }
}
