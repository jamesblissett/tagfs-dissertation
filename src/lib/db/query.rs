//! Handles the user queries to the tag db.

use anyhow::Result;

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

            // Read characters until we peek an end of tag token. This also
            // doubles as reading a boolean operator.
            _ => {
                const fn end_of_tag(c: char) -> bool {
                    c == '=' || c == '<' || c == '>'
                    || c == ' ' || c == '(' || c == ')'
                }

                buf.push(c);

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

fn to_sql(tokens: &[Token]) -> Result<String> {
    let mut sql = String::from("");
    Ok(sql)
}

pub struct Query {
    raw: String,
}

#[derive(Debug)]
pub struct QueryParseError;
impl std::fmt::Display for QueryParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "could not parse query.")
    }
}
impl std::error::Error for QueryParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl std::str::FromStr for Query {
    type Err = QueryParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let tokens = lex_query(s);

        Ok(Self { raw: String::from(s) })
    }
}

#[cfg(test)]
mod tests {
    use super::Token::*;
    #[test]
    fn lex_test() {
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
