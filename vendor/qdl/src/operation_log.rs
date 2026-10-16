// SPDX-License-Identifier: BSD-3-Clause
//! Optional structured observer for GUI hosts. With no observer, retain the
//! terminal progress bars. This module never changes transport or ACK handling.

use pbr::{ProgressBar, Units};
use std::{
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub enum Event<'a> {
    Diagnostic(&'a str),
    Transfer {
        id: u64,
        write: bool,
        target: &'a str,
        completed: u64,
        total: u64,
    },
}

static OBSERVER: OnceLock<fn(Event<'_>)> = OnceLock::new();

pub fn set_observer(observer: fn(Event<'_>)) {
    let _ = OBSERVER.set(observer);
}

pub fn diagnostic(message: &str) {
    if let Some(observer) = OBSERVER.get() {
        observer(Event::Diagnostic(message));
    } else {
        anstream::println!("{message}");
    }
}

pub(crate) struct Transfer {
    id: u64,
    write: bool,
    target: String,
    completed: u64,
    total: u64,
    last_emit: Instant,
    emitted: bool,
    bar: Option<ProgressBar<std::io::Stdout>>,
}

impl Transfer {
    pub(crate) fn new(write: bool, target: String, total: u64) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let bar = if OBSERVER.get().is_none() {
            let mut bar = ProgressBar::new(total);
            bar.set_units(Units::Bytes);
            bar.show_time_left = true;
            bar.message(&format!(
                "{} {target}: ",
                if write { "Writing" } else { "Reading" }
            ));
            Some(bar)
        } else {
            None
        };
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            write,
            target,
            completed: 0,
            total,
            last_emit: Instant::now(),
            emitted: false,
            bar,
        }
    }

    pub(crate) fn add(&mut self, bytes: u64) {
        self.completed = self.completed.saturating_add(bytes);
        if let Some(bar) = &mut self.bar {
            bar.add(bytes);
        }
        if self.last_emit.elapsed() >= Duration::from_millis(750) {
            self.publish();
            self.last_emit = Instant::now();
            self.emitted = true;
        }
    }

    fn publish(&self) {
        if let Some(observer) = OBSERVER.get() {
            observer(Event::Transfer {
                id: self.id,
                write: self.write,
                target: &self.target,
                completed: self.completed,
                total: self.total,
            });
        }
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        // Preserve the final observed byte count on both success and failure.
        // A byte count of 100% is not an ACK or a successful operation result.
        if self.emitted {
            self.publish();
        }
        if let Some(bar) = &mut self.bar {
            bar.finish_println("");
        }
    }
}
