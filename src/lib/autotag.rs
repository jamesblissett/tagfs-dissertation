//! Module that generates automatic tags from a path.

use std::path::Path;

use anyhow::{anyhow, bail, Result, Context};
use log::{info, warn, trace};
use once_cell::sync::Lazy;
use regex::Regex;

use crate::db::{Database, TagValuePair};

static FILM_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        "^(?P<title>.*) \\((?P<year>[0-9]{4})\\)$"
    ).unwrap()
});

const TMDB_KEY_ENV_VAR_NAME: &str = "TMDB_KEY";

const TMDB_URL: &str = "https://api.themoviedb.org/3";
const TMDB_SEARCH_URL: &str = constcat::concat!(TMDB_URL, "/search/movie");

pub struct AutoTagger {
    tmdb_key: Option<String>,
}

impl AutoTagger {

    /// Create a new AutoTagger, tries to find a tmdb api key if one is not
    /// provided.
    pub fn new(tmdb_key: Option<String>) -> Self {
        let tmdb_key = tmdb_key.or_else(||
            std::env::var(TMDB_KEY_ENV_VAR_NAME).ok());

        Self { tmdb_key }
    }

    /// Generate and apply the autotags for a particular path.
    pub fn autotag(&self, path: &str, db: &mut Database) -> Result<()> {
        let path_p = Path::new(path);

        let Some(ext) = path_p.extension() else { return Ok(()) };

        if ext == "flac" || ext == "m4a" || ext == "mp3" {
            if let Ok(tags) = generate_music_tags(path) {
                Self::apply_tags(path, tags.as_slice(), db)?;
            }

        } else if ext == "mkv" || ext == "mp4" {
            // file_stem is filename without extension.
            let Some(film) = path_p.file_stem() else { return Ok(()) };

            // this unwrap will not panic because the path was created
            // from a str so it is definitely valid unicode.
            let film = film.to_str().unwrap();

            let Some(matches) = FILM_REGEX.captures(film) else { return Ok(()) };

            // it is okay to use the panicking versions of getting
            // a named group because the groups are not optional.
            let title = &matches["title"];
            let year = &matches["year"];

            // we only check to see if the tmdb key exists just
            // before we need to use it. This is to make sure that
            // we are not unnecessarily requiring it.
            let tmdb_key = self.tmdb_key.as_deref().ok_or_else(|| anyhow!(
                "missing TMDB api key. Please provide it with the {} environment variable or use the --tmdb-key flag.",
                TMDB_KEY_ENV_VAR_NAME
            ))?;

            // Check if there is directory that matches the film name to tag
            // instead of the mkv/mp4 file.
            let mut path = path;
            if let Some(parent_dir) = path_p.parent() {
                if parent_dir.ends_with(film) {
                    path = parent_dir.to_str().unwrap();
                }
            }

            let tags = generate_film_tags(title, year, tmdb_key);
            if let Ok(tags) = tags {
                Self::apply_tags(path, tags.as_slice(), db)?;
            }
        }

        Ok(())
    }

    /// Apply a list of tags to a path, whilst ignoring any duplicate tag
    /// errors.
    fn apply_tags(path: &str, tags: &[TagValuePair], db: &mut Database)
        -> Result<()>
    {
        if !tags.is_empty() {
            info!(
                "Autotagging \"{path}\" with tags: {}",
                crate::db::TagValuePairListFormatter(tags)
            );
        }

        for tag in tags {
            let res = db.autotag(path, &tag.tag, tag.value.as_deref());

            if let Err(ref err) = res {
                if let Some(rusqlite::Error::SqliteFailure(sql_error, _)) = err.downcast_ref::<rusqlite::Error>() {
                    // Error code 2067 is a failure of a unique constraint.
                    // This means we tried to add a tag that already exists.
                    // This makes sense if we are retagging a directory tree,
                    // so we can just ignore it.
                    if sql_error.extended_code == 2067 {
                        trace!("Path \"{path}\" is already tagged with tag \"{tag}\" ignoring...");
                        continue;
                    }
                }
            }
            res?
        }
        Ok(())
    }
}

/// When given a path to a music file, returns the list of automatically
/// generated tags.
fn generate_music_tags(path: &str) -> Result<Vec<TagValuePair>> {
    let mut tags = Vec::new();

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

    if let Some(year) = metadata.year() {
        tags.push(TagValuePair {
            tag: String::from("year"),
            value: Some(year.to_string())
        });
    }

    Ok(tags)
}

/// When given the title and year of a film, returns the list of automatically
/// generated tags.
fn generate_film_tags(title: &str, year: &str, tmdb_key: &str)
    -> Result<Vec<TagValuePair>>
{
    let mut tags = Vec::new();

    tags.push(TagValuePair {
        tag: String::from("type"),
        value: Some(String::from("film"))
    });

    tags.push(TagValuePair {
        tag: String::from("title"),
        value: Some(String::from(title))
    });

    tags.push(TagValuePair {
        tag: String::from("year"),
        value: Some(String::from(year))
    });

    // if getting the remote tags fails then we still want to tag the year and
    // title, so we log the error and move on.
    if let Err(e) = get_remote_film_tags(title, year, tmdb_key, &mut tags) {
        warn!("error getting cast and crew tags. Continuing anyway: {e:?}.");
    }

    Ok(tags)
}

/// Puts any tags into the tags out parameter.
fn get_remote_film_tags(title: &str, year: &str, tmdb_key: &str,
                        tags: &mut Vec<TagValuePair>)
    -> Result<()>
{
    let agent = ureq::AgentBuilder::new().build();

    let matches: serde_json::Value = agent.get(TMDB_SEARCH_URL)
        .query("api_key", tmdb_key)
        .query("query", title)
        .query("year", year)
        .call()
        .with_context(|| format!("failed to call the TMDB api. Is the network okay?"))?
        .into_json()
        .with_context(|| format!("failed to convert TMDB response into JSON. Is the network okay?"))?;

    let results = matches.get("results")
        .and_then(|results| results.as_array());

    let mut film = None;
    if let Some(results) = results {
        for m in results.iter() {
            let matches_title = m.get("title").map(|t| t == title) == Some(true);
            let matches_orig_title = m.get("original_title").map(|t| t == title) == Some(true);

            if matches_title || matches_orig_title {
                film = Some(m);
                break;
            }
        }
    }

    let Some(film) = film else {
        warn!("could not find an exact match for film \"{title} ({year})\", skipping autotagging...");
        bail!("could not find an exact match for film \"{title} ({year})\", skipping autotagging...");
    };

    let Some(tmdb_id) = film.get("id").and_then(|id| id.as_u64()) else {
        warn!("could not get TMDB id for film \"{title} ({year})\", skipping autotagging...");
        bail!("could not get TMDB id for film \"{title} ({year})\", skipping autotagging...");
    };

    trace!("Matched \"{title} ({year})\" to https://tmdb.org/movie/{tmdb_id}");

    // we don't care too much if these operations fails, so we just log and
    // move on.
    if let Err(e) = get_main_film_tags(tmdb_id, &agent, tmdb_key, tags) {
        warn!("error getting cast and crew tags. Continuing anyway: {e:?}.");
    }
    if let Err(e) = get_cast_and_crew_tags(tmdb_id, &agent, tmdb_key, tags) {
        warn!("error getting cast and crew tags. Continuing anyway: {e:?}.");
    }

    Ok(())
}

fn get_main_film_tags(tmdb_id: u64, agent: &ureq::Agent, tmdb_key: &str,
                          tags: &mut Vec<TagValuePair>)
    -> Result<()>
{
    let film_url = format!("{}/movie/{}", TMDB_URL, tmdb_id);
    let film_info: serde_json::Value = agent.get(&film_url)
        .query("api_key", tmdb_key)
        .call()
        .with_context(|| format!("failed to call the TMDB api. Is the network okay?"))?
        .into_json()
        .with_context(|| format!("failed to convert TMDB response into JSON. Is the network okay?"))?;

    // dbg!(&film_info);

    // TODO get languages and imdb/tmdb tags.

    let runtime = film_info.get("runtime")
        .and_then(|runtime| runtime.as_u64());
    if let Some(runtime) = runtime {
        tags.push(TagValuePair {
            tag: String::from("runtime"),
            value: Some(runtime.to_string())
        });
    }

    let mut genres = film_info.get("genres")
        .and_then(|genres| genres.as_array())
        .map(|genres| genres.iter().filter_map(|genre|
            genre.get("name").and_then(|genre| genre.as_str())));

    if let Some(ref mut genres) = genres {
        while let Some(genre) = genres.next() {
            tags.push(TagValuePair {
                tag: String::from("genre"),
                value: Some(genre.to_lowercase())
            });
        }
    }

    Ok(())
}

fn get_cast_and_crew_tags(tmdb_id: u64, agent: &ureq::Agent, tmdb_key: &str,
                          tags: &mut Vec<TagValuePair>)
    -> Result<()>
{
    let film_credits_url = format!("{}/movie/{}/credits", TMDB_URL, tmdb_id);
    let film_credits: serde_json::Value = agent.get(&film_credits_url)
        .query("api_key", tmdb_key)
        .call()
        .with_context(|| format!("failed to call the TMDB api. Is the network okay?"))?
        .into_json()
        .with_context(|| format!("failed to convert TMDB response into JSON. Is the network okay?"))?;

    let mut directors = film_credits.get("crew")
        .and_then(|crew| crew.as_array())
        .map(|crew| crew.iter().filter_map(|person|
            person.get("job").and_then(|job|
                if job == "Director" {
                    person.get("name").and_then(|name| name.as_str())
                } else {
                    None
                }
            )));

    if let Some(ref mut directors) = directors {
        while let Some(director) = directors.next() {
            tags.push(TagValuePair {
                tag: String::from("director"),
                value: Some(String::from(director))
            });
        }
    }

    let mut actors = film_credits.get("cast")
        .and_then(|cast| cast.as_array())
        .map(|cast| cast.iter().filter_map(|person|
            person.get("name").and_then(|name| name.as_str())));

    if let Some(ref mut actors) = actors {
        let mut actors = actors.take(5);
        while let Some(actor) = actors.next() {
            tags.push(TagValuePair {
                tag: String::from("actor"),
                value: Some(String::from(actor))
            });
        }
    }

    Ok(())
}
