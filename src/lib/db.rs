//! Module that handles interfacing with the sqlite database.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use rusqlite::Connection;

#[derive(Debug)]
pub struct Tag {
    id: i64,
    pub name: String,
    pub takes_value: bool,
}

#[derive(Debug)]
pub struct TagMapping {
    tag: Tag,
    path: PathBuf,
    value: Option<String>
}

/// Encapsulates all database logic.
#[derive(Debug)]
pub struct Database {
    /// Handle to the sqlite connection.
    conn: Connection,
}

impl Database {
    fn initialise_tables(&self) -> Result<()> {
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS Tag (
            TagID INTEGER PRIMARY KEY,
            Name TEXT NOT NULL,
            TakesValue BOOL NOT NULL,
            UNIQUE(Name)
        )")?;

        // SQL / SQLite does some strange stuff with nulls and unique
        // constraints. See https://sqlite.org/faq.html#q26
        //
        // To work around this we use a generated column that is equal to the
        // value column unless the value column is NULL in which case the
        // constraint column is set to the string 'NULL'.
        // We can use this column in the UNIQUE constraint to avoid the NULL
        // issue.
        // Not an ideal situation however...
        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS TagMapping (
            TagMappingID INTEGER PRIMARY KEY,
            Path TEXT NOT NULL,
            TagID INTEGER NOT NULL,
            Value TEXT,
            ValueUniqConstraint GENERATED ALWAYS AS (COALESCE(Value, 'NULL')),
            FOREIGN KEY(TagID) REFERENCES Tag(TagID),
            UNIQUE(TagID, ValueUniqConstraint, Path)
        )")?;

        Ok(())
    }

    /// This function tries to find a tag matching the str in the database, if
    /// it does not exist it returns None.
    pub fn get_tag(&self, tag: &str) -> Option<Tag> {
        self.conn.query_row(
            "SELECT TagID, Name, TakesValue FROM Tag WHERE Name = ?",
            rusqlite::params![tag],
            |row| Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                takes_value: row.get(2)?
            })
        ).ok()
    }

    /// This function creates a tag in the database.
    ///
    /// It can fail if a tag already exists with the same name.
    fn create_tag(&mut self, tag: &str, takes_value: bool) -> Result<Tag> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO Tag (Name, TakesValue) VALUES (?, ?)",
        )?;

        stmt.execute(rusqlite::params![tag, takes_value])?;

        Ok(Tag {
            id: self.conn.last_insert_rowid(),
            name: tag.to_string(),
            takes_value,
        })
    }

    /// This function creates a mapping between a tag and a path in the
    /// database.
    ///
    /// It can fail if a mapping already exists with the same tag, path and
    /// value. It can also fail if the tag is required to take a value and one
    /// was not given or the opposite.
    pub fn tag(&mut self, path: &str, tag_name: &str, value: Option<&str>) -> Result<()> {

        let tag = if let Some(tag) = self.get_tag(tag_name) {
            if tag.takes_value && value.is_none() {
                bail!("tag \"{}\" takes a value but one was not given", tag_name);
            } else if !tag.takes_value && value.is_some() {
                bail!("tag \"{}\" does not take a value but one was given", tag_name);
            }
            tag
        } else {
            self.create_tag(tag_name, value.is_some())?
        };

        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO TagMapping (TagID, Path, Value) VALUES (?, ?, ?)"
        )?;

        stmt.execute(rusqlite::params![tag.id, path, value])?;

        Ok(())
    }

    /// Returns a list of the tags for a particular path.
    pub fn tags(&mut self, path: &str) -> Result<Vec<TagMapping>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT
                Tag.TagID, Tag.Name, Tag.TakesValue,
                TagMapping.Path, TagMapping.Value,
            FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
            WHERE TagMapping.Path = ?"
        )?;

        let tags = stmt.query_map(rusqlite::params![path],
            |row| {
                Ok(TagMapping {
                    tag: Tag {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        takes_value: row.get(2)?,
                    },
                    path: PathBuf::from(row.get::<_, String>(3)?),
                    value: row.get(4)?,
                })
            }
        )?.collect::<rusqlite::Result<_>>()?;

        Ok(tags)
    }

    /// Returns a list of paths tagged with a particular tag.
    pub fn paths_with_tag(&mut self, tag: &str, value: Option<&str>) -> Result<HashSet<String>> {
        let mut stmt = if value.is_some() {
            self.conn.prepare_cached(
                "SELECT TagMapping.Path
                FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
                WHERE Tag.Name = ? AND TagMapping.Value = ?"
            )?
        } else {
            self.conn.prepare_cached(
                "SELECT TagMapping.Path
                FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
                WHERE Tag.Name = ?"
            )?
        };

        let tags = if value.is_some() {
            stmt.query_map(rusqlite::params![tag, value], |row| row.get(0))?
                .collect::<rusqlite::Result<HashSet<_>>>()?
        } else {
            stmt.query_map(rusqlite::params![tag], |row| row.get(0))?
                .collect::<rusqlite::Result<HashSet<_>>>()?
        };

        Ok(tags)
    }

    /// Returns all values used for a particular tag.
    pub fn values(&mut self, tag: &str) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT TagMapping.Value
            FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
            WHERE Tag.Name = ?"
        )?;

        let tags = stmt.query_map([tag], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;

        Ok(tags)
    }

    /// Returns the names of all tags that appear in the database.
    pub fn all_tags(&mut self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT Name FROM Tag"
        )?;

        let tags = stmt.query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;

        Ok(tags)
    }
}

// TODO: remove unwrap, but it is very unlikely the HOME env var is not set.
fn find_db_path() -> Result<PathBuf> {

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
    std::fs::create_dir_all(&db_dir)?;

    // add our database file to the path.
    db_dir.push("default.db");

    let db = db_dir;
    Ok(db)
}

/// Locates an existing TagFS database, or creates and intialises tables in a
/// new database.
pub fn get_or_create_db() -> Result<Database> {
    let db_path = find_db_path()?;
    let db = Connection::open(db_path)
        .map(|conn| Database { conn })?;

    db.initialise_tables()?;

    Ok(db)
}
