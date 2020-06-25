use indicatif::{ProgressBar, ProgressStyle};

pub fn new_spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(80);
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&[
                "⣾",
                "⣽",
                "⣻",
                "⢿",
                "⡿",
                "⣟",
                "⣯",
                "⣷"
            ])
            .template("{spinner:.blue} {msg}"),
    );
    pb.set_message(message);
    pb
}