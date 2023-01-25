//! Command line program that mounts the filesystem.

use std::io::Write;

use log::{error, trace};

/// The default log level if RUST_LOG is not set.
static DEFAULT_LOG_LEVEL: &str = "info";

/// Initialise the global logger with the default log level or the level
/// specified in the RUST_LOG environment variable.
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
/// Parse arguments to obtain a mount point.
///
/// Callers of this function should realise that it is possible for this
/// function to terminate the process.
fn get_mnt_point() -> String {
    if let Some(arg) = std::env::args().nth(1) {
        arg
    } else {
        error!("user did not provide mount point as cli option");
        eprintln!("tagfs: please a provide a mount point as an option.");
        std::process::exit(1);
    }
}

/// Entry point.
fn main() {
    init_logging();
    trace!("Application starting.");

    let mnt_point = get_mnt_point();

    if let Err(e) = libtagfs::fs::mount(&mnt_point) {
        error!("unmounted with error {e:?}");
        eprintln!("tagfs: an unexpected error occured.");
        eprintln!("tagfs: please check the log for more details.");
    }

    trace!("Application ending.");
}
