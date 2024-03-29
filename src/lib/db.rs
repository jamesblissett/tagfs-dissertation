//! Module that handles interfacing with the sqlite database.

mod query;
pub use query::{
    TagValuePair, ListFormatter, SimpleTagFormatter, EscapedTagFormatter,
};

mod edit_repr;
mod stored_query;
pub use stored_query::{SanitisedStoredQuery, StoredQuery};

use anyhow::{anyhow, bail, Context, Result};
use indexmap::map::IndexMap;
use rusqlite::Connection;

use crate::error::TagFSErrorExt;

/// Analogue to the database table.
#[derive(Debug)]
pub struct TagInfo {
    id: i64,
    pub name: String,
    pub takes_value: bool,
}

/// Analogue to the database table.
#[derive(Debug)]
pub struct TagMapping {
    _id: i64,
    pub tag: TagInfo,
    pub path: String,
    pub value: Option<String>,
    pub auto: bool,
}

pub trait Tag<'a> {
    fn tag(&'a self) -> &'a str;
    fn value(&'a self) -> Option<&'a str>;
}

impl<'a> Tag<'a> for TagMapping {
    fn tag(&'a self) -> &'a str {
        &self.tag.name
    }

    fn value(&'a self) -> Option<&'a str> {
        self.value.as_deref()
    }
}

/// Encapsulates all database logic.
#[derive(Debug)]
pub struct Database {
    /// Handle to the sqlite connection.
    conn: Connection,
}

impl Database {

    fn init(&self) -> Result<()> {
        self.conn.execute("PRAGMA foreign_keys = ON", [])?;

        self.initialise_tables()?;

        // remove unused tags if they are no longer referenced.
        // "OLD" references the row that was just deleted.
        self.conn.execute(
            "CREATE TRIGGER IF NOT EXISTS
             RemoveUnusedTags AFTER DELETE ON TagMapping
             BEGIN
                DELETE FROM Tag
                WHERE
                    Tag.TagID = OLD.TagID
                    AND NOT EXISTS(
                        SELECT TRUE
                        FROM TagMapping
                        WHERE TagMapping.TagID = OLD.TagID);
             END",
            []
        )?;

        Ok(())
    }

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
            Auto BOOL NOT NULL,
            ValueUniqConstraint GENERATED ALWAYS AS (COALESCE(Value, 'NULL')),
            FOREIGN KEY(TagID) REFERENCES Tag(TagID),
            UNIQUE(TagID, ValueUniqConstraint, Path)
        )")?;

        self.conn.execute_batch("CREATE TABLE IF NOT EXISTS StoredQueries (
            StoredQueryID INTEGER PRIMARY KEY,
            Name TEXT NOT NULL,
            Query TEXT NOT NULL,
            UNIQUE(Name)
        )")?;

        Ok(())
    }

    /// Return a list of all stored queries in the database.
    pub fn stored_queries(&self) -> Result<Vec<StoredQuery>> {
        let mut stmt = self.conn.prepare_cached("
            SELECT StoredQueries.Name, StoredQueries.Query FROM StoredQueries
        ")?;

        let stored_queries = stmt.query_map([], |row| Ok(StoredQuery {
            name: row.get(0)?, query: row.get(1)?
        }))?.collect::<rusqlite::Result<_>>()?;

        Ok(stored_queries)
    }

    /// Create a stored query in the database.
    pub fn create_stored_query(&mut self, name: &str, query: &str)
        -> Result<()>
    {
        self.conn.execute("
            INSERT INTO StoredQueries (Name, Query) VALUES (?, ?)",
            &[name, query]
        )?;

        Ok(())
    }

    /// Delete a stored query in the database by name. Returns whether any
    /// deletion occured.
    pub fn delete_stored_query(&mut self, name: &str) -> Result<bool> {
        let n = self.conn.execute("
            DELETE FROM StoredQueries WHERE StoredQueries.Name = ?",
            &[name]
        )?;

        Ok(n != 0)
    }

    /// This function tries to find a tag matching the str in the database, if
    /// it does not exist it returns None.
    pub fn get_tag(&self, tag: &str) -> Option<TagInfo> {
        self.conn.query_row(
            "SELECT TagID, Name, TakesValue FROM Tag WHERE Name = ?",
            rusqlite::params![tag],
            |row| Ok(TagInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                takes_value: row.get(2)?
            })
        ).ok()
    }

    /// This function creates a tag in the database.
    ///
    /// It can fail if a tag already exists with the same name.
    fn create_tag(&mut self, tag: &str, takes_value: bool) -> Result<TagInfo> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO Tag (Name, TakesValue) VALUES (?, ?)",
        )?;

        stmt.execute(rusqlite::params![tag, takes_value])?;

        Ok(TagInfo {
            id: self.conn.last_insert_rowid(),
            name: tag.to_string(),
            takes_value,
        })
    }

    /// Helper function to perform a manual tag.
    pub fn tag(&mut self, path: &str, tag_name: &str, value: Option<&str>)
        -> Result<()>
    {
        self.tag_inner(path, tag_name, value, false)
    }

    /// This function creates a mapping between a tag and a path in the
    /// database.
    ///
    /// It can fail if a mapping already exists with the same tag, path and
    /// value. It can also fail if the tag is required to take a value and one
    /// was not given or the opposite.
    fn tag_inner(&mut self, path: &str, tag_name: &str, value: Option<&str>,
                 auto: bool)
        -> Result<()>
    {
        let tag = if let Some(tag) = self.get_tag(tag_name) {
            if tag.takes_value && value.is_none() {
                bail!("tag \"{}\" takes a value but one was not given",
                      tag_name);
            } else if !tag.takes_value && value.is_some() {
                bail!("tag \"{}\" does not take a value but one was given",
                      tag_name);
            }
            tag
        } else {
            self.create_tag(tag_name, value.is_some())?
        };

        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO TagMapping (TagID, Path, Value, Auto) \
             VALUES (?, ?, ?, ?)"
        )?;

        let res = stmt.execute(rusqlite::params![tag.id, path, value, auto])
            .map_err(|e| anyhow!(e));

        if res.is_sql_unique_cons_err() {
            if let Some(value) = value {
                res.with_context(||
                    format!("\"{path}\" already has tag \
                             \"{tag_name}={value}\"."))?;
            } else {
                res.with_context(||
                    format!("\"{path}\" already has tag \"{tag_name}\"."))?;
            }
        } else {
            res?;
        }

        Ok(())
    }

    pub fn to_edit_repr(&self, out: &mut impl std::fmt::Write) -> Result<()> {
        edit_repr::to_edit_repr(self, out)
    }

    pub fn from_edit_repr(&mut self, input: &mut impl std::io::BufRead)
        -> Result<()>
    {
        // make sure to check the user input is valid before we drop the entire
        // database :)
        let path_map = edit_repr::from_edit_repr(input)?;

        // TODO: It is probably better to be smarter about this and not just
        //       drop the entire table. It would be useful if we could make
        //       this more granular.
        // NOTE: should not be necessary to clear the Tag table because the
        //       trigger should have already wiped it, but better to be safe
        //       than sorry.
        self.conn.execute_batch("
            DELETE FROM TagMapping;
            DELETE FROM Tag;
        ")?;

        for (path, tags) in path_map {
            for (auto, tag) in tags {
                self.tag_inner(&path, &tag.tag, tag.value.as_deref(), auto)?;
            }
        }

        Ok(())
    }

    /// Returns all tagmappings.
    fn dump(&self) -> Result<IndexMap<String, Vec<TagMapping>>> {
        let rows = self.tags_inner(None)?;
        let mut path_map: IndexMap<String, Vec<TagMapping>> = IndexMap::new();
        for row in rows {
            path_map.entry(row.path.clone()).or_default().push(row);
        }
        Ok(path_map)
    }

    /// Returns a list of the tags for a particular path.
    pub fn tags(&self, path: &str) -> Result<Vec<TagMapping>> {
        self.tags_inner(Some(path))
    }

    /// Returns a list of the tags for a particular path.
    fn tags_inner(&self, path: Option<&str>) -> Result<Vec<TagMapping>> {
        let mut params = Vec::new();

        let mut stmt = if let Some(path) = path {
            params.push(path);
            self.conn.prepare_cached(
                "SELECT
                    Tag.TagID, Tag.Name, Tag.TakesValue,
                    TagMapping.TagMappingID, TagMapping.Path, TagMapping.Value,
                    TagMapping.Auto
                FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
                WHERE TagMapping.Path = ?
                ORDER BY TagMapping.TagMappingID"
            )
        } else {
            self.conn.prepare_cached(
                "SELECT
                    Tag.TagID, Tag.Name, Tag.TakesValue,
                    TagMapping.TagMappingID, TagMapping.Path, TagMapping.Value,
                    TagMapping.Auto
                FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
                ORDER BY TagMapping.TagMappingID"
            )
        }?;

        let tags = stmt.query_map(rusqlite::params_from_iter(&params),
            |row| {
                Ok(TagMapping {
                    tag: TagInfo {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        takes_value: row.get(2)?,
                    },
                    _id: row.get(3)?,
                    path: row.get::<_, String>(4)?,
                    value: row.get(5)?,
                    auto: row.get(6)?,
                })
            })?.collect::<rusqlite::Result<_>>()?;

        Ok(tags)
    }

    /// Returns a list of paths tagged with a particular tag.
    pub fn paths_with_tag(&mut self, tag: &str, value: Option<&str>)
        -> Result<Vec<(String, u64)>>
    {
        let mut stmt = if value.is_some() {
            self.conn.prepare_cached(
                "SELECT TagMapping.Path, TagMapping.TagMappingID
                FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
                WHERE Tag.Name = ? AND TagMapping.Value = ?
                ORDER BY TagMapping.TagMappingID"
            )?
        } else {
            self.conn.prepare_cached(
                "SELECT TagMapping.Path, TagMapping.TagMappingID
                FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
                WHERE Tag.Name = ?
                ORDER BY TagMapping.TagMappingID"
            )?
        };

        let tags = if value.is_some() {
            stmt.query_map(rusqlite::params![tag, value],
                    |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()
        } else {
            stmt.query_map(rusqlite::params![tag],
                    |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()
        }?;

        Ok(tags)
    }

    /// Returns all values used for a particular tag.
    pub fn values(&mut self, tag: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT TagMapping.Value
            FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
            WHERE Tag.Name = ?
            ORDER BY TagMapping.TagMappingID ASC"
        )?;

        let values = stmt.query_map([tag], |row| row.get(0))?
                        .collect::<rusqlite::Result<_>>();
        values.or_else(|e| Err(e).with_context(||
            format!("tag \"{tag}\" does not take any values")))
    }

    /// Returns the names of all tags that appear in the database.
    pub fn all_tags(&mut self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT Name FROM Tag ORDER BY Tag.TagID"
        )?;

        let tags = stmt.query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;

        Ok(tags)
    }

    /// Remove a tag from a path.
    pub fn untag(&mut self, path: &str, tag: &str, value: Option<&str>)
        -> Result<()>
    {
        let n = if let Some(value) = value {
            self.conn.execute(
                "DELETE FROM TagMapping
                 WHERE TagMapping.Path = ? AND
                    TagMapping.Value = ? AND
                    TagMapping.TagID in
                        (SELECT Tag.TagID
                        FROM Tag
                        WHERE Tag.Name = ?)",
                rusqlite::params![path, value, tag]
            )?
        } else {
            self.conn.execute(
                "DELETE FROM TagMapping
                 WHERE TagMapping.Path = ? AND
                    TagMapping.TagID in
                        (SELECT Tag.TagID
                        FROM Tag
                        WHERE Tag.Name = ?)",
                rusqlite::params![path, tag]
            )?
        };

        if n == 0 {
            if let Some(value) = value {
                bail!("could not remove tag \"{tag}={value}\" from \
                       \"{path}\". Does it exist?");
            } else {
                bail!("could not remove tag \"{tag}\" from \"{path}\". \
                       Does it exist?");
            }
        }

        Ok(())
    }

    /// Remove all tags from a path.
    pub fn untag_all(&mut self, path: &str) -> Result<()> {
        let n = self.conn.execute(
            "DELETE FROM TagMapping
            WHERE TagMapping.Path = ?",
            rusqlite::params![path]
        )?;

        if n == 0 {
            bail!("could not remove tags from path \"{path}\". \
                   Does it exist?");
        }

        Ok(())
    }

    /// Build and execute a user query.
    pub fn query(&mut self, query: &str, case_sensitive: bool)
        -> Result<Vec<(String, u64)>>
    {
        let query = query::Query::from_raw(query, case_sensitive)?;

        query.execute(self)
            .map_err(|e| e.context("invalid query."))
    }

    pub fn paths_with_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let escaped_prefix = prefix
            .replace('%', "\\%")
            .replace('_', "\\_");

        let mut stmt = self.conn.prepare_cached("
            SELECT DISTINCT TagMapping.Path
            FROM TagMapping
            WHERE TagMapping.Path LIKE (? || '%') ESCAPE '\\'
            ORDER BY TagMapping.TagMappingID
        ")?;

        let paths = stmt.query_map(rusqlite::params![escaped_prefix],
            |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;

        Ok(paths)
    }

    /// Search and replace paths in the database that match the given
    /// old_prefix and replace it with the new_prefix.
    pub fn prefix_change(&mut self, old_prefix: &str, new_prefix: &str)
        -> Result<()>
    {
        let escaped_old_prefix = old_prefix
            .replace('%', "\\%")
            .replace('_', "\\_");

        self.conn.execute(
            "UPDATE TagMapping
            SET Path = REPLACE(TagMapping.Path, ?, ?)
            WHERE Path LIKE (? || '%') ESCAPE '\\' ",
            rusqlite::params![old_prefix, new_prefix, escaped_old_prefix]
        )?;
        Ok(())
    }

    /// Given a tag_mapping_id return the path associated with it in the
    /// database.
    pub fn get_path_from_id(&mut self, tag_mapping_id: u64) -> Result<String> {
        let path = self.conn.query_row(
            "SELECT TagMapping.Path
            FROM TagMapping
            WHERE TagMapping.TagMappingID = ?",
            rusqlite::params![tag_mapping_id],
            |row| row.get(0),
        )?;
        Ok(path)
    }

    /// Returns true if all paths in the database point to a real existing path
    /// in the filesystem.
    pub fn all_paths_valid(&self) -> Result<bool> {
        let mut stmt = self.conn.prepare_cached("
            SELECT DISTINCT TagMapping.Path FROM TagMapping
        ")?;

        let all_valid = stmt.query_map([], |row| row.get::<_, String>(0))?
            .all(|path| path.map_or(false,
                |path| camino::Utf8Path::new(&path).exists()));

        Ok(all_valid)
    }

    #[cfg(feature = "autotag")]
    /// Helper function to autotag a path.
    pub fn autotag(&mut self, path: &str, tag_name: &str, value: Option<&str>)
        -> Result<()>
    {
        self.tag_inner(path, tag_name, value, true)
    }
}

/// Locates an existing tagfs database, or creates and intialises tables in a
/// new database. \
/// If path is None the database is created in memory (useful for testing).
pub fn get_or_create_db(path: Option<&str>) -> Result<Database> {
    let conn = path.map_or_else(Connection::open_in_memory, Connection::open);

    let db = conn.map(|conn| Database { conn })?;

    db.init()?;

    Ok(db)
}
