use std::{
    ffi::OsString,
    path::Path,
};

use anyhow::Result;
use once_cell::sync::OnceCell;

static MOUNT_PATH: OnceCell<String> = OnceCell::new();
static DB_PATH: OnceCell<String> = OnceCell::new();

macro_rules! assert_symlink {
    ($path:expr) => {
        let path = format!("{}{}", MOUNT_PATH.get().unwrap(), $path);
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            assert!(meta.is_symlink(), "path {:?} is not a symlink.", $path);
        } else {
            panic!("path {:?} does not exist.", $path);
        }
    }
}

macro_rules! assert_symlink_target {
    ($path:expr, $target:expr) => {
        assert_symlink!($path);

        let path = format!("{}{}", MOUNT_PATH.get().unwrap(), $path);

        if let Ok(target) = std::fs::read_link(&path) {
            let target_path = Path::new(&$target);
            assert_eq!(target, target_path, "path {:?} has incorrect target",
                       $path);
        } else {
            panic!("path {:?} does not exist.", $path);
        }
    }
}

macro_rules! assert_file {
    ($path:expr) => {
        let path = format!("{}{}", MOUNT_PATH.get().unwrap(), $path);
        let meta = std::fs::metadata(&path);
        assert!(meta.is_ok(), "path {:?} does not exist.", $path);

        let meta = meta.unwrap();
        assert!(meta.is_file(), "path {:?} is not a regular file.", $path);
    }
}

macro_rules! assert_file_contents {
    ($path:expr, $contents:expr) => {
        assert_file!($path);
        let path = format!("{}{}", MOUNT_PATH.get().unwrap(), $path);

        // unwrap is fine because we assert file exists.
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, $contents, "contents of file {:?}.", $path);
    }
}

macro_rules! assert_dir_children {
    ($path:expr, $children:expr) => {
        let path = format!("{}{}", MOUNT_PATH.get().unwrap(), $path);

        let Ok(children) = std::fs::read_dir(&path) else {
            panic!("path \"{}\" does not exist or is not a directory.", $path);
        };

        let Ok(mut children) = children
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<std::io::Result<Vec<OsString>>>() else {
                panic!("error reading children of path \"{}\".", $path);
            };
        children.sort();
        assert_eq!(children.as_slice(), $children, "path \"{}\".", $path);
    }
}

#[test]
fn fs_runthrough() -> Result<()> {

    // the reason the temp directory is not cleaned up on drop is because the
    // filesystem is still mounted when the destructor code runs, therefore it
    // cannot be removed. This is not a huge issue, so I will leave it for now.
    let tmp_mount = mktemp::Temp::new_dir()?;
    MOUNT_PATH.set(tmp_mount.as_os_str().to_str().unwrap().to_owned())
        .unwrap();

    let tmp_db = mktemp::Temp::new_file()?;
    DB_PATH.set(tmp_db.as_os_str().to_str().unwrap().to_owned())
        .unwrap();

    // mount the filesystem in a background thread.
    std::thread::spawn(move || {
        let db_path = &DB_PATH.get().unwrap();
        let db = libtagfs::db::get_or_create_db(Some(db_path))?;

        let mount_path = &MOUNT_PATH.get().unwrap();
        libtagfs::fs::mount(mount_path, db)?;

        Ok::<(), anyhow::Error>(())
    });

    // sleep to wait for fs to mount not exactly great but seems to work.
    std::thread::sleep(std::time::Duration::from_millis(1000));

    let db_path = &DB_PATH.get().unwrap();
    let mut db = libtagfs::db::get_or_create_db(Some(db_path))?;

    db.tag("/my/very/cool/path", "hello", None)?;
    db.tag("/my/very/cool/path", "type", Some("awesome"))?;
    db.tag("/my/other/very/cool/file", "type", Some("cool"))?;
    db.tag("/my/other/super/cool/path", "type", Some("awesome"))?;
    db.tag("/my/other/very/cool/path", "type", Some("cool"))?;
    db.tag("/my/other/super/cool/file", "type", Some("awesome"))?;
    db.tag("/my/other/super/cool/file", "type", Some("cool"))?;

    assert_dir_children!("/", &["?", "hello", "tags", "type"]);

    assert_dir_children!("/hello/", &["path"]);
    assert_symlink_target!("/hello/path", "/my/very/cool/path");

    assert_dir_children!("/type", &["awesome", "cool"]);
    assert_dir_children!("/type/awesome/", &["file", "path.0", "path.1"]);

    db.untag("/my/very/cool/path", "hello", None)?;
    assert_dir_children!("/", &["?", "tags", "type"]);

    assert_dir_children!(
        "/?/type=awesome or type=cool/",
        &["file.0", "file.1", "path.0", "path.1", "path.2"]
    );

    assert_dir_children!("/?/type=awesome and type=cool/", &["file"]);

    assert_file_contents!(
        "/tags/my/other/super/cool/file.tags",
        "type=awesome\ntype=cool\n"
    );

    db.tag("/some/path/", "mytag", Some("a value with a / in it"))?;
    assert_symlink_target!("/mytag/a value with a _ in it/path", "/some/path");
    assert_dir_children!("/?/mytag", &["path"]);

    assert_dir_children!("/?", &[] as &[&str; 0]);

    db.create_stored_query("my-query", "type=cool")?;
    assert_dir_children!("/?", &["my-query @ [type=cool]"]);
    assert_dir_children!(
        "/?/my-query @ [type=cool]/",
        &["file.0", "file.1", "path"]
    );

    db.delete_stored_query("my-query")?;
    assert_dir_children!("/?", &[] as &[&str; 0]);

    Ok(())
}
