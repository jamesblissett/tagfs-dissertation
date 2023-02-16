//! Handles serialising the database to the edit representation format.
//!
//! ```mono
//! ...
//! --------
//! /path/to/tagged/file
//! tag1=value
//! tag2
//! tag3=long\ value
//! --AUTO--
//! autotag=autovalue
//! --------
//! ...
//! ```
//!

use std::str::FromStr;

use anyhow::{Result, Context};
use indexmap::map::IndexMap;

use super::{Database, TagValuePair, query::EscapedTagFormatter};

/// Delimits the start and end of a block consisting of a path and a list of
/// tags.
const LINE_SEPARATOR: &str = "--------";

/// Delimits the switch to marking tags as auto.
const AUTOTAG_LINE_SEPARATOR: &str = "--AUTO--";

/// Prefix found at the beginning of a line that starts a comment.
const COMMENT_PREFIX: &str = "//";

/// Writes the contents of the database to the out buffer in the edit
/// representation format.
pub fn to_edit_repr(db: &Database, out: &mut impl std::fmt::Write) -> Result<()> {
    writeln!(out, "{COMMENT_PREFIX} You can edit this file as you please, but bear in mind to escape double")?;
    writeln!(out, "{COMMENT_PREFIX} quotes and spaces in tag values (or use double quotes around the value).")?;
    writeln!(out, "{COMMENT_PREFIX}")?;
    writeln!(out, "{COMMENT_PREFIX} All tags default to manual tags, however any tags after the \"{}\"", AUTOTAG_LINE_SEPARATOR)?;
    writeln!(out, "{COMMENT_PREFIX} header are marked as autotags.")?;

    let path_map = db.dump()?;

    for (path, tags) in path_map.into_iter() {
        writeln!(out)?;
        writeln!(out, "{}", LINE_SEPARATOR)?;
        writeln!(out, "{path}")?;

        // write the manual tags.
        for tag in tags.iter().filter(|tag| !tag.auto) {
            let tag_fmt = EscapedTagFormatter(&tag.tag.name, tag.value.as_ref());
            writeln!(out, "{}", tag_fmt)?;
        }

        // write the autotags.
        let mut autotags = tags.iter().filter(|tag| tag.auto).peekable();
        if autotags.peek().is_some() {
            writeln!(out, "{}", AUTOTAG_LINE_SEPARATOR)?;
        }
        for tag in autotags {
            let tag_fmt = EscapedTagFormatter(&tag.tag.name, tag.value.as_ref());
            writeln!(out, "{}", tag_fmt)?;
        }

        writeln!(out, "{}", LINE_SEPARATOR)?;
    }

    Ok(())
}

/// Read the edit representation back into a map which can be used to modify
/// the database accordingly.
pub fn from_edit_repr(input: &mut impl std::io::BufRead)
    -> Result<IndexMap<String, Vec<(bool, TagValuePair)>>>
{
    let mut path_map: IndexMap<String, Vec<(bool, TagValuePair)>> = IndexMap::new();

    let mut line_num = 0;

    let mut inside_block = false;
    let mut path = None;
    let mut auto = false;
    let mut tags = Vec::new();
    let mut buf = String::new();

    while let Ok(n) = input.read_line(&mut buf) {
        // when 0 bytes is returned we have exhausted the stream.
        if n == 0 { break; }

        line_num += 1;

        // trim trailing newline.
        if buf.ends_with('\n') { buf.pop(); }

        // skip any blank lines or comments.
        if buf.is_empty() || buf.chars().all(|c| c.is_whitespace())
            || buf.starts_with(COMMENT_PREFIX)
        {
            // pass

        // if block has ended.
        } else if buf == LINE_SEPARATOR && inside_block {
            inside_block = false;

            // insert the accumulated tags into the map with key path.
            if let Some(path) = path.take() {
                path_map.entry(path).or_default().append(&mut tags);
            }

        // if block has just started.
        } else if buf == LINE_SEPARATOR {
            inside_block = true;

        } else if inside_block && buf == AUTOTAG_LINE_SEPARATOR {
            auto = true;

        } else if inside_block && path.is_none() {
            path = Some(buf.clone())

        } else if inside_block {
            let tag = TagValuePair::from_str(&buf).with_context(||
                format!("could not parse tag on line {}.", line_num))?;

            tags.push((auto, tag));
        }

        // make sure to reset the buffer before the next iteration.
        buf.clear();
    }

    Ok(path_map)
}
