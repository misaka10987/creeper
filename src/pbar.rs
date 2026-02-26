use std::{fmt::Write, sync::LazyLock};

use indicatif::{FormattedDuration, ProgressState, ProgressStyle};

fn pb_eta(state: &ProgressState, w: &mut dyn Write) {
    write!(w, "{}", FormattedDuration(state.eta())).unwrap()
}

pub static PROGRESS_STYLE_DOWNLOAD: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template("{spinner:.green} {msg} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes:>11}/{total_bytes:<11} ETA {eta:<8}")
        .unwrap()
        .with_key("eta", pb_eta)
        .progress_chars("=> ")
});
