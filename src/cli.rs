//! Module that handles parsing the command line using clap.

use anyhow::{Result, Context};
use camino::Utf8PathBuf;

use libtagfs::db::TagValuePair;

/// Handles the query command args.
#[derive(clap::Args, Clone, Debug)]
pub struct QueryCommand {
    /// Query to run.
    #[arg(required = true, value_name = "query")]
    pub query: String,

    /// Enable case sensitivity for the strict equals operator (==).
    #[arg(short = 'I', long = "case-sensitive")]
    pub case_sensitive: bool,
}

/// Handles the tag command args.
#[derive(clap::Args, Clone, Debug)]
pub struct TagCommand {
    /// Path to apply tag to.
    #[arg(required = true, value_name = "path")]
    pub path: Utf8PathBuf,

    /// Tag and optional value to apply to path tag(=value)?
    #[arg(required = true, value_name = "tags")]
    pub tags: Vec<TagValuePair>,
}

/// Handles the mount command args.
#[derive(clap::Args, Clone, Debug)]
pub struct MountCommand {
    /// Directory to mount the file system at.
    #[arg(required = true, value_name = "mount-point")]
    pub mount_point: Utf8PathBuf,
}

/// Handles the tags command args.
#[derive(clap::Args, Clone, Debug)]
pub struct TagsCommand {
    /// Path to show associated tags.
    #[arg(value_name = "path")]
    pub path: Option<Utf8PathBuf>,
}

/// Handles the untag command args.
#[derive(clap::Args, Clone, Debug)]
pub struct UntagCommand {
    /// Path to remove tag from.
    #[arg(required = true, value_name = "path")]
    pub path: Utf8PathBuf,

    /// Optional tag and optional value to remove from path tag(=value)?
    #[arg(value_name = "tag")]
    pub tag: Option<TagValuePair>,
}

/// Handles the autotag command args.
#[cfg(feature = "autotag")]
#[derive(clap::Args, Clone, Debug)]
pub struct AutotagCommand {
    /// Path to directory or file to autotag.
    #[arg(required = true, value_name = "path")]
    pub path: Utf8PathBuf,

    /// TMDB API key.
    ///
    /// Only required when autotagging films. If not provided the TMDB_KEY
    /// environment variable is used instead.
    #[arg(long = "tmdb-key", value_name = "tmdb-api-key")]
    pub tmdb_key: Option<String>,
}

/// Handles the prefix command args.
#[derive(clap::Args, Clone, Debug)]
pub struct PrefixCommand {
    /// Prefix to change.
    #[arg(required = true, value_name = "old-prefix")]
    pub old_prefix: String,

    /// New prefix.
    #[arg(required = true, value_name = "new-prefix")]
    pub new_prefix: String,
}

/// Handles the edit command args.
#[derive(clap::Args, Clone, Debug)]
pub struct EditCommand {
}

/// Handles the stored-queries command args.
#[derive(clap::Subcommand, Clone, Debug)]
pub enum StoredQueriesSubCommand {

    /// List the queries stored in the database. (default)
    #[command(name = "list")]
    List,

    /// Store a new query in the database.
    #[command(name = "create", visible_alias = "add")]
    Create {
        /// Name of the new query.
        #[arg(required = true, value_name = "name")]
        name: String,

        /// The new query.
        #[arg(required = true, value_name = "query")]
        query: String,
    },

    /// Remove a query from the database.
    #[command(name = "remove", visible_alias = "delete")]
    Delete {
        /// Name of the query to remove from the database.
        #[arg(required = true, value_name = "query")]
        query_to_delete: String,
    },
}

/// Wrapper struct to make arguements to the stored-queries command optional.
#[derive(clap::Args, Clone, Debug)]
pub struct StoredQueriesCommand {
    #[command(subcommand)]
    pub command: Option<StoredQueriesSubCommand>,
}

/// Contains a subcommand and the specific struct pertaining to it.
#[derive(clap::Subcommand, Debug)]
pub enum Command {

    /// Apply a tag to a path.
    Tag(TagCommand),

    /// Remove a tag from a path.
    ///
    /// When a tag value pair is given, only that specific pair is removed.
    /// When a tag without value is given, all tag value pairs for that tag
    /// are removed.
    /// When no tag is given, all tags are removed from the path.
    Untag(UntagCommand),

    /// Mount the filesystem.
    #[command(visible_alias = "mnt", visible_alias = "m")]
    Mount(MountCommand),

    /// Display tags associated with a path.
    Tags(TagsCommand),

    /// Query the database.
    ///
    /// The query format is best described with an example:
    ///
    ///     genre==romance and not actor=delpy
    ///
    /// This query will match the paths with the genre=romance tag (exact
    /// match) as long as it does not have the actor tag with a value matching
    /// "delpy" (non-exact match).
    ///
    /// There is also an or operator and parentheses can be used to further
    /// refine the query.
    #[command(visible_alias = "q", visible_alias = "search")]
    Query(QueryCommand),

    /// Autotag a directory tree or file.
    #[cfg(feature = "autotag")]
    Autotag(AutotagCommand),

    /// Modify the prefix of paths in the database.
    ///
    /// Implemented as a naïve search and replace.
    Prefix(PrefixCommand),

    /// Edit the tags database using a text editor.
    Edit(EditCommand),

    /// List, create and delete stored queries in the database.
    StoredQueries(StoredQueriesCommand),
}

/// Contains the parsed arguments from the command line.
#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    /// Path to database to use or create.
    #[arg(long, global = true, value_name = "database")]
    pub database: Option<Utf8PathBuf>,
}

impl Args {

    // TODO: remove unwrap, but it is very unlikely the HOME env var is not
    //       set.
    // TODO: this could also be cached, but not really worth it atm.
    /// Find the path to the database. \
    /// It is either specified in the command line arguments or a default is
    /// computed.
    pub fn db_path(&self) -> Result<Utf8PathBuf> {

        if let Some(path) = &self.database {
            return Ok(path.clone());
        }

        // find the XDG_DATA_HOME or generate a suitable alternative.
        let mut db_dir = std::env::var("XDG_DATA_HOME")
            .map_or_else(
                |_e| {
                    let home = std::env::var("HOME").unwrap();
                    let mut path = Utf8PathBuf::from(&home).to_path_buf();
                    path.push(".local/share");
                    path
                },
                |xdg_data_path| Utf8PathBuf::from(&xdg_data_path));

        // create our own directory within XDG_DATA_HOME.
        db_dir.push("tagfs");
        std::fs::create_dir_all(&db_dir).with_context(||
            format!("could not create directory \"{}\" for the database.",
                    db_dir))?;

        // add our database file to the path.
        db_dir.push("default.db");

        let db = db_dir;
        Ok(db)
    }
}
