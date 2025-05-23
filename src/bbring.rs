use crossbeam_utils::{Backoff, CachePadded};

use core::cell::UnsafeCell;
use core::cmp::max;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

// BLOCK_NUM and BLOCK_SIZE must be power of 2
// only implement retry-new mode now
pub struct RingBuffer<T, const BLOCK_NUM: usize, const BLOCK_SIZE: usize> {
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    blocks: [Block<T, BLOCK_SIZE>; BLOCK_NUM],
    one_lap: usize,
}

unsafe impl<T: Send, const BLOCK_NUM: usize, const BLOCK_SIZE: usize> Send
    for RingBuffer<T, BLOCK_NUM, BLOCK_SIZE>
{
}
unsafe impl<T: Send, const BLOCK_NUM: usize, const BLOCK_SIZE: usize> Sync
    for RingBuffer<T, BLOCK_NUM, BLOCK_SIZE>
{
}

struct Block<T, const BLOCK_SIZE: usize> {
    allocated: CachePadded<AtomicUsize>,
    committed: CachePadded<AtomicUsize>, // Actually counter
    reserved: CachePadded<AtomicUsize>,
    consumed: CachePadded<AtomicUsize>, // Actually counter
    slots: [UnsafeCell<MaybeUninit<T>>; BLOCK_SIZE],

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

impl<T, const BLOCK_NUM: usize, const BLOCK_SIZE: usize> RingBuffer<T, BLOCK_NUM, BLOCK_SIZE> {
    pub fn new() -> Self {
        // better error handle
        if !BLOCK_NUM.is_power_of_two() || !BLOCK_SIZE.is_power_of_two() {
            panic!("must be power of two")
        }

        // may be bug! the overflow of cursor index is a problem
        let one_lap = max(BLOCK_NUM, BLOCK_SIZE << 1);

        Self {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            blocks: core::array::from_fn(|i| {
                if i == 0 {
                    Block::<T, BLOCK_SIZE>::new(one_lap, 0)
                } else {
                    Block::<T, BLOCK_SIZE>::new(one_lap, BLOCK_SIZE)
                }
            }),
            one_lap,
        }
    }

    pub fn push(&self, mut value: T) -> Result<(), T> {
        let backoff = Backoff::new();

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
                            backoff.spin();
                            // backoff.snooze();
                        }
                        AdvanceHeadResult::Success => {}
                    }
                }
            }
        }
    }

    pub fn pop(&self) -> Option<T> {
        let backoff = Backoff::new();
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
                    backoff.spin();
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
        BLOCK_NUM * BLOCK_SIZE
    }

    fn advance_head(&self, old_head: usize) -> AdvanceHeadResult {
        let old_blk_idx = old_head & (self.one_lap - 1);
        let old_head_vsn = old_head & !(self.one_lap - 1);

        let next_blk = &self.blocks[(old_blk_idx + 1) % BLOCK_NUM];

        let next_blk_consumed = next_blk.consumed.load(Ordering::SeqCst);
        let consumed_cnt = next_blk_consumed & (self.one_lap - 1);
        let consumed_vsn = next_blk_consumed & !(self.one_lap - 1);

        if consumed_vsn < old_head_vsn
            || (consumed_vsn == old_head_vsn && consumed_cnt != BLOCK_SIZE)
        {
            let next_blk_reserved = next_blk.reserved.load(Ordering::SeqCst);
            let reserved_idx = next_blk_reserved & (self.one_lap - 1);

            if reserved_idx == consumed_cnt {
                return AdvanceHeadResult::NoEntry;
            } else {
                return AdvanceHeadResult::NotAvaliable;
            }
        }

        // sequence matter?
        next_blk
            .committed
            .fetch_max(old_head_vsn.wrapping_add(self.one_lap), Ordering::SeqCst);
        next_blk
            .allocated
            .fetch_max(old_head_vsn.wrapping_add(self.one_lap), Ordering::SeqCst);

        // wow!
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

        if committed_vsn != old_tail_vsn + 1 {
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

impl<T, const BLOCK_SIZE: usize> Block<T, BLOCK_SIZE> {
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
        if self.allocated.load(Ordering::SeqCst) & (self.one_lap - 1) >= BLOCK_SIZE {
            return CommitResult::BlockDone(value);
        }

        // overflow problem ? won't cause version add?
        // what if many threads get here and do add?
        // really bug!!!
        let old = self.allocated.fetch_add(1, Ordering::SeqCst);

        if old >= BLOCK_SIZE {
            return CommitResult::BlockDone(value);
        }

        unsafe {
            self.slots[old].get().write(MaybeUninit::new(value));
        }
        self.committed.fetch_add(1, Ordering::SeqCst);
        return CommitResult::Success;
    }

    fn try_consume(&self) -> ConsumeResult<T> {
        loop {
            let reserved = self.reserved.load(Ordering::SeqCst);
            let reserved_idx = reserved & (self.one_lap - 1);

            if reserved_idx < BLOCK_SIZE {
                let committed = self.committed.load(Ordering::SeqCst);
                let committed_cnt = committed & (self.one_lap - 1);

                if reserved_idx == committed_cnt {
                    return ConsumeResult::NoEntry;
                }

                if committed_cnt != BLOCK_SIZE {
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

impl<T, const BLOCK_NUM: usize, const BLOCK_SIZE: usize> Drop
    for RingBuffer<T, BLOCK_NUM, BLOCK_SIZE>
{
    fn drop(&mut self) {}
}
