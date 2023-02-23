//! Module to handle stored queries.

pub struct StoredQuery {
    pub name: String,
    pub query: String,
}

impl std::fmt::Display for StoredQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} @ [{}]", self.name, self.query)
    }
}

pub struct SanitisedStoredQuery<'a>(&'a StoredQuery);

impl<'a> SanitisedStoredQuery<'a> {
    pub fn query(&self) -> &str {
        &self.0.query
    }

    pub fn name(&self) -> &str {
        &self.0.name
    }
}

impl<'a> std::fmt::Display for SanitisedStoredQuery<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} @ [", self.0.name)?;

        for c in self.0.query.chars() {
            match c {
                '/' => write!(f, "_")?,
                _ => write!(f, "{}", c)?,
            }
        }

        write!(f, "]")?;

        Ok(())
    }
}

impl<'a> From<&'a StoredQuery> for SanitisedStoredQuery<'a> {
    fn from(value: &'a StoredQuery) -> Self {
        Self(value)
    }
}
