//! Typed operation-log events. GUI consumers use the sink directly; console
//! consumers retain stdout without feeding a second copy through the GUI tap.

use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

const MAX_BUFFERED: usize = 4_096;
static GUI_ATTACHED: AtomicBool = AtomicBool::new(false);
static SINK: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Info,
    Debug,
    Progress { key: String },
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub line: String,
    pub kind: Kind,
}

impl Entry {
    pub fn info(line: String) -> Self {
        Self {
            line,
            kind: Kind::Info,
        }
    }
    pub fn debug(line: String) -> Self {
        Self {
            line,
            kind: Kind::Debug,
        }
    }
}

/// Call before installing the native stdout tap or starting workers.
pub fn attach_gui() {
    GUI_ATTACHED.store(true, Ordering::Relaxed);
}

pub fn progress_key(scope: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("{scope}:{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

pub fn progress(key: &str, line: String) {
    emit(Entry {
        line,
        kind: Kind::Progress {
            key: key.to_owned(),
        },
    });
}

pub fn emit(entry: Entry) {
    if !GUI_ATTACHED.load(Ordering::Relaxed) {
        match entry.kind {
            Kind::Debug => println!("[Debug] {}", entry.line),
            _ => println!("{}", entry.line),
        }
    }
    let mut buf = buffer().lock().unwrap_or_else(|e| e.into_inner());
    push_into(&mut buf, entry);
}

fn buffer() -> &'static Mutex<Vec<Entry>> {
    SINK.get_or_init(|| Mutex::new(Vec::new()))
}

fn push_into(buf: &mut Vec<Entry>, entry: Entry) {
    // Coalesce only adjacent updates of the same transfer. Never deduplicate
    // ordinary lines: identical text can belong to different EDL sessions.
    if matches!(&entry.kind, Kind::Progress { .. })
        && buf.last().is_some_and(|last| last.kind == entry.kind)
    {
        *buf.last_mut().expect("checked above") = entry;
        return;
    }
    if buf.len() >= MAX_BUFFERED {
        // Prefer dropping a replaceable progress update over a diagnostic.
        let index = buf
            .iter()
            .position(|e| matches!(e.kind, Kind::Progress { .. }))
            .unwrap_or(0);
        buf.remove(index);
    }
    buf.push(entry);
}

pub fn drain() -> Vec<Entry> {
    std::mem::take(&mut *buffer().lock().unwrap_or_else(|e| e.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_session_messages_are_not_deduplicated() {
        let mut buf = Vec::new();
        push_into(&mut buf, Entry::info("Sahara connected".into()));
        push_into(&mut buf, Entry::info("Sahara connected".into()));
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn only_adjacent_progress_with_the_same_key_is_replaced() {
        let mut buf = Vec::new();
        for (key, line) in [("a", "10%"), ("a", "20%"), ("b", "0%"), ("a", "30%")] {
            push_into(
                &mut buf,
                Entry {
                    line: line.into(),
                    kind: Kind::Progress { key: key.into() },
                },
            );
        }
        assert_eq!(
            buf.iter().map(|e| e.line.as_str()).collect::<Vec<_>>(),
            ["20%", "0%", "30%"]
        );
    }

    #[test]
    fn queue_is_bounded() {
        let mut buf = Vec::new();
        for i in 0..MAX_BUFFERED + 5 {
            push_into(&mut buf, Entry::info(format!("line {i}")));
        }
        assert_eq!(buf.len(), MAX_BUFFERED);
        assert_eq!(buf[0].line, "line 5");
    }
}
