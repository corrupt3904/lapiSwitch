//! Run stack-heavy blocking work off Tokio's 2 MiB blocking pool.
//!
//! qdl / Firehose frames (GPT parse, progress, owo-colors, reset_on_drop)
//! overflow Windows' 2 MiB worker stack. [`run_heavy`] spawns an OS thread
//! with 64 MiB and blocks until done; wrap it inside a `spawn_blocking`
//! closure so stack size is under our control.

/// Run `f` on a 64 MiB std thread; blocks until finished. Panics → `Err`.
/// Synchronous join, so caller need not be `async`.
pub fn run_heavy<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    std::thread::Builder::new()
        // 64 MiB: magiskboot cpio/xz on real init_boot ramdisks exceeded 32 MiB,
        // and Windows turns thread stack overflow into an uncatchable process abort.
        .stack_size(64 * 1024 * 1024)
        .name("ltbox-heavy".into())
        .spawn(f)
        .map_err(|e| format!("spawn heavy thread: {e}"))?
        .join()
        .map_err(|_| "heavy thread panicked".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_heavy_returns_value() {
        let v = run_heavy(|| 42u32).expect("thread");
        assert_eq!(v, 42);
    }

    #[test]
    fn run_heavy_propagates_panic_as_err() {
        let err = run_heavy(|| {
            panic!("boom");
            #[allow(unreachable_code)]
            0u32
        })
        .expect_err("should convert panic to Err");
        assert!(err.contains("panicked"));
    }

    #[test]
    fn run_heavy_has_plenty_of_stack() {
        // 1 MiB stack array would blow Tokio's 2 MiB budget but fits here.
        let v = run_heavy(|| {
            let buf = [0u8; 1024 * 1024];
            buf[0] as u32 + buf[buf.len() - 1] as u32
        })
        .expect("thread");
        assert_eq!(v, 0);
    }
}
