//! Cross-thread "run this on the UI thread" queue (the former toolkit's
//! `invoke_from_event_loop` / `upgrade_in_event_loop`).
use std::sync::{Mutex, OnceLock};

type Job = Box<dyn FnOnce() + Send>;

static QUEUE: Mutex<Vec<Job>> = Mutex::new(Vec::new());
static WAKER: OnceLock<acme_platform::Waker> = OnceLock::new();

pub fn set_waker(w: acme_platform::Waker) { let _ = WAKER.set(w); }

/// Queue `f` for the UI thread and wake the event loop.
pub fn invoke_from_event_loop(f: impl FnOnce() + Send + 'static) {
    QUEUE.lock().unwrap_or_else(|e| e.into_inner()).push(Box::new(f));
    if let Some(w) = WAKER.get() { w.wake(); }
}

/// Run the queued jobs (UI thread); true when any ran.
pub fn drain() -> bool {
    let jobs: Vec<Job> = std::mem::take(&mut *QUEUE.lock().unwrap_or_else(|e| e.into_inner()));
    let ran = !jobs.is_empty();
    for j in jobs { j(); }
    ran
}

#[cfg(test)]
mod tests {
    #[test]
    fn jobs_from_other_threads_run_on_drain() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || super::invoke_from_event_loop(move || tx.send(7).unwrap())).join().unwrap();
        assert!(super::drain());
        assert_eq!(rx.try_recv(), Ok(7));
        assert!(!super::drain());
    }
}
