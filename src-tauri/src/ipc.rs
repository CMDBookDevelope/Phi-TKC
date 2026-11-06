pub mod client {
    use serde::Serialize;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// IPC events are only meaningful for the GUI/server, which parse the
    /// child's stdout. A human running the binary directly wants the progress
    /// bar and logs instead, so `render_impl` turns this off in CLI mode.
    static ENABLED: AtomicBool = AtomicBool::new(true);

    pub fn set_enabled(enabled: bool) {
        ENABLED.store(enabled, Ordering::Relaxed);
    }

    pub fn send<T: Serialize>(value: T) {
        if !ENABLED.load(Ordering::Relaxed) {
            return;
        }
        match serde_json::to_string(&value) {
            Ok(json) => println!("{}", json),
            Err(e) => eprintln!("Serialization error: {}", e),
        }
    }
}