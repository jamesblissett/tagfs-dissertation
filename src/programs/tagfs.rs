//! Entry point of tagfs where subcommands are implemented.

use std::io::Write;

use anyhow::{bail, Context, Result};
use clap::Parser;
use log::{error, trace};

use libtagfs::{
    cli::{Args, Command, MountCommand, TagCommand, TagsCommand, UntagCommand, TagValuePair},
    db::Database,
};

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

// TODO: implement a 'prefix' system so that we don't have to give full paths
//       to tag.
/// Tag subcommand entry point.
fn tag_main(command: TagCommand, mut db: Database) -> Result<()> {
    let tag = command.tag;
    let path = command.path.trim_end_matches('/');

    db.tag(path, &tag.tag, tag.value.as_deref())?;
    Ok(())
}

/// Untag subcommand entry point.
fn untag_main(command: UntagCommand, mut db: Database) -> Result<()> {
    let path = command.path.trim_end_matches('/');

    match command.tag {
        Some(TagValuePair { tag, value }) =>
            db.untag(path, &tag, value.as_deref()),
        None => db.untag_all(path),
    }
}

/// Tags subcommand entry point when no path argument is given.
fn tags_all_main(mut db: Database) -> Result<()> {
    let tags = db.all_tags()?;

    if tags.is_empty() {
        bail!("no tags found in the database.");
    }

    for tag in &tags {
        println!("{tag}");
    }

    Ok(())
}

/// Tags subcommand entry point when a path argument is given.
fn tags_specific_path_main(path: &str, mut db: Database) -> Result<()> {
    let path = path.trim_end_matches('/');
    let tags = db.tags(path)?;

    if tags.is_empty() {
        bail!("no tags found associated with \"{}\".", path);
    }

    for tag in &tags {
        println!("{}", libtagfs::db::SimpleTagFormatter(tag));
    }

    Ok(())
}

/// Mount subcommand entry point.
fn mount_main(command: MountCommand, mut db: Database) -> Result<()> {

    populate_db(&mut db)?;

    libtagfs::fs::mount(&command.mount_point, db)
        .context("an unexpected fuse error occured. Please check the log for more details.")?;

    Ok(())
}

/// Debugging function to generate test data.
fn populate_db(db: &mut Database) -> Result<()> {

    // Ignore result deliberate debug.
    db.tag("/media/hdd/film/Before Sunrise (1995)", "genre", Some("romance"));
    db.tag("/media/hdd/film/Before Sunrise (1995)", "genre", Some("slice-of-life"));
    db.tag("/media/hdd/film/Before Sunset (2004)", "genre", Some("romance"));
    db.tag("/media/hdd/film/Casino (1995)", "genre", Some("crime"));
    db.tag("/media/hdd/film/Heat (1995)", "genre", Some("crime"));

    db.tag("/media/hdd/film/Before Sunrise (1995)", "favourite", None);

    Ok(())
}

/// Helper function to display and log an error.
fn display_and_log_error<T>(res: Result<T>) {
    if let Err(e) = res {
        error!("error: {e:?}");
        eprintln!("tagfs: {e}");
    }
}

/// Helper function to display and log error then exit, or alternatively unwrap.
fn unwrap_or_exit<T>(res: Result<T>) -> T {
    if let Ok(db_path) = res {
        db_path
    } else {
        display_and_log_error(res);
        std::process::exit(1)
    }
}

/// Entry point.
fn main() {
    init_logging();
    trace!("Application starting.");

    let args = Args::parse();

    let db_path = unwrap_or_exit(args.db_path());
    let db = unwrap_or_exit(libtagfs::db::get_or_create_db(&db_path)
        .context("could not find or create a database. Please check the log for more details."));

    let err = match args.command {
        Command::Tag(tag_command) => tag_main(tag_command, db),
        Command::Untag(untag_command) => untag_main(untag_command, db),
        Command::Mount(mount_command) => mount_main(mount_command, db),
        Command::Tags(TagsCommand { path: Some(path), .. } ) =>
            tags_specific_path_main(&path, db),
        Command::Tags(TagsCommand { path: None, .. } ) =>
            tags_all_main(db),
    };

    display_and_log_error(err);

    trace!("Application ending.");
}
