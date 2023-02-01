//! Tests for cli module.

use std::str::FromStr;

use super::{TagValuePair, TagValuePairParseError};

#[test]
fn tag_value_parse() -> Result<(), TagValuePairParseError> {

    let tag = TagValuePair::from_str("hello=world")?;
    assert_eq!(tag.tag, "hello");
    assert_eq!(tag.value.as_deref(), Some("world"));

    let tag = TagValuePair::from_str("hello=")?;
    assert_eq!(tag.tag, "hello");
    assert_eq!(tag.value.as_deref(), Some(""));

    let tag = TagValuePair::from_str("hello")?;
    assert_eq!(tag.tag, "hello");
    assert_eq!(tag.value.as_deref(), None);

    let tag = TagValuePair::from_str("actor=Julie Delpy")?;
    assert_eq!(tag.tag, "actor");
    assert_eq!(tag.value.as_deref(), Some("Julie Delpy"));

    let tag = TagValuePair::from_str("hello = world");
    assert!(tag.is_err());

    let tag = TagValuePair::from_str("  hello=world");
    assert!(tag.is_err());

    let tag = TagValuePair::from_str("he#$@=world");
    assert!(tag.is_err());

    Ok(())
}
