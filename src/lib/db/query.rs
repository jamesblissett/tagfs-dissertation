//! Handles the user queries to the tag db.

#[cfg(test)]
mod tests;

use anyhow::{bail, Result};
use log::info;

use super::{Database, Tag};

static SQL_PARTIAL_SELECT_START: &str = "\
SELECT TagMapping.Path, TagMapping.TagMappingID \
FROM TagMapping \
WHERE \
";

static SQL_PARTIAL_STRICT_EQ_VALUE: &str = "\
TagMapping.Path IN ( \
    SELECT TagMapping.Path \
    FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID \
    WHERE Tag.Name = ? AND TagMapping.Value = ? \
    COLLATE NOCASE \
)";

static SQL_PARTIAL_STRICT_EQ_VALUE_CASE_SENS: &str = "\
TagMapping.Path IN ( \
    SELECT TagMapping.Path \
    FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID \
    WHERE Tag.Name = ? AND TagMapping.Value = ? \
)";

static SQL_PARTIAL_EQ: &str = "\
TagMapping.Path IN ( \
    SELECT TagMapping.Path \
    FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID \
    WHERE Tag.Name = ? \
    COLLATE NOCASE \
)";

static SQL_PARTIAL_EQ_CASE_SENS: &str = "\
TagMapping.Path IN ( \
    SELECT TagMapping.Path \
    FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID \
    WHERE Tag.Name = ? \
)";

static SQL_PARTIAL_EQ_VALUE: &str = "\
TagMapping.Path IN ( \
    SELECT TagMapping.Path \
    FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID \
    WHERE \
        Tag.Name = ? \
        AND TagMapping.Value LIKE ('%' || ? || '%') ESCAPE '\\' \
    COLLATE NOCASE \
)";

static SQL_PARTIAL_LT_VALUE: &str = "\
TagMapping.Path IN ( \
    SELECT TagMapping.Path \
    FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID \
    WHERE Tag.Name = ? AND TagMapping.Value < ? \
)";

static SQL_PARTIAL_GT_VALUE: &str = "\
TagMapping.Path IN ( \
    SELECT TagMapping.Path \
    FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID \
    WHERE Tag.Name = ? AND TagMapping.Value > ? \
)";

/// A lexed token.
#[derive(Debug, PartialEq)]
enum Token {
    LeftParen,
    RightParen,
    And,
    Or,
    Not,
    StrictEquals,
    Equals,
    LessThan,
    GreaterThan,
    Tag(String),
    Value(String),
    // TODO: maybe add an 'any' tag to search for values e.g. '_=hello'
    //       would match any tag that contains the value hello.
    // AnyTag,
}

impl Token {
    /// Returns whether this token is a comparison operator.
    const fn is_comparison_operator(&self) -> bool {
        matches!(&self, Self::StrictEquals | Self::Equals | Self::LessThan
            | Self::GreaterThan)
    }
}

// TODO: disallow forward slashes in tags and values.
/// Lex a raw query string into tokens.
///
/// # Warning
/// This function will very likely return nonsense if the query string is
/// nonsense. Any malformed expressions should hopefully be caught when the
/// list of tokens is transformed into SQL.
fn lex_query(query: &str) -> Vec<Token> {
    let mut tokens = Vec::new();

    // we use a peekable iterator because we need to see the next character
    // without consuming it.
    let mut chars = query.chars().peekable();
    let mut buf = String::new();

    while let Some(c) = chars.next() {
        match c {
            // whitespace outside of a quoted string is ignored.
            ' ' => {}

            // anytime we see a paren outside of a quoted string we can
            // directly push it to the tokens list.
            '(' => tokens.push(Token::LeftParen),
            ')' => tokens.push(Token::RightParen),

            // after any comparison operator we should see a value.
            '=' | '>' | '<' => {
                if c == '=' {
                    if chars.next_if(|&c| c == '=').is_some() {
                        tokens.push(Token::StrictEquals);
                    } else {
                        tokens.push(Token::Equals);
                    }
                }
                else if c == '<' { tokens.push(Token::LessThan); }
                else if c == '>' { tokens.push(Token::GreaterThan); }

                // Skip leading whitespace
                while chars.next_if(|&c| c == ' ').is_some() {}

                // Consume the next character if it is a double quote.
                let quoted_literal = chars.next_if(|&c| c == '"').is_some();

                // Whether the next character has been escaped.
                let mut escaped = false;

                // Loop over the characters until we notice a termination
                // point. This is an unescaped double quote if we are in a
                // quoted literal, or it is an unescaped paren or unescaped
                // space if we are not in a quoted literal.
                while let Some(c) = chars.next_if(|c| {
                    let end_of_value = if quoted_literal {
                        *c == '"' && !escaped
                    } else {
                        !escaped && (*c == ')' || *c == ' ')
                    };

                    !end_of_value
                })
                {
                    // a character is escaped if it is preceded by a backslash
                    // and it is not already escaped.
                    if c == '\\' && !escaped {
                        escaped = true;
                    } else {
                        escaped = false;
                        buf.push(c);
                    }
                }

                // Consume the next character if it is a double quote.
                chars.next_if(|&c| c == '"');

                // Push what we have accumulated as a value.
                tokens.push(Token::Value(buf.clone()));
            }
            _ => {
                const fn end_of_tag(c: char) -> bool {
                    c == '=' || c == '<' || c == '>'
                    || c == ' ' || c == '(' || c == ')'
                }

                buf.push(c);

                // Read characters until we peek an end of tag token. This also
                // doubles as reading a boolean operator.
                while let Some(c) = chars.next_if(|&c| !end_of_tag(c)) {
                    buf.push(c);
                }

                match buf.as_str() {
                    "not" | "NOT" => tokens.push(Token::Not),
                    "and" | "AND" => tokens.push(Token::And),
                    "or" | "OR" => tokens.push(Token::Or),
                    _ => tokens.push(Token::Tag(buf.clone())),
                }
            }
        }
        buf.clear();
    }
    tokens
}

// TODO: for the unstrict match use deunicode to match unicode chars with
// ascii. We can store a column in the database with this search data.
// TODO: implement proper query building errors.
/// Convert a stream of tokens into an SQL query.
///
/// # Warnings
/// This can result in some broken / nonsense queries being built, but there
/// _should_ be no risk of SQL injection due to no string interpolation of user
/// provided values.
fn to_sql(tokens: &[Token], case_sensitive: bool)
    -> Result<(String, Vec<String>)>
{
    use Token::*;

    let mut sql = String::from(SQL_PARTIAL_SELECT_START);
    let mut params = Vec::new();

    let mut windows = tokens.iter().peekable();
    while let Some(token) = windows.next() {
        match token {

            // A tag must either be followed by a comparison operator and then
            // a value, or it stands alone.
            Tag(tag) => {
                match windows.next_if(|t| t.is_comparison_operator()) {
                    Some(StrictEquals) => {
                        let value = windows.next_if(|t| matches!(t, Value(_)));
                        if let Some(Value(value)) = value {

                            if case_sensitive {
                                sql.push_str(
                                    SQL_PARTIAL_STRICT_EQ_VALUE_CASE_SENS);
                            } else {
                                sql.push_str(SQL_PARTIAL_STRICT_EQ_VALUE);
                            }

                            params.push(tag.clone());
                            params.push(value.clone());
                        } else {
                            bail!("expected value after == operator.");
                        }
                    }
                    // non-strict equals is always case insensitive regardless
                    // of the user flag.
                    Some(Equals) => {
                        let value = windows.next_if(|t| matches!(t, Value(_)));
                        if let Some(Value(value)) = value {

                            let escaped_value = value.replace('%', "\\%")
                                .replace('_', "\\_");

                            sql.push_str(SQL_PARTIAL_EQ_VALUE);

                            params.push(tag.clone());
                            params.push(escaped_value);
                        } else {
                            bail!("expected value after = operator.");
                        }
                    }
                    Some(GreaterThan) => {
                        let value = windows.next_if(|t| matches!(t, Value(_)));
                        if let Some(Value(value)) = value {
                            sql.push_str(SQL_PARTIAL_GT_VALUE);

                            params.push(tag.clone());
                            params.push(value.clone());
                        } else {
                            bail!("expected value after > operator.");
                        }
                    }
                    Some(LessThan) => {
                        let value = windows.next_if(|t| matches!(t, Value(_)));
                        if let Some(Value(value)) = value {
                            sql.push_str(SQL_PARTIAL_LT_VALUE);

                            params.push(tag.clone());
                            params.push(value.clone());
                        } else {
                            bail!("expected value after < operator.");
                        }
                    }

                    // if there is no comparison operator then we just match
                    // against the existence of the tag.
                    None => {
                        if case_sensitive {
                            sql.push_str(SQL_PARTIAL_EQ_CASE_SENS);
                        } else {
                            sql.push_str(SQL_PARTIAL_EQ);
                        }

                        params.push(tag.clone());
                    }
                    _ => unreachable!(),
                }
            }

            // we can just naïvely insert these tokens, because the query has a
            // similar structure to the SQL query. If the usage of these
            // operators is incorrect then we will produce invalid SQL which
            // will be rejected by SQLite.
            And => sql.push_str(" AND "),
            Or => sql.push_str(" OR "),
            Not => sql.push_str(" NOT "),
            LeftParen => sql.push_str(" ( "),
            RightParen => sql.push_str(" ) "),

            // we should only see these tokens after a tag, otherwise it is an
            // error.
            Value(_) => {
                bail!("unexpected value.");
            }
            StrictEquals | Equals | LessThan | GreaterThan => {
                bail!("unexpected comparison operator.");
            }
        }
    }

    // we group by the path to ensure we only get one match for each path
    sql.push_str(" GROUP BY TagMapping.Path");

    // this is insertion order, due to the incrementing behaviour of the key.
    sql.push_str(" ORDER BY TagMapping.TagMappingID");

    Ok((sql, params))
}

/// Required by clap to parse a tag value pair. \
/// Used when the tag value pair is not in the correct format.
#[derive(Clone, Debug)]
pub struct TagValuePairParseError;

impl std::fmt::Display for TagValuePairParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not in the required tag(=value)? format.")
    }
}

impl std::error::Error for TagValuePairParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

/// Contains a simple tag value pair parsed from user input.
#[derive(Clone, Debug)]
pub struct TagValuePair {
    pub tag: String,
    pub value: Option<String>,
}

impl<'a> Tag<'a> for TagValuePair {
    fn tag(&'a self) -> &'a str {
        &self.tag
    }

    fn value(&'a self) -> Option<&'a str> {
        self.value.as_deref()
    }
}

impl std::str::FromStr for TagValuePair {
    type Err = TagValuePairParseError;

    /// Parses a tag value pair from a string as a subset of a query.
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let mut tokens = lex_query(s);

        match &mut *tokens {
            [Token::Tag(tag), Token::Equals, Token::Value(value)] => {
                let tag = std::mem::take(tag);
                let value = Some(std::mem::take(value));
                Ok(Self { tag, value })
            }
            [Token::Tag(tag)] => {
                let tag = std::mem::take(tag);
                Ok(Self { tag, value: None })
            }
            _ => Err(TagValuePairParseError),
        }
    }
}

/// Newtype wrapper struct to print a tag mapping.
///
/// A newtype is used (rather than implementing [`std::fmt::Display`] directly
/// on [`TagMapping`]) because it allows the easy creation of multiple
/// differing Display implementations.
pub struct SimpleTagFormatter<'a, T: Tag<'a>>(&'a T);

impl<'a, T: Tag<'a>> std::fmt::Display for SimpleTagFormatter<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(value) = &self.0.value() {
            write!(f, "{}={}", self.0.tag(), value)
        } else {
            write!(f, "{}", self.0.tag())
        }
    }
}

impl<'a, T: Tag<'a>> From<&'a T> for SimpleTagFormatter<'a, T> {
    fn from(tag: &'a T) -> Self {
        Self(tag)
    }
}

/// Implements a version of display for a [`TagValuePair`] or
/// [`TagMapping`] where the value of tags are correctly escaped.
pub struct EscapedTagFormatter<'a, T: Tag<'a>>(&'a T);

impl<'a, T: Tag<'a>> std::fmt::Display for EscapedTagFormatter<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(value) = self.0.value() {
            write!(f, "{}=", self.0.tag())?;

            for c in value.chars() {
                match c {
                    '"' | ' ' | '\\' | '(' | ')' => write!(f, "\\{c}")?,
                    c => write!(f, "{c}")?,
                }
            }

            Ok(())
        } else {
            write!(f, "{}", self.0.tag())
        }
    }
}

impl<'a, T: Tag<'a>> From<&'a T> for EscapedTagFormatter<'a, T> {
    fn from(tag: &'a T) -> Self {
        Self(tag)
    }
}

/// Wrapper struct to implement Display on a list of tags.
pub struct ListFormatter<XS, X>(XS)
where XS: Iterator<Item = X> + Clone,
      X: std::fmt::Display;

impl<XS, X> std::fmt::Display for ListFormatter<XS, X>
where X: std::fmt::Display, XS: Iterator<Item = X> + Clone
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut xs = self.0.clone();
        if let Some(tag) = xs.next() {
            write!(f, "\"{tag}\"")?;

            for tag in xs {
                write!(f, ", \"{tag}\"")?;
            }
        }
        Ok(())
    }
}

impl<XS, X> From<XS> for ListFormatter<XS, X>
where X: std::fmt::Display, XS: Iterator<Item = X> + Clone
{
    fn from(value: XS) -> Self {
        Self(value)
    }
}

/// Ready to execute query.
#[derive(Debug)]
pub struct Query {
    _raw: String,
    sql: String,
    params: Vec<String>,
}

impl Query {

    /// Runs the query on the provided database and returns the list of paths
    /// that match.
    pub fn execute(self, db: &mut Database) -> Result<Vec<(String, u64)>> {
        let mut stmt = db.conn.prepare_cached(&self.sql)?;
        let params = rusqlite::params_from_iter(self.params);

        let paths = stmt.query_map(params, |row| {
            Ok((row.get::<_, String>(0)?, row.get(1)?))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(paths)
    }

    // To build a query it is simply split into tokens and converted directly
    // to SQL. There is no need for proper parsing because the query language
    // has the same structure as the SQL query.
    pub fn from_raw(s: &str, case_sensitive: bool) -> Result<Self> {
        let tokens = lex_query(s);

        info!("Lexed query \"{s}\" as {:?}", tokens);

        let (sql, params) = to_sql(&tokens, case_sensitive)?;

        Ok(Self { _raw: String::from(s), sql, params })
    }
}
