//! Entry point of tagfs where subcommands are implemented.

mod cli;

use std::{
    ffi::OsString, io::{Write, Read}, str::FromStr
};

use anyhow::{bail, Context, Result};
use clap::Parser;
use log::{error, warn, trace};

use libtagfs::db::{Database, TagValuePair};

use cli::{
    Args, Command, EditCommand, MountCommand, PrefixCommand, QueryCommand,
    TagCommand, TagsCommand, UntagCommand,
};

#[cfg(feature = "autotag")]
use cli::AutotagCommand;

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

// TODO: reject relative paths.
/// Tag subcommand entry point.
fn tag_main(command: TagCommand, mut db: Database) -> Result<()> {
    let path = command.path.trim_end_matches('/');

    for tag in &command.tags {
        db.tag(path, &tag.tag, tag.value.as_deref())?;
    }
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

/// Query subcommand entry point.
fn query_main(command: QueryCommand, mut db: Database) -> Result<()> {
    let query = command.query;

    let paths = db.query(&query, command.case_sensitive)?;

    if paths.is_empty() {
        bail!("no paths found matching query \"{}\".", query);
    }

    for (path, _) in &paths {
        println!("{path}");
    }

    Ok(())
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
fn tags_specific_path_main(path: &str, db: Database) -> Result<()> {
    let path = path.trim_end_matches('/');
    let tags = db.tags(path)?;

    if tags.is_empty() {
        bail!("no tags found associated with \"{}\".", path);
    }

    for tag in &tags {
        println!("{}", libtagfs::db::SimpleTagFormatter::from(tag));
    }

    Ok(())
}

/// Mount subcommand entry point.
fn mount_main(command: MountCommand, db: Database) -> Result<()> {

    if !db.all_paths_valid()? {
        warn!("The database contains invalid paths. Mounting anyway...");
    }

    libtagfs::fs::mount(&command.mount_point, db)
        .context("an unexpected fuse error occured. \
                  Please check the log for more details.")?;

    Ok(())
}

/// Prefix subcommand entry point.
fn prefix_main(command: PrefixCommand, mut db: Database) -> Result<()> {
    db.prefix_change(&command.old_prefix, &command.new_prefix)
}

/// Edit subcommand entry point
fn edit_main(_command: EditCommand, mut db: Database) -> Result<()> {

    let mut initial_dump = String::new();
    db.to_edit_repr(&mut initial_dump)?;

    let temp_file = mktemp::Temp::new_file()
        .context("could not create temporary file to edit.")?;

    std::fs::write(&temp_file, &initial_dump).with_context(||
        format!("could not write to temporary file \"{}\".",
                temp_file.display()))?;

    // allow user to edit text here.
    //   make temp file.
    //   launch $EDITOR on temp file.
    //   wait until editor process ends
    //   read temp file

    // naïvely check bytewise if the file has changed and do nothing if it has
    // not.

    let exit_status = std::process::Command::new(get_editor())
        .arg(temp_file.as_os_str())
        .status()
        .context("failed to start an editor. \
                  Please set $EDITOR or $VISUAL to your preferred editor.")?;

    if !exit_status.success() {
        bail!("editor did not exit cleanly, aborting...");
    }

    let mut file = std::fs::File::open(&temp_file).with_context(||
        format!("could not open temporary file \"{}\" for reading.",
                temp_file.display()))?;

    // preallocating the edited string to be at least the same size as the
    // initial string - might save a few allocations.
    let mut edited_dump = String::with_capacity(initial_dump.len());
    file.read_to_string(&mut edited_dump)?;

    if edited_dump == initial_dump {
        bail!("nothing changed, aborting...");
    }

    db.from_edit_repr(&mut std::io::BufReader::new(edited_dump.as_bytes()))?;

    // the temp file from the mktemp crate is deleted on drop (explicit drop
    // for clarity)
    drop(temp_file);

    Ok(())
}

// TODO: ensure path exists and inform user if not.
#[cfg(feature = "autotag")]
/// Autotag subcommand entry point.
fn autotag_main(command: AutotagCommand, mut db: Database) -> Result<()> {

    let autotagger = libtagfs::autotag::AutoTagger::new(command.tmdb_key);

    // if we are given a file rather a directory, walkdir will just return the
    // file so we do not need to do anything special to handle this case.
    let root = command.path;

    // recursively walk the directory given to us by the user.
    // we are only interested in files, not directories.
    let entries = walkdir::WalkDir::new(root).follow_links(true).into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.metadata().ok()
            .filter(|metadata| metadata.is_file()).is_some());

    for entry in entries {
        let path = entry.path();
        if let Some(path) = path.to_str() {
            autotagger.autotag(path, &mut db)?;
        } else {
            warn!("ignoring path \"{}\" due to invalid UTF-8.",
                  path.display());
        }
    }

    Ok(())
}

/// Helper function to display and log an error.
fn display_and_log_error<T>(res: Result<T>) {
    if let Err(e) = res {
        error!("error: {e:?}");
        eprintln!("{}: {e}", clap::crate_name!());
    }
}

/// Helper function to display and log error then exit, or alternatively
/// unwrap.
fn unwrap_or_exit<T>(res: Result<T>) -> T {
    if let Ok(db_path) = res {
        db_path
    } else {
        display_and_log_error(res);
        std::process::exit(1)
    }
}

fn get_editor() -> OsString {
    std::env::var_os("VISUAL")
        .or_else(|| std::env::var_os("EDITOR"))
        .unwrap_or_else(|| OsString::from_str("nano").unwrap())
}

/// Entry point.
fn main() {
    init_logging();
    trace!("Application starting.");

    let args = Args::parse();

    let db_path = unwrap_or_exit(args.db_path());
    let db = unwrap_or_exit(libtagfs::db::get_or_create_db(Some(&db_path))
        .context("could not find or create a database. \
                  Please check the log for more details."));

    let err = match args.command {
        Command::Tag(tag_command) => tag_main(tag_command, db),
        Command::Untag(untag_command) => untag_main(untag_command, db),
        Command::Mount(mount_command) => mount_main(mount_command, db),
        Command::Tags(TagsCommand { path: Some(path), .. } ) =>
            tags_specific_path_main(&path, db),
        Command::Tags(TagsCommand { path: None, .. } ) =>
            tags_all_main(db),
        Command::Query(query_command) => query_main(query_command, db),
        Command::Prefix(prefix_command) => prefix_main(prefix_command, db),
        Command::Edit(edit_command) => edit_main(edit_command, db),

        #[cfg(feature = "autotag")]
        Command::Autotag(autotag_command) => autotag_main(autotag_command, db),
    };

    display_and_log_error(err);

    trace!("Application ending.");
}
