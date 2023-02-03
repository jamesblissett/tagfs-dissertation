use std::str::FromStr;

use super::Token::*;
use super::TagValuePair;

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

    assert_eq!(
        super::lex_query("      (     not > \"(wor\\\"=ld)\""),
        &[
            LeftParen, Not, GreaterThan,
            Value(String::from("(wor\"=ld)"))
        ]
    );
}

#[test]
fn tag_value_parse() -> Result<(), super::TagValuePairParseError> {

    let tag = TagValuePair::from_str("hello=world")?;
    assert_eq!(tag.tag, "hello");
    assert_eq!(tag.value.as_deref(), Some("world"));

    let tag = TagValuePair::from_str("hello=")?;
    assert_eq!(tag.tag, "hello");
    assert_eq!(tag.value.as_deref(), Some(""));

    let tag = TagValuePair::from_str("hello")?;
    assert_eq!(tag.tag, "hello");
    assert_eq!(tag.value.as_deref(), None);

    let tag = TagValuePair::from_str("actor=Julie Delpy");
    assert!(tag.is_err());

    let tag = TagValuePair::from_str("actor=Julie\\ Delpy")?;
    assert_eq!(tag.tag, "actor");
    assert_eq!(tag.value.as_deref(), Some("Julie Delpy"));

    let tag = TagValuePair::from_str("hello = world")?;
    assert_eq!(tag.tag, "hello");
    assert_eq!(tag.value.as_deref(), Some("world"));

    let tag = TagValuePair::from_str("  hello=world")?;
    assert_eq!(tag.tag, "hello");
    assert_eq!(tag.value.as_deref(), Some("world"));

    let tag = TagValuePair::from_str("he#$@=world")?;
    assert_eq!(tag.tag, "he#$@");
    assert_eq!(tag.value.as_deref(), Some("world"));

    Ok(())
}
