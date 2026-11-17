//! UI-thread timers (replace the former toolkit's `Timer`). Callbacks run from the event loop's
//! background tick, outside any `App` borrow, via [`fire_due`].
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerMode { SingleShot, Repeated }

struct Slot {
    deadline: Instant,
    period: Option<Duration>,
    generation: u64,
    cb: Option<Box<dyn FnMut()>>,
}

#[derive(Default)]
struct Registry { next: u64, generation: u64, slots: HashMap<u64, Slot> }

thread_local! { static REG: RefCell<Registry> = RefCell::new(Registry::default()); }

/// A restartable timer; dropping it stops it.
#[derive(Default)]
pub struct Timer { id: Cell<Option<u64>> }

impl Timer {
    /// (Re)start the timer; a running timer is restarted with the new callback.
    pub fn start(&self, mode: TimerMode, interval: Duration, cb: impl FnMut() + 'static) {
        let (id, replaced) = REG.with(|r| {
            let mut r = r.borrow_mut();
            let id = match self.id.get() { Some(id) => id, None => { r.next += 1; r.next } };
            r.generation += 1;
            let generation = r.generation;
            let old = r.slots.insert(id, Slot {
                deadline: Instant::now() + interval,
                period: (mode == TimerMode::Repeated).then_some(interval.max(Duration::from_millis(1))),
                generation,
                cb: Some(Box::new(cb)),
            });
            (id, old)
        });
        self.id.set(Some(id));
        // the replaced callback may own timers: drop it outside the registry borrow
        drop(replaced);
    }

    pub fn stop(&self) {
        // try_with: timers may be dropped during thread-local destruction at exit
        if let Some(id) = self.id.take() {
            let removed = REG.try_with(|r| r.borrow_mut().slots.remove(&id)).ok().flatten();
            drop(removed);
        }
    }

    pub fn running(&self) -> bool {
        self.id.get().is_some_and(|id| REG.try_with(|r| r.borrow().slots.contains_key(&id)).unwrap_or(false))
    }

    /// Run `cb` once after `d` (not cancellable, like the old `Timer::single_shot`).
    pub fn single_shot(d: Duration, cb: impl FnOnce() + 'static) {
        let mut cb = Some(cb);
        let t = Timer::default();
        t.start(TimerMode::SingleShot, d, move || { if let Some(f) = cb.take() { f() } });
        // detach: the slot lives on in the registry until it fires
        t.id.set(None);
    }
}

impl Drop for Timer { fn drop(&mut self) { self.stop(); } }

/// Drop every pending timer (end of `main`, before thread-local teardown);
/// callbacks are dropped outside the registry borrow.
pub fn clear_all() {
    let slots = REG.try_with(|r| std::mem::take(&mut r.borrow_mut().slots)).unwrap_or_default();
    drop(slots);
}

/// Time until the earliest deadline, rounded down into a few buckets so the
/// platform's poll interval does not change on every call; None when idle.
pub fn next_interval() -> Option<Duration> {
    let earliest = REG.with(|r| r.borrow().slots.values().map(|s| s.deadline).min())?;
    let d = earliest.saturating_duration_since(Instant::now());
    Some(Duration::from_millis(if d >= Duration::from_millis(250) { 250 } else if d >= Duration::from_millis(50) { 50 } else { 10 }))
}

/// Run every due callback; true when at least one ran.
pub fn fire_due() -> bool {
    let now = Instant::now();
    let due: Vec<(u64, u64)> = REG.with(|r| {
        let mut v: Vec<(Instant, u64, u64)> = r.borrow().slots.iter().filter(|(_, s)| s.deadline <= now && s.cb.is_some()).map(|(&id, s)| (s.deadline, id, s.generation)).collect();
        v.sort();
        v.into_iter().map(|(_, id, g)| (id, g)).collect()
    });
    let mut ran = false;
    for (id, generation) in due {
        // take the callback out so it may start/stop timers (including itself)
        let taken = REG.with(|r| {
            let mut r = r.borrow_mut();
            let s = r.slots.get_mut(&id).filter(|s| s.generation == generation)?;
            let cb = s.cb.take()?;
            let single = s.period.is_none();
            let removed = if single { r.slots.remove(&id) } else { None };
            Some((cb, removed))
        });
        let Some((mut cb, removed)) = taken else { continue };
        drop(removed);
        cb();
        ran = true;
        // re-arm a repeating timer; a callback that is not kept is dropped outside the borrow
        let unused = REG.with(|r| {
            let mut r = r.borrow_mut();
            match r.slots.get_mut(&id).filter(|s| s.generation == generation) {
                Some(s) => {
                    if let Some(p) = s.period {
                        s.deadline = (s.deadline + p).max(Instant::now());
                        s.cb = Some(cb);
                        None
                    } else {
                        Some(cb)
                    }
                }
                None => Some(cb),
            }
        });
        drop(unused);
    }
    ran
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn single_shot_repeated_stop_and_restart() {
        let hits = Rc::new(Cell::new(0));
        let h = hits.clone();
        Timer::single_shot(Duration::ZERO, move || h.set(h.get() + 1));
        let rep = Timer::default();
        let h = hits.clone();
        rep.start(TimerMode::Repeated, Duration::from_millis(1), move || h.set(h.get() + 10));
        let stopped = Timer::default();
        let h = hits.clone();
        stopped.start(TimerMode::SingleShot, Duration::ZERO, move || h.set(h.get() + 1000));
        stopped.stop();
        assert!(!stopped.running() && rep.running());
        assert!(next_interval().is_some());
        std::thread::sleep(Duration::from_millis(3));
        assert!(fire_due());
        assert_eq!(hits.get(), 11);
        assert!(rep.running(), "repeated timers re-arm");
        std::thread::sleep(Duration::from_millis(3));
        fire_due();
        assert_eq!(hits.get(), 21);
        drop(rep);
        assert_eq!(next_interval(), None);
    }

    #[test]
    fn callback_can_restart_its_own_timer() {
        let t = Rc::new(Timer::default());
        let n = Rc::new(Cell::new(0));
        let (t2, n2) = (t.clone(), n.clone());
        t.start(TimerMode::SingleShot, Duration::ZERO, move || {
            n2.set(n2.get() + 1);
            let n3 = n2.clone();
            t2.start(TimerMode::SingleShot, Duration::ZERO, move || n3.set(n3.get() + 1));
        });
        std::thread::sleep(Duration::from_millis(1));
        fire_due();
        assert_eq!(n.get(), 1);
        assert!(t.running());
        std::thread::sleep(Duration::from_millis(1));
        fire_due();
        assert_eq!(n.get(), 2);
        assert!(!t.running());
    }
}
