use std::io::Write;

use log::{error, trace};

static DEFAULT_LOG_LEVEL: &str = "info";

fn init_logging() {
    use env_logger::{Builder, Env};

    let env = Env::default().default_filter_or(DEFAULT_LOG_LEVEL);
    Builder::from_env(env)
        .format(|sink, rec| {
            writeln!(sink, "[{}] {}", rec.level(), rec.args())
        })
        .init();
}

// TODO: make this more robust with proper argument parsing.
// TODO: validate path and make sure it is sensible.
// Callers of this function should realise that it is possible for this
// function to terminate the process.
fn get_mnt_point() -> String {
    if let Some(arg) = std::env::args().nth(1) {
        arg
    } else {
        error!("user did not provide mount point as cli option");
        eprintln!("tagfs: please a provide a mount point as an option.");
        std::process::exit(1);
    }
}

fn main() {
    init_logging();
    trace!("Application starting.");

    let mnt_point = get_mnt_point();

    // returns when fs is unmounted.
    tagfs::fs::mount(&mnt_point);

    trace!("Application ending.");
}
