use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::OnceLock;

const MAX_LOG_ENTRIES: usize = 2000;

#[derive(Clone, serde::Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

static LOG_BUFFER: OnceLock<Mutex<VecDeque<LogEntry>>> = OnceLock::new();

fn buffer() -> &'static Mutex<VecDeque<LogEntry>> {
    LOG_BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_LOG_ENTRIES)))
}

/// Custom logger that captures log messages to an in-memory ring buffer
/// and also writes to stderr for terminal debugging.
pub struct DebugLogger;

impl log::Log for DebugLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // Capture our own crate's logs at all levels, plus WARN+ from deps
        metadata.target().starts_with("tunante")
            || metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let entry = LogEntry {
            timestamp: chrono_now(),
            level: record.level().to_string(),
            target: record.target().to_string(),
            message: format!("{}", record.args()),
        };

        // Print to stderr for terminal debugging
        eprintln!(
            "[{}] {} [{}] {}",
            entry.timestamp, entry.level, entry.target, entry.message
        );

        // Store in ring buffer
        let mut buf = buffer().lock();
        if buf.len() >= MAX_LOG_ENTRIES {
            buf.pop_front();
        }
        buf.push_back(entry);
    }

    fn flush(&self) {}
}

/// Get current time as ISO-like string without pulling in chrono crate
fn chrono_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();

    // Simple HH:MM:SS.mmm format (UTC)
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}.{:03}", h, m, s, millis)
}

/// Initialize the global logger. Call once at startup.
pub fn init() {
    // Pre-initialize the buffer
    let _ = buffer();

    let _ = log::set_logger(&DebugLogger);
    log::set_max_level(log::LevelFilter::Debug);
}

/// Retrieve all buffered log entries.
pub fn get_logs() -> Vec<LogEntry> {
    buffer().lock().iter().cloned().collect()
}

/// Clear the log buffer.
pub fn clear_logs() {
    buffer().lock().clear();
}
