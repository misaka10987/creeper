use std::{
    fmt::Write,
    io::sink,
    sync::{LazyLock, Mutex, MutexGuard, atomic::AtomicBool},
    time::Duration,
};

use indicatif::{FormattedDuration, ProgressState, ProgressStyle};

use crate::Creeper;

fn pb_eta(state: &ProgressState, w: &mut dyn Write) {
    let eta = state.eta();

    if eta >= Duration::from_hours(72) {
        return write!(w, "N/A").unwrap();
    }

    write!(w, "{}", FormattedDuration(eta)).unwrap()
}

pub static PROGRESS_STYLE_DOWNLOAD: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template("{span_child_prefix}{spinner:.green} {msg} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes:>11}/{total_bytes:<11} ETA {eta:<8}")
        .unwrap()
        .with_key("eta", pb_eta)
        .progress_chars("=> ")
});

pub static PROGRESS_STYLE_DEFAULT: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template("{span_child_prefix}{spinner:.green} {span_name}{{{span_fields}}} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>6}/{len:<6} ETA {eta:<8}")
        .unwrap()
        .with_key("eta", pb_eta)
        .progress_chars("=> ")
});

pub struct StdioWriter {
    enabled: AtomicBool,
    inquire_disabled: AtomicBool,
    stdout: Mutex<Box<dyn std::io::Write + Send>>,
    stderr: Mutex<Box<dyn std::io::Write + Send>>,
}

impl Default for StdioWriter {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(true),
            inquire_disabled: AtomicBool::new(false),
            stdout: Mutex::new(Box::new(std::io::stdout())),
            stderr: Mutex::new(Box::new(std::io::stderr())),
        }
    }
}

impl StdioWriter {
    pub(crate) fn disable_by_inquire(&self) {
        self.inquire_disabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn reenable_by_inquire(&self) {
        self.inquire_disabled
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Creeper {
    pub fn enable_stdio(&self) {
        self.stdio
            .enabled
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn disable_stdio(&self) {
        self.stdio
            .enabled
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_stdio_enabled(&self) -> bool {
        if self
            .stdio
            .inquire_disabled
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return false;
        }

        self.stdio.enabled.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn set_stdout(&self, stdout: impl std::io::Write + Send + 'static) {
        *self.stdio.stdout.lock().unwrap() = Box::new(stdout);
    }

    pub fn set_stderr(&self, stderr: impl std::io::Write + Send + 'static) {
        *self.stdio.stderr.lock().unwrap() = Box::new(stderr);
    }

    pub fn reset_stdio(&self) {
        self.set_stdout(std::io::stdout());
        self.set_stderr(std::io::stderr());
    }

    pub fn get_stdout(&self) -> impl std::io::Write {
        if !self.is_stdio_enabled() {
            return StdioGuard::Sink;
        }

        StdioGuard::Output(self.stdio.stdout.lock().unwrap())
    }

    pub fn get_stderr(&self) -> impl std::io::Write {
        if !self.is_stdio_enabled() {
            return StdioGuard::Sink;
        }

        StdioGuard::Output(self.stdio.stderr.lock().unwrap())
    }
}

enum StdioGuard<'a> {
    Output(MutexGuard<'a, Box<dyn std::io::Write + Send>>),
    Sink,
}

impl<'a> std::io::Write for StdioGuard<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            StdioGuard::Output(mutex_guard) => mutex_guard.write(buf),
            StdioGuard::Sink => sink().write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            StdioGuard::Output(mutex_guard) => mutex_guard.flush(),
            StdioGuard::Sink => sink().flush(),
        }
    }

    fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
        match self {
            StdioGuard::Output(mutex_guard) => mutex_guard.write_vectored(bufs),
            StdioGuard::Sink => sink().write_vectored(bufs),
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        match self {
            StdioGuard::Output(mutex_guard) => mutex_guard.write_all(buf),
            StdioGuard::Sink => sink().write_all(buf),
        }
    }

    fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> std::io::Result<()> {
        match self {
            StdioGuard::Output(mutex_guard) => mutex_guard.write_fmt(args),
            StdioGuard::Sink => sink().write_fmt(args),
        }
    }

    fn by_ref(&mut self) -> &mut Self
    where
        Self: Sized,
    {
        self
    }
}
