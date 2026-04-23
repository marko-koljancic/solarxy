use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

use time::UtcOffset;
use time::macros::format_description;
use tracing::Level;
use tracing_subscriber::Layer;

const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub(crate) level: Level,
    pub(crate) message: String,
    pub(crate) timestamp: String,
}

pub type LogBuffer = Arc<Mutex<VecDeque<LogEntry>>>;

pub fn new_log_buffer() -> LogBuffer {
    Arc::new(Mutex::new(VecDeque::with_capacity(MAX_ENTRIES)))
}

#[derive(Debug)]
pub struct ConsoleLayer {
    buffer: LogBuffer,
    offset: UtcOffset,
}

impl ConsoleLayer {
    pub fn new(buffer: LogBuffer, offset: UtcOffset) -> Self {
        Self { buffer, offset }
    }
}

impl<S: tracing::Subscriber> Layer<S> for ConsoleLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        if visitor.message.is_empty() {
            return;
        }

        let fmt = format_description!("[hour]:[minute]:[second].[subsecond digits:3]");
        let timestamp = time::OffsetDateTime::now_utc()
            .to_offset(self.offset)
            .format(&fmt)
            .unwrap_or_else(|_| String::from("--:--:--.---"));

        let entry = LogEntry {
            level: *event.metadata().level(),
            message: visitor.message,
            timestamp,
        };

        if let Ok(mut buf) = self.buffer.lock() {
            if buf.len() >= MAX_ENTRIES {
                buf.pop_front();
            }
            buf.push_back(entry);
        }
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else if self.message.is_empty() {
            self.message = format!("{}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else if self.message.is_empty() {
            self.message = format!("{}={value}", field.name());
        }
    }
}

#[derive(Debug)]
pub struct ConsoleState {
    pub(crate) buffer: LogBuffer,
    pub auto_scroll: bool,
    pub min_level: Level,
    pub visible: bool,
    pub docked: bool,
    pub(crate) search: String,
}

impl ConsoleState {
    pub fn new(buffer: LogBuffer) -> Self {
        Self {
            buffer,
            auto_scroll: true,
            min_level: Level::INFO,
            visible: false,
            docked: true,
            search: String::new(),
        }
    }

    pub fn clear(&self) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
    }
}
