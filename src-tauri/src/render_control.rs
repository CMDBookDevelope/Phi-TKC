//! Cooperative cancellation owns the encoder subprocesses as well as the renderer.
use anyhow::Result;
use std::{
    io::{BufRead, BufReader},
    process::{Child, ChildStderr, ChildStdin, ExitStatus},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};

#[derive(Default)]
pub(crate) struct RenderControl {
    canceled: AtomicBool,
    processes: Mutex<Vec<Weak<Mutex<Child>>>>,
}
impl RenderControl {
    pub(crate) fn listen(
        self: &Arc<Self>,
        input: BufReader<std::io::Stdin>,
    ) -> std::io::Result<()> {
        let control = self.clone();
        std::thread::Builder::new()
            .name("render-cancel".into())
            .spawn(move || {
                for line in input.lines() {
                    match line {
                        Ok(line)
                            if serde_json::from_str::<String>(&line)
                                .is_ok_and(|value| value == "cancel") =>
                        {
                            control.cancel();
                            break;
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            })?;
        Ok(())
    }
    pub(crate) fn cancel(&self) {
        self.canceled.store(true, Ordering::Release);
        for process in self
            .processes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(Weak::upgrade)
        {
            let _ = process.lock().unwrap_or_else(|e| e.into_inner()).kill();
        }
    }
    pub(crate) fn check(&self) -> Result<()> {
        anyhow::ensure!(!self.canceled.load(Ordering::Acquire), "render canceled");
        Ok(())
    }
    pub(crate) fn process(&self, mut child: Child) -> ManagedChild {
        let stdin = child.stdin.take();
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));
        self.processes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Arc::downgrade(&child));
        if self.canceled.load(Ordering::Acquire) {
            let _ = child.lock().unwrap().kill();
        }
        ManagedChild {
            child,
            stdin,
            stderr,
        }
    }
    pub(crate) fn scope(self: &Arc<Self>) -> CancelScope {
        CancelScope {
            control: self.clone(),
            completed: false,
        }
    }
}

pub(crate) struct CancelScope {
    control: Arc<RenderControl>,
    completed: bool,
}
impl CancelScope {
    pub(crate) fn complete(&mut self) {
        self.completed = true;
    }
}
impl Drop for CancelScope {
    fn drop(&mut self) {
        if !self.completed {
            self.control.cancel();
        }
    }
}

pub(crate) struct ManagedChild {
    child: Arc<Mutex<Child>>,
    pub stdin: Option<ChildStdin>,
    pub stderr: Option<ChildStderr>,
}
impl ManagedChild {
    pub(crate) fn wait(&mut self) -> std::io::Result<ExitStatus> {
        self.stdin.take();
        loop {
            if let Some(status) = self
                .child
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .try_wait()?
            {
                return Ok(status);
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        self.stdin.take();
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        if !matches!(child.try_wait(), Ok(Some(_))) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canceled_control_rejects_further_rendering() {
        let control = RenderControl::default();
        control.check().unwrap();
        control.cancel();
        assert!(control.check().is_err());
    }
    #[test]
    fn error_scope_cancels_but_success_scope_preserves_result() {
        let control = Arc::new(RenderControl::default());
        let mut scope = control.scope();
        scope.complete();
        drop(scope);
        control.check().unwrap();
        drop(control.scope());
        assert!(control.check().is_err());
    }
}
