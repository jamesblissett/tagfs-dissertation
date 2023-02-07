//! Module that generates automatic tags from a path.

use std::path::Path;

use anyhow::Result;

use crate::db::TagValuePair;

/// When given a path, returns the list of automatically generated tags.
pub fn generate_autotags(path: &str) -> Result<Vec<TagValuePair>> {
    let mut tags = Vec::new();

    let path = Path::new(path);

    if let Some(ext) = path.extension() {
        if ext == "flac" || ext == "m4a" || ext == "mp3" {
            let metadata = audiotags::Tag::new().read_from_path(path)?;

            if let Some(album) = metadata.album() {
                tags.push(TagValuePair {
                    tag: String::from("album"),
                    value: Some(album.title.to_string())
                });
            }

            if let Some(album_artist) = metadata.album_artist() {
                tags.push(TagValuePair {
                    tag: String::from("albumartist"),
                    value: Some(album_artist.to_string())
                });
            }

        } else if ext == "mkv" || ext == "mp4" {

            // TODO: match against tmdb "Title (Year).mkv"

            tags.push(TagValuePair {
                tag: String::from("type"),
                value: Some(String::from("film"))
            });
        }
    }

    Ok(tags)
}
