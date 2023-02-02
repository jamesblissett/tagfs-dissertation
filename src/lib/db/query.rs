//! Handles the user queries to the tag db.

use anyhow::Result;

static SQL_PARTIAL_SELECT_START: &str = "\
SELECT DISTINCT TagMapping.Path \
FROM TagMapping \
WHERE \
";

static SQL_PARTIAL_EQ_VALUE: &str = "\
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
)";

#[derive(Debug, PartialEq)]
enum Token {
    LeftParen,
    RightParen,
    And,
    Or,
    Not,
    Equals,
    LessThan,
    GreaterThan,
    Tag(String),
    Value(String),
}

impl Token {
    const fn is_comparison_operator(&self) -> bool {
        matches!(&self, Self::Equals | Self::LessThan | Self::GreaterThan)
    }
}

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
                if c == '=' { tokens.push(Token::Equals) }
                else if c == '<' { tokens.push(Token::LessThan) }
                else if c == '>' { tokens.push(Token::GreaterThan) }

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

// TODO: implement less than and greater than.
// TODO: implement a less strict match.
// TODO: implement proper query building errors.
/// Convert a stream of tokens into an SQL query.
///
/// # Warnings
/// This can result in some broken / nonsense queries being built, but there
/// _should_ be no risk of SQL injection due to no string interpolation of user
/// provided values.
fn to_sql(tokens: &[Token])
    -> std::result::Result<(String, Vec<String>), QueryBuildError>
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
                    Some(Equals) => {
                        let value = windows.next_if(|t| matches!(t, Value(_)));
                        if let Some(Value(value)) = value {
                            sql.push_str(SQL_PARTIAL_EQ_VALUE);

                            params.push(tag.clone());
                            params.push(value.clone());
                        } else {
                            Err(QueryBuildError)?;
                        }
                    }
                    Some(GreaterThan) => {
                        let value = windows.next_if(|t| matches!(t, Value(_)));
                        if let Some(Value(value)) = value {
                            eprintln!("{tag} > {value}");
                        } else {
                            Err(QueryBuildError)?;
                        }
                    }
                    Some(LessThan) => {
                        let value = windows.next_if(|t| matches!(t, Value(_)));
                        if let Some(Value(value)) = value {
                            eprintln!("{tag} < {value}");
                        } else {
                            Err(QueryBuildError)?;
                        }
                    }

                    // if there is no comparison operator then we just match
                    // against the existence of the tag.
                    None => {
                        sql.push_str(SQL_PARTIAL_EQ);

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
            Equals | LessThan | GreaterThan | Value(_) => {
                Err(QueryBuildError)?;
            }
        }
    }

    // this is insertion order, due to the incrementing behaviour of the key.
    sql.push_str(" ORDER BY TagMapping.TagMappingID");

    Ok((sql, params))
}

#[derive(Debug)]
pub struct Query {
    _raw: String,
    sql: String,
    params: Vec<String>,
}

impl Query {
    pub fn execute(self, db: &mut super::Database) -> Result<Vec<String>> {
        let mut stmt = db.conn.prepare_cached(&self.sql)?;
        let params = rusqlite::params_from_iter(self.params);

        let paths = stmt.query_map(params, |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(paths)
    }
}

#[derive(Debug)]
pub struct QueryBuildError;
impl std::fmt::Display for QueryBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "could not build query.")
    }
}
impl std::error::Error for QueryBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl std::str::FromStr for Query {
    type Err = QueryBuildError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let tokens = lex_query(s);
        let (sql, params) = to_sql(&tokens)?;

        Ok(Self { _raw: String::from(s), sql, params })
    }
}

#[cfg(test)]
mod tests {
    use super::Token::*;

    #[test]
    fn lex() {
        assert_eq!(
            super::lex_query("hello"),
            &[Tag(String::from("hello"))]
        );

        assert_eq!(
            super::lex_query("hello=world"),
            &[Tag(String::from("hello")), Equals, Value(String::from("world"))]
        );

        assert_eq!(
            super::lex_query("      (     hello   =world)"),
            &[
                LeftParen, Tag(String::from("hello")), Equals,
                Value(String::from("world")), RightParen
            ]
        );

        assert_eq!(
            super::lex_query("      (     not nothello   < \\(wor=ld)"),
            &[
                LeftParen, Not, Tag(String::from("nothello")), LessThan,
                Value(String::from("(wor=ld")), RightParen
            ]
        );

        assert_eq!(
            super::lex_query("      (     not nothello   > \"(wor\\\"=ld)\""),
            &[
                LeftParen, Not, Tag(String::from("nothello")), GreaterThan,
                Value(String::from("(wor\"=ld)"))
            ]
        );
    }
}
