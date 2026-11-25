//! Buffered stdout for the JSON log subscriber.
//!
//! Every request writes one access-log line. When stdout is a file or a pipe,
//! unbuffered output turns that into one `write` syscall per line and one lock
//! hand-off between worker threads. A shared `BufWriter` amortises both. A
//! terminal keeps direct output so `make dev-api` still streams lines live.
//! [`LogWriter::flush`] drains the buffer during graceful shutdown.

use std::io::{self, BufWriter, IsTerminal, Write};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tracing_subscriber::fmt::MakeWriter;

/// Small enough that a crash loses at most a handful of lines, large enough to
/// avoid a syscall per request.
const BUFFER_BYTES: usize = 16 << 10;

#[derive(Debug)]
enum Output {
    Direct(io::Stdout),
    Buffered(BufWriter<io::Stdout>),
}

impl Write for Output {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Direct(stdout) => stdout.write(buffer),
            Self::Buffered(buffered) => buffered.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Direct(stdout) => stdout.flush(),
            Self::Buffered(buffered) => buffered.flush(),
        }
    }
}

/// Cloneable handle to the process-wide log sink.
#[derive(Clone, Debug)]
pub struct LogWriter {
    inner: Arc<Mutex<Output>>,
}

impl LogWriter {
    #[must_use]
    pub fn new() -> Self {
        let stdout = io::stdout();
        let output = if stdout.is_terminal() {
            Output::Direct(stdout)
        } else {
            Output::Buffered(BufWriter::with_capacity(BUFFER_BYTES, stdout))
        };
        Self {
            inner: Arc::new(Mutex::new(output)),
        }
    }

    /// Writes buffered lines to stdout. Called after the server stops.
    pub fn flush(&self) {
        let mut writer = self.lock();
        let _ = writer.flush();
    }

    fn lock(&self) -> MutexGuard<'_, Output> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for LogWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// One event's borrow of the shared sink.
pub struct LogWriterGuard<'a>(MutexGuard<'a, Output>);

impl Write for LogWriterGuard<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<'a> MakeWriter<'a> for LogWriter {
    type Writer = LogWriterGuard<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriterGuard(self.lock())
    }
}
