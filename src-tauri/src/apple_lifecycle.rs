//! Coordinate the render worker with UIKit before background GPU access stops.
use anyhow::Result;
use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::Duration,
};

struct State {
    active: bool,
    next_worker: u64,
    workers: HashMap<u64, bool>,
}
struct Activity {
    state: Mutex<State>,
    changed: Condvar,
}
impl Activity {
    fn new(active: bool) -> Self {
        Self {
            state: Mutex::new(State {
                active,
                next_worker: 0,
                workers: HashMap::new(),
            }),
            changed: Condvar::new(),
        }
    }
    fn change(&self, active: bool) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.active = active;
        self.changed.notify_all();
        if !active {
            // WillResignActive is delivered before background entry. Give the
            // worker time to drain its bounded GPU/VT queues, keeping UIKit's
            // transition bounded even if the worker is still loading a chart.
            let (state, result) = self
                .changed
                .wait_timeout_while(state, Duration::from_millis(1500), |s| {
                    !s.active && s.workers.values().any(|quiesced| !quiesced)
                })
                .unwrap_or_else(|e| e.into_inner());
            if result.timed_out() && state.workers.values().any(|idle| !idle) {
                log::info!("render: background transition while worker is finishing setup");
            }
        }
    }
    fn begin(self: &Arc<Self>) -> RenderActivity {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let id = state.next_worker;
        state.next_worker += 1;
        state.workers.insert(id, false);
        RenderActivity {
            activity: Arc::clone(self),
            id,
        }
    }
}
fn activity() -> &'static Arc<Activity> {
    static ACTIVITY: OnceLock<Arc<Activity>> = OnceLock::new();
    ACTIVITY.get_or_init(|| Arc::new(Activity::new(true)))
}

#[cfg(target_os = "ios")]
pub fn wait_until_active() {
    let mut state = activity().state.lock().unwrap_or_else(|e| e.into_inner());
    while !state.active {
        state = activity()
            .changed
            .wait(state)
            .unwrap_or_else(|e| e.into_inner());
    }
}

/// Invoked synchronously by the native UIKit observer, before background entry.
#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn phi_tkc_render_lifecycle(active: i32) {
    // A Swift/C caller must never receive a Rust unwind.
    let _ = std::panic::catch_unwind(|| {
        if active == 0 {
            // Wake a low-FPS preview before the synchronous GPU checkpoint.
            // UIKit observer order must not postpone background entry.
            crate::native_preview::suspend_for_background();
        }
        activity().change(active != 0);
    });
}

pub struct RenderActivity {
    activity: Arc<Activity>,
    id: u64,
}
impl RenderActivity {
    pub fn begin() -> Self {
        activity().begin()
    }

    /// Returns true after a pause, so the caller can recreate its VT session.
    pub fn checkpoint(
        &self,
        check_cancel: impl Fn() -> Result<()>,
        quiesce: impl FnOnce() -> Result<()>,
    ) -> Result<bool> {
        check_cancel()?;
        {
            let state = self
                .activity
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if state.active {
                return Ok(false);
            }
        }
        quiesce()?;
        let mut state = self
            .activity
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        state.workers.insert(self.id, true);
        self.activity.changed.notify_all();
        log::info!("render: paused while app is inactive");
        while !state.active {
            drop(state);
            check_cancel()?;
            state = self
                .activity
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !state.active {
                state = self
                    .activity
                    .changed
                    .wait_timeout(state, Duration::from_millis(100))
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        }
        state.workers.insert(self.id, false);
        log::info!("render: resuming in foreground");
        Ok(true)
    }
}
impl Drop for RenderActivity {
    fn drop(&mut self) {
        self.activity
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .workers
            .remove(&self.id);
        self.activity.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn background_waits_for_gpu_quiescence_and_foreground_resumes() {
        let activity = Arc::new(Activity::new(true));
        let worker = activity.begin();
        let quiesced = Arc::new(AtomicBool::new(false));
        let output = Arc::clone(&quiesced);
        let a = Arc::clone(&activity);
        let background = std::thread::spawn(move || a.change(false));
        while activity.state.lock().unwrap().active {
            std::thread::yield_now();
        }
        let resume = Arc::clone(&activity);
        let seen = Arc::clone(&quiesced);
        let foreground = std::thread::spawn(move || {
            background.join().unwrap();
            assert!(seen.load(Ordering::SeqCst));
            resume.change(true);
        });
        assert!(worker
            .checkpoint(
                || Ok(()),
                || {
                    output.store(true, Ordering::SeqCst);
                    Ok(())
                }
            )
            .unwrap());
        foreground.join().unwrap();
        assert!(activity.state.lock().unwrap().active);
        assert!(!worker
            .checkpoint(|| Ok(()), || panic!("foreground must not suspend"))
            .unwrap());
    }

    #[test]
    fn cancellation_while_background_releases_the_worker() {
        let activity = Arc::new(Activity::new(false));
        let worker = activity.begin();
        let cancel = AtomicBool::new(false);
        let error = worker
            .checkpoint(
                || {
                    anyhow::ensure!(!cancel.load(Ordering::SeqCst), "cancelled");
                    Ok(())
                },
                || {
                    cancel.store(true, Ordering::SeqCst);
                    Ok(())
                },
            )
            .unwrap_err();
        assert_eq!(error.to_string(), "cancelled");
        drop(worker);
        assert!(activity.state.lock().unwrap().workers.is_empty());
    }
}
