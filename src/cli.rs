//! Module that handles parsing the command line using clap.

#[cfg(test)]
mod tests;

use std::path::Path;

use anyhow::{Result, Context};
use once_cell::sync::Lazy;

// NOTE: regex is quite restrictive at the moment, might want to allow more
// stuff e.g. special characters.
/// Regex to parse tag with an optional value delimitted by an equals.
static TAG_VALUE_PAIR_REGEX: Lazy<regex::Regex> = Lazy::new(||
    regex::Regex::new("^(?P<tag>[a-zA-Z0-9]+)(?:=(?P<value>.*))?$").unwrap());

/// Helper struct to allow clap to perform the parsing for us.
#[derive(Clone, Debug)]
pub struct TagValuePair {
    pub tag: String,
    pub value: Option<String>
}

/// Required by clap to parse a tag value pair. \
/// Used when the tag value pair is not in the correct format.
#[derive(Clone, Debug)]
pub struct TagValuePairParseError;

impl std::fmt::Display for TagValuePairParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not in the required tag(=value)? format. Only alphanumeric ASCII characters are allowed in the tag name.")
    }
}

impl std::error::Error for TagValuePairParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl std::str::FromStr for TagValuePair {
    type Err = TagValuePairParseError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let captures = TAG_VALUE_PAIR_REGEX.captures(s)
            .ok_or(Self::Err {})?;

        let tag = captures.name("tag").ok_or(TagValuePairParseError)?
            .as_str().to_string();
        let value = captures.name("value").map(|m| m.as_str().to_string());

        Ok(Self { tag, value })
    }
}

// impl std::fmt::Display for TagValuePair {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         if let Some(value) = &self.value {
//             write!(f, "{}={}", self.tag, value)
//         } else {
//             write!(f, "{}", self.tag)
//         }
//     }
// }

/// Handles the query command args.
#[derive(clap::Args, Clone, Debug)]
pub struct QueryCommand {
    /// Query to run.
    #[arg(required = true, value_name = "query")]
    pub query: String,
}

/// Handles the tag command args.
#[derive(clap::Args, Clone, Debug)]
pub struct TagCommand {
    /// Path to apply tag to.
    #[arg(required = true, value_name = "path")]
    pub path: String,

    /// Tag and optional value to apply to path tag(=value)?
    #[arg(required = true, value_name = "tag")]
    pub tag: TagValuePair,
}

/// Handles the mount command args.
#[derive(clap::Args, Clone, Debug)]
pub struct MountCommand {
    /// Directory to mount the file system at.
    #[arg(required = true, value_name = "mount-point")]
    pub mount_point: String,
}

/// Handles the tags command args.
#[derive(clap::Args, Clone, Debug)]
pub struct TagsCommand {
    /// Path to show associated tags.
    #[arg(value_name = "path")]
    pub path: Option<String>,
}

/// Handles the untag command args.
#[derive(clap::Args, Clone, Debug)]
pub struct UntagCommand {
    /// Path to remove tag from.
    #[arg(required = true, value_name = "path")]
    pub path: String,

    /// Optional tag and optional value to remove from path tag(=value)?
    #[arg(value_name = "tag")]
    pub tag: Option<TagValuePair>,
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

    /// Display tags associated with a path
    Tags(TagsCommand),

    /// Query the database.
    #[command(visible_alias = "q", visible_alias = "search")]
    Query(QueryCommand),
}

/// Contains the parsed arguments from the command line.
#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    /// Path to database to use or create.
    #[arg(long, global = true, value_name = "database")]
    pub database: Option<String>,
}

impl Args {

    // TODO: remove unwrap, but it is very unlikely the HOME env var is not
    //       set.
    // TODO: this could also be cached, but not really worth it atm.
    /// Find the path to the database. \
    /// It is either specified in the command line arguments or a default is
    /// computed.
    pub fn db_path(&self) -> Result<String> {

        if let Some(path) = &self.database {
            return Ok(path.clone());
        }

        // find the XDG_DATA_HOME or generate a suitable alternative.
        let mut db_dir = std::env::var("XDG_DATA_HOME")
            .map_or_else(
                |_e| {
                    let home = std::env::var("HOME").unwrap();
                    let mut path = Path::new(&home).to_path_buf();
                    path.push(Path::new(".local/share"));
                    path
                },
                |xdg_data_path| Path::new(&xdg_data_path).to_path_buf());

        // create our own directory within XDG_DATA_HOME.
        db_dir.push("tagfs");
        std::fs::create_dir_all(&db_dir).with_context(||
            format!("could not create directory \"{}\" for the database.", db_dir.display()))?;

        // add our database file to the path.
        db_dir.push("default.db");

        let db = db_dir;
        Ok(db.as_os_str().to_string_lossy().to_string())
    }
}
