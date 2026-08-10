use std::{
    fmt::Write,
    sync::{LazyLock, Mutex, MutexGuard},
};

use indicatif::{FormattedDuration, ProgressState, ProgressStyle};

use crate::Creeper;

fn pb_eta(state: &ProgressState, w: &mut dyn Write) {
    write!(w, "{}", FormattedDuration(state.eta())).unwrap()
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
    stdout: Mutex<Box<dyn std::io::Write + Send>>,
    stderr: Mutex<Box<dyn std::io::Write + Send>>,
}

impl Default for StdioWriter {
    fn default() -> Self {
        Self {
            stdout: Mutex::new(Box::new(std::io::stdout())),
            stderr: Mutex::new(Box::new(std::io::stderr())),
        }
    }
}

impl Creeper {
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

    pub fn get_stdout(&self) -> MutexGuard<'_, Box<dyn std::io::Write + Send>> {
        self.stdio.stdout.lock().unwrap()
    }

    pub fn get_stderr(&self) -> MutexGuard<'_, Box<dyn std::io::Write + Send>> {
        self.stdio.stderr.lock().unwrap()
    }
}
