//! Tests for db module.

#[test]
fn db_query_debug() -> super::Result<()> {
    let mut db = super::get_or_create_db(None)?;

    db.tag("/media/hdd/film/Before Sunrise (1995)", "genre", Some("romance"))?;
    db.tag("/media/hdd/film/Before Sunrise (1995)", "genre", Some("slice-of-life"))?;
    db.tag("/media/hdd/film/Before Sunset (2004)", "genre", Some("romance"))?;
    db.tag("/media/hdd/film/Casino (1995)", "genre", Some("crime"))?;
    db.tag("/media/hdd/film/Heat (1995)", "genre", Some("crime"))?;

    db.tag("/media/hdd/film/Before Sunrise (1995)", "favourite", None)?;
    db.tag("/media/hdd/film/Casino (1995)", "favourite", None)?;

    let mut stmt = db.conn.prepare_cached(
        "SELECT DISTINCT TagMapping.Path
        FROM TagMapping
        WHERE
            TagMapping.Path IN (
                SELECT TagMapping.Path
                FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
                WHERE Tag.Name = ? AND TagMapping.Value = ?
            )
            OR
            (
            TagMapping.Path IN (
                SELECT TagMapping.Path
                FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
                WHERE Tag.Name = ?
            )
            AND
            TagMapping.Path IN (
                SELECT TagMapping.Path
                FROM TagMapping INNER JOIN Tag ON Tag.TagID = TagMapping.TagID
                WHERE Tag.Name = ? AND TagMapping.Value = ?
            )
            )
        ORDER BY TagMapping.TagMappingID"
    )?;

    let params = rusqlite::params_from_iter(vec![
        "genre", "romance", "favourite", "genre", "crime"
    ]);

    let paths = stmt.query_map(params, |row| {
        Ok(row.get::<_, String>(0)?)
    })?.collect::<rusqlite::Result<Vec<_>>>()?;

    assert_eq!(paths, &[
        "/media/hdd/film/Before Sunrise (1995)",
        "/media/hdd/film/Before Sunset (2004)",
        "/media/hdd/film/Casino (1995)",
    ]);

    Ok(())
}
