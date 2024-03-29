use anyhow::Result;

use libtagfs::db::TagMapping;

#[test]
fn db_runthrough() -> Result<()> {
    let mut db = libtagfs::db::get_or_create_db(None)?;

    db.tag("hello", "cool-tag", None)?;

    let tags: Vec<_> = db.tags("hello")?.into_iter().map(|tag_mapping| {
        tag_mapping.tag.name
    }).collect();

    assert_eq!(tags, &["cool-tag"]);

    db.tag("goodbye", "cool-tag", None)?;

    let paths: Vec<_> = db.paths_with_tag("cool-tag", None)?
        .into_iter().map(|(path, _)| path).collect();
    assert_eq!(paths, &["hello", "goodbye"]);

    Ok(())
}

#[test]
fn db_untag() -> Result<()> {

    fn tags_mapping_helper(tags: &[TagMapping]) -> Vec<(&str, Option<&str>)> {
        tags.iter().map(|tag_mapping| {
            (tag_mapping.tag.name.as_str(), tag_mapping.value.as_deref())
        }).collect()
    }

    let mut db = libtagfs::db::get_or_create_db(None)?;

    let path = "Before Sunset (2004)";

    db.tag(path, "genre", Some("romance"))?;

    // TODO: we should probably have a proper error type (thiserror?) rather
    // than using anyhow for everything. Rather than use is_err it would be
    // nice to be able to use matches!() with a specific error enum variant.
    assert!(db.tag(path, "genre", Some("romance")).is_err());

    db.tag(path, "genre", Some("slice-of-life"))?;
    db.tag(path, "genre", Some("drama"))?;
    db.tag(path, "actor", Some("Ethan Hawke"))?;
    db.tag(path, "actor", Some("Julie Delpy"))?;

    let tags = db.tags(path)?;
    let tags = tags_mapping_helper(&tags);

    assert_eq!(tags, &[
        ("genre", Some("romance")), ("genre", Some("slice-of-life")),
        ("genre", Some("drama")),
        ("actor", Some("Ethan Hawke")), ("actor", Some("Julie Delpy")),
    ]);

    db.untag(path, "genre", Some("drama"))?;

    let tags = db.tags(path)?;
    let tags = tags_mapping_helper(&tags);

    assert_eq!(tags, &[
        ("genre", Some("romance")), ("genre", Some("slice-of-life")),
        ("actor", Some("Ethan Hawke")), ("actor", Some("Julie Delpy")),
    ]);

    db.untag(path, "genre", None)?;

    let tags = db.tags(path)?;
    let tags = tags_mapping_helper(&tags);

    assert_eq!(tags, &[
        ("actor", Some("Ethan Hawke")), ("actor", Some("Julie Delpy")),
    ]);

    let values: Vec<_> = db.values("actor")?.into_iter().collect();
    assert_eq!(values, &["Ethan Hawke", "Julie Delpy"]);

    db.untag_all(path)?;

    let tags = db.tags(path)?;
    assert!(tags.is_empty());

    Ok(())
}

#[test]
fn db_values() -> Result<()> {
    let mut db = libtagfs::db::get_or_create_db(None)?;

    db.tag("super-cool-film", "genre", Some("crime"))?;
    db.tag("another-super-cool-film", "genre", Some("crime"))?;

    let values = db.values("genre")?;

    assert_eq!(values, &["crime"]);

    db.tag("super-cool-film", "very-cool", None)?;

    assert!(db.values("very-cool").is_err());

    Ok(())
}

#[test]
fn db_query() -> Result<()> {
    let mut db = libtagfs::db::get_or_create_db(None)?;

    db.tag("Casino (1995)", "genre", Some("crime"))?;
    db.tag("Before Sunrise (1995)", "genre", Some("romance"))?;
    db.tag("Before Sunrise (1995)", "genre", Some("slice-of-life"))?;
    db.tag("Before Sunset (2004)", "genre", Some("romance"))?;
    db.tag("Heat (1995)", "genre", Some("crime"))?;

    db.tag("Casino (1995)", "favourite", None)?;
    db.tag("Before Sunrise (1995)", "favourite", None)?;

    db.tag("Before Sunrise (1995)", "actor", Some("Julie Delpy"))?;
    db.tag("Before Sunset (2004)", "actor", Some("Julie Delpy"))?;

    db.tag("Before Sunrise (1995)", "year", Some("1995"))?;
    db.tag("Before Sunset (2004)", "year", Some("2004"))?;
    db.tag("Casino (1995)", "year", Some("1995"))?;
    db.tag("Heat (1995)", "year", Some("1995"))?;

    let paths = db.query("genre==romance or not favourite and genre==crime", false)?
        .into_iter().map(|(path, _)| path).collect::<Vec<_>>();

    assert_eq!(paths, &[
        "Before Sunrise (1995)",
        "Before Sunset (2004)",
        "Heat (1995)",
    ]);

    let paths = db.query("genre==romance and favourite", false)?
        .into_iter().map(|(path, _)| path).collect::<Vec<_>>();

    assert_eq!(paths, &[
        "Before Sunrise (1995)",
    ]);

    let paths = db.query("not genre==crime", false)?
        .into_iter().map(|(path, _)| path).collect::<Vec<_>>();

    assert_eq!(paths, &[
        "Before Sunrise (1995)",
        "Before Sunset (2004)",
    ]);

    let paths = db.query("actor==\"Julie Delpy\"", false)?
        .into_iter().map(|(path, _)| path).collect::<Vec<_>>();

    assert_eq!(paths, &[
        "Before Sunrise (1995)",
        "Before Sunset (2004)",
    ]);

    // malformed query should result in error.
    assert!(db.query("actor \"Julie Delpy\"", false).is_err());

    let paths = db.query("actor==\"julie delpy\"", true)?;
    assert!(paths.is_empty());

    let paths = db.query("genre==ROMANCE", true)?;
    assert!(paths.is_empty());

    let paths = db.query("genre==rOMANCE", false)?
        .into_iter().map(|(path, _)| path).collect::<Vec<_>>();
    assert_eq!(paths, &[
        "Before Sunrise (1995)",
        "Before Sunset (2004)",
    ]);

    let paths = db.query("genre=romance and actor=deLpY and not genre=slice", false)?
        .into_iter().map(|(path, _)| path).collect::<Vec<_>>();

    assert_eq!(paths, &[
        "Before Sunset (2004)",
    ]);

    let paths = db.query("year > 1995", false)?
        .into_iter().map(|(path, _)| path).collect::<Vec<_>>();

    assert_eq!(paths, &[
        "Before Sunset (2004)",
    ]);

    let paths = db.query("year < 2000", false)?
        .into_iter().map(|(path, _)| path).collect::<Vec<_>>();

    assert_eq!(paths, &[
        "Casino (1995)",
        "Before Sunrise (1995)",
        "Heat (1995)",
    ]);

    Ok(())
}
