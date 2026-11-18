//! `Output::Sync`: ordered or unordered access to the output of the worker threads.
//!
//! The C++ object had a mutex that the owner holds from `acquire()` to `release()`/`finish()`, a second mutex for the
//! completion bits and one condition variable. Here one mutex guards the owner flag and the completed slots, so
//! `cancel()` cannot lose a wakeup between a waiter's test of `last` and its wait. Which thread owns the output is
//! kept by the caller (the `owned` flag that `std::unique_lock::owns_lock` was), passed in and returned.

use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

#[cfg(loom)]
use loom::sync::{atomic::AtomicUsize, Condvar, Mutex, MutexGuard};
#[cfg(not(loom))]
use std::sync::{atomic::AtomicUsize, Condvar, Mutex, MutexGuard};

#[derive(Default)]
struct State {
    /// A thread owns the output.
    held: bool,
    /// ORDERED: slots above `last` whose threads finished without output.
    completed: BTreeSet<usize>,
}

pub struct OutputSync {
    ordered: bool,
    /// `Output::STOP`: `UNDEFINED_SIZE` of the C++ build, which depends on the size of `unsigned long`.
    stop: usize,
    state: Mutex<State>,
    turn: Condvar,
    /// ORDERED: the slot to output next, or `stop` after `cancel()`.
    last: AtomicUsize,
}

impl OutputSync {
    pub fn new(ordered: bool, stop: usize) -> Self {
        Self {
            ordered,
            stop,
            state: Mutex::new(State::default()),
            turn: Condvar::new(),
            last: AtomicUsize::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // a poisoned mutex cannot happen with panic=abort; recover the state anyway
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wait<'a>(&self, g: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.turn.wait(g).unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn last(&self) -> usize {
        self.last.load(Ordering::SeqCst)
    }

    /// `acquire(lock, slot)`: returns the new owner state, always true.
    pub fn acquire(&self, owned: bool, slot: usize) -> bool {
        if owned {
            return true;
        }
        let mut g = self.lock();
        if self.ordered {
            // wait for our turn, unless cancelled
            while g.held || (self.last() != self.stop && slot != self.last()) {
                g = self.wait(g);
            }
        } else {
            while g.held {
                g = self.wait(g);
            }
        }
        g.held = true;
        true
    }

    /// `try_acquire(lock)`: returns whether the caller owns the output.
    pub fn try_acquire(&self, owned: bool) -> bool {
        if owned || self.ordered {
            return owned;
        }
        let mut g = self.lock();
        if g.held {
            false
        } else {
            g.held = true;
            true
        }
    }

    fn unlock(&self) {
        self.lock().held = false;
        self.turn.notify_all();
    }

    /// `release(lock)`: UNORDERED drops the ownership, ORDERED keeps it until `finish()`. Returns the new owner state.
    pub fn release(&self, owned: bool) -> bool {
        if self.ordered {
            return owned;
        }
        if owned {
            self.unlock();
        }
        false
    }

    /// `finish(lock, slot)`: ORDERED hands the turn to the next slot. Returns the new owner state.
    pub fn finish(&self, owned: bool, slot: usize) -> bool {
        if !self.ordered {
            return owned;
        }
        let mut g = self.lock();
        let mut last = self.last();
        if last != self.stop && slot == last && !owned {
            while g.held {
                g = self.wait(g);
            }
            // cancel() may have run while the wait released the state mutex
            last = self.last();
        }
        if last == self.stop {
            if owned {
                g.held = false;
            }
            drop(g);
            self.turn.notify_all();
            false
        } else if slot == last {
            let mut next = last.wrapping_add(1);
            while g.completed.remove(&next) {
                next = next.wrapping_add(1);
            }
            self.last.store(next, Ordering::SeqCst);
            g.held = false;
            drop(g);
            self.turn.notify_all();
            false
        } else {
            // threads without output may run ahead but must mark off their completion
            if slot > last {
                g.completed.insert(slot);
            }
            owned
        }
    }

    /// `cancel()`: all threads waiting for their turn proceed.
    pub fn cancel(&self) {
        let g = self.lock();
        self.last.store(self.stop, Ordering::SeqCst);
        drop(g);
        self.turn.notify_all();
    }

    pub fn cancelled(&self) -> bool {
        self.last() == self.stop
    }

    /// Releases the output when the owner's lock object goes away (the `std::unique_lock` destructor).
    pub fn drop_owner(&self, owned: bool) {
        if owned {
            self.unlock();
        }
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    const STOP: usize = usize::MAX;

    /// Threads take their slots in a scrambled start order and must write in slot order.
    #[test]
    fn ordered_slots_write_in_order() {
        for _ in 0..200 {
            let sync = Arc::new(OutputSync::new(true, STOP));
            let log = Arc::new(StdMutex::new(Vec::new()));
            let threads: Vec<_> = (0..8usize)
                .rev()
                .map(|slot| {
                    let (sync, log) = (sync.clone(), log.clone());
                    std::thread::spawn(move || {
                        // odd slots have no output and only mark their completion
                        let mut owned = false;
                        if slot % 2 == 0 {
                            owned = sync.acquire(owned, slot);
                            log.lock().unwrap().push(slot);
                        }
                        sync.finish(owned, slot);
                    })
                })
                .collect();
            for t in threads {
                t.join().unwrap();
            }
            assert_eq!(*log.lock().unwrap(), vec![0, 2, 4, 6]);
            assert_eq!(sync.last(), 8);
        }
    }

    #[test]
    fn cancel_wakes_waiters() {
        for _ in 0..200 {
            let sync = Arc::new(OutputSync::new(true, STOP));
            let t = {
                let sync = sync.clone();
                std::thread::spawn(move || {
                    let owned = sync.acquire(false, 5);
                    sync.finish(owned, 5);
                })
            };
            sync.cancel();
            t.join().unwrap();
            assert!(sync.cancelled());
        }
    }

    #[test]
    fn finish_does_not_undo_cancel() {
        for _ in 0..200 {
            let sync = Arc::new(OutputSync::new(true, STOP));
            let owned = sync.acquire(false, 0);
            let finisher = {
                let sync = sync.clone();
                std::thread::spawn(move || {
                    sync.finish(false, 0);
                })
            };
            std::thread::sleep(std::time::Duration::from_micros(200));
            sync.cancel();
            sync.drop_owner(owned);
            finisher.join().unwrap();
            assert!(sync.cancelled());
        }
    }

    #[test]
    fn unordered_is_exclusive() {
        let sync = Arc::new(OutputSync::new(false, STOP));
        let counter = Arc::new(StdMutex::new(0usize));
        let inside = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let (sync, counter, inside) = (sync.clone(), counter.clone(), inside.clone());
                std::thread::spawn(move || {
                    for _ in 0..500 {
                        let owned = sync.acquire(false, 0);
                        assert_eq!(inside.fetch_add(1, Ordering::SeqCst), 0);
                        *counter.lock().unwrap() += 1;
                        inside.fetch_sub(1, Ordering::SeqCst);
                        assert!(!sync.release(owned));
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(*counter.lock().unwrap(), 2000);
    }

    #[test]
    fn try_acquire_semantics() {
        let u = OutputSync::new(false, STOP);
        assert!(u.try_acquire(false));
        assert!(!u.try_acquire(false));
        assert!(!u.release(true));
        let o = OutputSync::new(true, STOP);
        assert!(!o.try_acquire(false));
        assert!(o.release(true));
    }
}

#[cfg(loom)]
mod loom_tests {
    use super::*;
    use loom::sync::Arc;

    const STOP: usize = usize::MAX;

    /// All interleavings: slot 1 (with output) waits for slot 0 (no output), and both finish.
    #[test]
    fn ordered_turns() {
        loom::model(|| {
            let sync = Arc::new(OutputSync::new(true, STOP));
            let t = {
                let sync = sync.clone();
                loom::thread::spawn(move || {
                    let owned = sync.acquire(false, 1);
                    sync.finish(owned, 1);
                })
            };
            sync.finish(false, 0);
            t.join().unwrap();
            assert_eq!(sync.last(), 2);
        });
    }

    /// A waiter for a turn that never comes must be released by cancel(), whatever the interleaving.
    #[test]
    fn cancel_releases_waiter() {
        loom::model(|| {
            let sync = Arc::new(OutputSync::new(true, STOP));
            let t = {
                let sync = sync.clone();
                loom::thread::spawn(move || {
                    let owned = sync.acquire(false, 3);
                    sync.finish(owned, 3);
                })
            };
            sync.cancel();
            t.join().unwrap();
            assert!(sync.cancelled());
        });
    }

    /// finish() waits for the owner while cancel() runs: the stop must survive, whatever the interleaving.
    #[test]
    fn finish_does_not_undo_cancel() {
        loom::model(|| {
            let sync = Arc::new(OutputSync::new(true, STOP));
            let owned = sync.acquire(false, 0);
            let finisher = {
                let sync = sync.clone();
                loom::thread::spawn(move || {
                    sync.finish(false, 0);
                })
            };
            let canceller = {
                let sync = sync.clone();
                loom::thread::spawn(move || sync.cancel())
            };
            sync.drop_owner(owned);
            finisher.join().unwrap();
            canceller.join().unwrap();
            assert!(sync.cancelled());
        });
    }

    #[test]
    fn unordered_exclusive() {
        use loom::sync::atomic::AtomicUsize as Inside;
        loom::model(|| {
            let sync = Arc::new(OutputSync::new(false, STOP));
            let inside = Arc::new(Inside::new(0));
            let t = {
                let (sync, inside) = (sync.clone(), inside.clone());
                loom::thread::spawn(move || {
                    let owned = sync.acquire(false, 0);
                    assert_eq!(inside.fetch_add(1, Ordering::SeqCst), 0);
                    inside.fetch_sub(1, Ordering::SeqCst);
                    sync.release(owned);
                })
            };
            let owned = sync.acquire(false, 0);
            assert_eq!(inside.fetch_add(1, Ordering::SeqCst), 0);
            inside.fetch_sub(1, Ordering::SeqCst);
            sync.release(owned);
            t.join().unwrap();
        });
    }
}