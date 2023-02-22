//! Helper module that contains an extension trait for the [`anyhow::Result`]
//! type.

/// Extension trait for [`anyhow::Result`].
pub trait TagFSErrorExt {
    /// Should return true if the underlying error is a [`rusqlite::Error`] and
    /// it represents a failure of a unique constraint within the database.
    fn is_sql_unique_cons_err(&self) -> bool;
}

impl<T> TagFSErrorExt for anyhow::Result<T> {

    /// Returns true if the underlying error is a [`rusqlite::Error`] and it
    /// represents a failure of a unique constraint within the database.
    fn is_sql_unique_cons_err(&self) -> bool {
        if let Err(ref err) = &self {
            let err = err.downcast_ref::<rusqlite::Error>();
            if let Some(rusqlite::Error::SqliteFailure(sql_error, _)) = err {
                // Error code 2067 is a failure of a unique constraint.
                // This means we tried to add a tag that already exists.
                // This makes sense if we are retagging a directory tree,
                // so we can just ignore it.
                if sql_error.extended_code == 2067 {
                    return true;
                }
            }
        }
        false
    }
}
