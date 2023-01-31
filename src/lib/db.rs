//! Module that handles interfacing with the sqlite database.

use std::collections::HashSet;

use anyhow::{bail, Context, Result};
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
    path: String,
    value: Option<String>
}

pub struct SimpleTagFormatter<'a>(pub &'a TagMapping);
impl<'a> std::fmt::Display for SimpleTagFormatter<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(value) = &self.0.value {
            write!(f, "{}={}", self.0.tag.name, value)
        } else {
            write!(f, "{}", self.0.tag.name)
        }
    }
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

        let err = stmt.execute(rusqlite::params![tag.id, path, value]);
        if let Err(rusqlite::Error::SqliteFailure(sql_error, _)) = err {
            // Error code 2067 is a failure of a unique constraint.
            // This means we tried to add a tag that already exists.
            if sql_error.extended_code == 2067 {
                if let Some(value) = value {
                    err.with_context(||
                        format!("\"{path}\" already has tag \"{tag_name}\" with value \"{value}\"."))?;
                } else {
                    err.with_context(||
                        format!("error: \"{path}\" already has tag \"{tag_name}\"."))?;
                }
            } else {
                err?;
            }
        } else {
            err?;
        }

        Ok(())
    }

    /// Returns a list of the tags for a particular path.
    pub fn tags(&mut self, path: &str) -> Result<Vec<TagMapping>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT
                Tag.TagID, Tag.Name, Tag.TakesValue,
                TagMapping.Path, TagMapping.Value
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
                    path: row.get::<_, String>(3)?,
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

    /// Remove a tag from a path.
    pub fn untag(&mut self, path: &str, tag: &str, value: Option<&str>)
        -> Result<()>
    {
        if let Some(value) = value {
            self.conn.execute(
                "DELETE FROM TagMapping
                 WHERE TagMapping.Path = ? AND
                    TagMapping.Value = ? AND
                    TagMapping.TagID in
                        (SELECT Tag.TagID
                        FROM Tag
                        WHERE Tag.Name = ?)",
                rusqlite::params![path, value, tag]
            )?;
        } else {
            self.conn.execute(
                "DELETE FROM TagMapping
                 WHERE TagMapping.Path = ? AND
                    TagMapping.TagID in
                        (SELECT Tag.TagID
                        FROM Tag
                        WHERE Tag.Name = ?)",
                rusqlite::params![path, tag]
            )?;
        }
        Ok(())
    }

    pub fn untag_all(&mut self, path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM TagMapping
            WHERE TagMapping.Path = ?",
            rusqlite::params![path]
        )?;
        Ok(())
    }
}

/// Locates an existing TagFS database, or creates and intialises tables in a
/// new database.
pub fn get_or_create_db(path: &str) -> Result<Database> {
    let db = Connection::open(path)
        .map(|conn| Database { conn })?;

    db.initialise_tables()?;

    Ok(db)
}
