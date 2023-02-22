//! Module that contains the fuse file system for tagfs.
//!
//! The general order in which the FUSE functions are called is as follows:
//!
//! getattr(inode: [`FUSE_ROOT_ID`]) -> [`fuser::FileAttr`] \
//! readdir(inode: [`FUSE_ROOT_ID`], offset: 0) -> [(inode, type, name)] \
//!
//! Then for each name returned by readdir \
//! lookup(name: x, parent_inode: [`FUSE_ROOT_ID`]) -> [`fuser::FileAttr`]

mod inode_generator;
mod entries;

use inode_generator::INodeGenerator;
use entries::{Entries, EntryType};

use std::{
    borrow::Cow, collections::BTreeMap, fmt::Write, iter::Iterator, ffi::OsStr,
};

use fuser::{
    FileType, FUSE_ROOT_ID, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    ReplyOpen, Request
};
use log::{error, info};
use once_cell::sync::Lazy;

use crate::db::Database;

static TTL: std::time::Duration = std::time::Duration::from_secs(1);

/// Initialised to the time when the filesystem was mounted. Used as the *times
/// on files and directories.
static MOUNT_TIME: Lazy<std::time::SystemTime> = Lazy::new(|| {
    std::time::SystemTime::now()
});

/// Filesystem struct that implements the [`fuser::Filesystem`] trait.
///
/// Many of the methods for readdir take an optional [`fuser::ReplyDirectory`].
/// This parameter is optional because it allows us to reuse these methods to
/// create the internal entries even without a [`fuser::ReplyDirectory`]. This
/// means we can call the readdir helper methods on lookup() calls so that we
/// can use their side effects.
#[derive(Debug)]
struct TagFS {
    entries: Entries,
    db: Database,
}

impl TagFS {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            entries: Entries::new(),
        }
    }

    /// Helper function to reply with the root directory entries.
    fn readdir_root(&mut self, offset: i64, mut reply: Option<ReplyDirectory>)
    {
        let static_dirs = &[
            self.entries.get_or_create_query_directory(),
            self.entries.get_or_create_all_tags_dir(),
        ];

        if offset < static_dirs.len() as i64 {
            let offset_dirs = static_dirs.iter()
                .enumerate().skip(offset as usize);

            for (idx, dir_inode) in offset_dirs {
                let done = reply.as_mut().map_or(false, |reply|
                    reply.add(*dir_inode, (idx + 1) as i64,
                        FileType::Directory,
                        self.entries.get_name(*dir_inode)));

                if done {
                    if let Some(reply) = reply { reply.ok() }
                    return;
                }
            }
        }

        if let Ok(tags) = self.db.all_tags() {
            for (idx, tag) in tags.iter().enumerate().skip(offset as usize) {
                let child_inode = self.entries.get_or_create_tag_directory(
                    FUSE_ROOT_ID, tag
                );

                // we add two to the index to account for the query directory.
                let done = reply.as_mut().map_or(false, |reply|
                    reply.add(child_inode,
                        (idx + 1 + static_dirs.len()) as i64,
                        FileType::Directory, tag));

                if done { break; }
            }
        }
        if let Some(reply) = reply { reply.ok() }
    }

    /// Helper function to reply with the entries for a particular tag value
    /// pair.
    fn readdir_files(&mut self, tag: &str, value: Option<&str>, inode: u64,
                     offset: i64, mut reply: Option<ReplyDirectory>)
    {
        if let Ok(children) = self.db.paths_with_tag(tag, value) {

            let children_offset = children.iter()
                .enumerate().skip(offset as usize);
            for (idx, (child, child_id)) in children_offset {

                let siblings = children.iter().map(|(child, _)| child);
                let display_name = sanitise_path(child, idx, siblings);

                let child_inode = self.entries.try_get_link_inode(inode,
                    display_name.as_ref(), *child_id)
                        .unwrap_or_else(||
                            self.entries.create_link(inode,
                                display_name.as_ref(), *child_id,
                                child.len() as u64));

                let done = reply.as_mut().map_or(false, |reply|
                    reply.add(child_inode, (idx + 1) as i64,
                        FileType::Symlink, display_name.as_str()));

                if done { break; }
            }
        }
        if let Some(reply) = reply { reply.ok() }
    }

    /// Helper function to reply with all the values for a particular tag.
    fn readdir_values(&mut self, tag: &str, inode: u64, offset: i64,
                      mut reply: Option<ReplyDirectory>)
    {
        if let Ok(children) = self.db.values(tag) {

            let children_offset = children.iter()
                .enumerate().skip(offset as usize);
            for (idx, child) in children_offset {

                let display_name = sanitise_value(child);

                let child_inode = self.entries.try_get_inode(inode,
                    display_name.as_ref())
                        .unwrap_or_else(||
                            self.entries.get_or_create_value_directory(inode,
                                display_name.as_ref(), child));

                let done = reply.as_mut().map_or(false, |reply|
                    reply.add(child_inode, (idx + 1) as i64,
                        FileType::Directory, display_name.as_str()));

                if done { break; }
            }
        }
        if let Some(reply) = reply { reply.ok() }
    }

    /// Helper function to look up the root inode and create its children if
    /// necessary. The reason we return the inode of the child is so that we
    /// can pre-emptively call readdir with that inode to 'preload' the
    /// entries.
    fn lookup_root_child(&mut self, parent: u64, name: &str, reply: ReplyEntry)
        -> Option<u64>
    {
        let matches_tag = self.db.all_tags()
            .map_or(false, |tags| tags.iter().any(|tag| tag == name));

        // the children of the root directory are either the name of a tag, or
        // the query directory.
        if matches_tag {
            let inode = self.entries.get_or_create_tag_directory(parent, name);
            reply.entry(&TTL, self.entries.get_attr(inode), 0);
            Some(inode)

        } else {
            let query_dir_inode = self.entries.get_or_create_query_directory();
            let all_tags_dir_inode = self.entries.get_or_create_all_tags_dir();

            if name == self.entries.get_name(query_dir_inode) {
                reply.entry(&TTL, self.entries.get_attr(query_dir_inode), 0);
                Some(query_dir_inode)

            } else if name == self.entries.get_name(all_tags_dir_inode) {
                reply.entry(&TTL,
                    self.entries.get_attr(all_tags_dir_inode), 0);
                Some(all_tags_dir_inode)

            } else {
                reply.error(libc::ENOENT);
                None
            }
        }
    }

    /// Helper function to reply with all the paths for a particular query.
    fn readdir_query(&mut self, inode: u64, offset: i64,
                     mut reply: Option<ReplyDirectory>)
    {
        let query = self.entries.get_query(inode);
        // the else case _should_ never happen because we have
        // already rejected any invalid queries.
        if let Ok(paths) = self.db.query(query, false) {

            let paths_offset = paths.iter().enumerate().skip(offset as usize);
            for (idx, (child, child_id)) in paths_offset {

                let siblings = paths.iter().map(|(child, _)| child);
                let display_name = sanitise_path(child, idx, siblings);

                let child_inode = self.entries.try_get_link_inode(inode,
                    display_name.as_ref(), *child_id)
                        .unwrap_or_else(||
                            self.entries.create_link(
                                inode, display_name.as_ref(),
                                *child_id, child.len() as u64
                            ));

                let done = reply.as_mut().map_or(false, |reply|
                    reply.add(child_inode, (idx + 1) as i64,
                        FileType::Symlink, display_name.as_str()));

                if done { break; }
            }
        }

        if let Some(reply) = reply { reply.ok() }
    }

    fn readdir_query_dir(&mut self, offset: i64,
                         mut reply: Option<ReplyDirectory>)
    {
        // TODO: put these into the database.
        let stored_queries = vec![
            ("medium-watch",
                "runtime > 100 and runtime < 130 and not watched"),
            ("unwatched",
                "not watched and type=film"),
        ];
        let stored_queries = stored_queries.iter()
            .map(|(name, query)| (format!("{name} @ [{query}]"), query));

        let stored_queries_offset = stored_queries
            .enumerate().skip(offset as usize);

        for (idx, (name, query)) in stored_queries_offset {
            let child_inode = self.entries.get_or_create_query_result_dir(
                query, &name);

            let done = reply.as_mut().map_or(false, |reply|
                reply.add(child_inode, (idx + 1) as i64,
                    FileType::Symlink, &name));

            if done { break; }
        }

        if let Some(reply) = reply { reply.ok() }
    }

    /// Read a directory in the AllTags hierarchy. Mirrors the user's paths and
    /// terminates them with a file containing the tags on that path.
    fn readdir_all_tags(&mut self, inode: u64, offset: i64,
                        mut reply: Option<ReplyDirectory>)
    {
        let path = self.entries.get_path(inode);

        let mut uniq_names: BTreeMap<Cow<str>, _> = BTreeMap::new();

        if let Ok(children) = self.db.paths_with_prefix(path) {
            for child in &children {
                let Some(child_stripped) = child.strip_prefix(path) else {
                    error!("programming error - path: \"{child}\" must have \
                            prefix: \"{path}\".");
                    panic!("programming error - path: \"{child}\" must have \
                            prefix: \"{path}\".");
                };

                if let Some(split_point) = child_stripped.find('/') {
                    let name = &child_stripped[0..split_point];
                    uniq_names.insert(
                        Cow::from(name),
                        (&child[0 .. path.len() + split_point + 1],
                            FileType::Directory))
                    ;
                } else {
                    uniq_names.insert(
                        Cow::from(format!("{child_stripped}.tags")),
                        (child, FileType::RegularFile)
                    );
                }
            }

            let uniq_names_offset = uniq_names.iter()
                .enumerate()
                .skip(offset as usize);

            for (idx, (name, (path, kind))) in uniq_names_offset {
                let child_inode = if *kind == FileType::Directory {
                    self.entries.get_or_create_all_tags_intermediate(inode,
                        name, path)
                } else {
                    self.entries.get_or_create_all_tags_terminal(inode,
                        name, path)
                };

                let done = reply.as_mut().map_or(false, |reply|
                    reply.add(child_inode, (idx + 1) as i64,
                        *kind, name.as_ref()));

                if done { break; }
            }
        }

        if let Some(reply) = reply { reply.ok() }
    }

    /// Helper function that is called by both readdir and lookup.
    ///
    /// Creates the child inodes of a particular directory.
    ///
    /// Can be called with reply as None to do a 'fake' readdir for its side
    /// effects only.
    fn readdir_helper(&mut self, inode: u64, offset: i64,
                      reply: Option<ReplyDirectory>)
    {
        let attr = self.entries.get_attr(inode);

        if attr.kind == FileType::Directory {
            let name = self.entries.get_name(inode);

            match self.entries.get_type(inode) {
                EntryType::TagDir => {
                    if let Some(tag) = self.db.get_tag(name) {
                        if tag.takes_value {
                            self.readdir_values(&tag.name, inode,
                                offset, reply);
                        } else {
                            self.readdir_files(&tag.name, None,
                                inode, offset, reply);
                        }
                    } else {
                        panic!("programming error: tried to readdir for a \
                                directory without a tag in the database.");
                    }
                }
                EntryType::ValueDir => {
                    let tag_name = self.entries.get_parent_tag(inode)
                        .to_string();
                    let value = self.entries.get_tag_value(inode).to_string();
                    self.readdir_files(&tag_name, Some(&value),
                        inode, offset, reply);
                }

                // query dir contains stored queries and nothing else.
                EntryType::QueryDir => {
                    self.readdir_query_dir(offset, reply);
                }

                EntryType::QueryResultDir => {
                    self.readdir_query(inode, offset, reply);
                }

                EntryType::AllTagsDir | EntryType::AllTagsIntermediate => {
                    self.readdir_all_tags(inode, offset, reply);
                }

                // cannot readdir something that is not a directory and the
                // root is already covered.
                EntryType::Root | EntryType::Link | EntryType::AllTagsTerminal
                    => unreachable!(),
            }
        }
    }
}

impl fuser::Filesystem for TagFS {

    // look up inode and get its attrs.
    fn getattr(&mut self, _req: &Request<'_>, inode: u64, reply: ReplyAttr) {
        info!("getattr(inode: {inode:#x?})");

        reply.attr(&TTL, self.entries.get_attr(inode));
    }

    // tells the caller if a file with parent and name exists.
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr,
              reply: ReplyEntry)
    {
        info!("lookup(parent_ino: {parent:#x?}, name: {name:?})");

        // for now at least, we are only going to allow valid unicode.
        let name = name.to_str().unwrap_or_else(|| {
                error!("tried to lookup non-unicode name \"{name:?}\"");
                panic!("tried to lookup non-unicode name \"{name:?}\"");
            });

        // We call the readdir helper functions to ensure that we create the
        // child inodes as soon as possible. This ensures that even if readdir
        // is not called, we can operate with just lookup.
        if parent == FUSE_ROOT_ID {
            self.readdir_root(0, None);

            if let Some(inode) = self.lookup_root_child(parent, name, reply) {
                self.readdir_helper(inode, 0, None);
            }
        } else if parent == self.entries.get_or_create_query_directory() {

            let inode = self.entries.get_or_create_query_result_dir(
                name, name);
            let query = self.entries.get_query(inode);

            info!("Running database query \"{query}\".");
            if self.db.query(query, false).is_err() {
                reply.error(libc::ENOENT);

            } else {
                let attr = *self.entries.get_attr(inode);

                self.readdir_helper(inode, 0, None);
                reply.entry(&TTL, &attr, 0);
            }

        } else if let Some(inode) = self.entries.try_get_inode(parent, name) {
            let attr = *self.entries.get_attr(inode);

            self.readdir_helper(inode, 0, None);

            reply.entry(&TTL, &attr, 0);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    // returns the entries in a directory
    fn readdir(&mut self, _req: &Request, inode: u64, _fh: u64, offset: i64,
               reply: ReplyDirectory)
    {
        info!("readdir(inode: {inode:#x?}, offset: {offset:?})");

        if inode == FUSE_ROOT_ID {
            self.readdir_root(offset, Some(reply));
        } else {
            self.readdir_helper(inode, offset, Some(reply));
        }
    }

    // returns the target for a given link inode.
    fn readlink(&mut self, _req: &Request, inode: u64, reply: ReplyData) {
        info!("readlink(inode: {inode:#x?})");
        if let Some(tag_mapping_id) = self.entries.get_link_target(inode) {
            if let Ok(target) = self.db.get_path_from_id(tag_mapping_id) {
                reply.data(target.as_bytes());
                return;
            }
        }
        error!("could not find link target for inode: {inode:#x?}.");
        panic!("could not find link target for inode: {inode:#x?}.");
    }

    fn open(&mut self, _req: &Request, inode: u64, _flags: i32,
            reply: ReplyOpen)
    {
        info!("open(inode: {inode:#x?})");

        if !matches!(self.entries.get_type(inode), EntryType::AllTagsTerminal)
        {
            error!("tried to open a file that is not a file! inode: \
                    {inode:#x?}.");
            panic!("tried to open a file that is not a file! inode: \
                    {inode:#x?}.");
        }

        reply.opened(0, 0);
    }

    fn read(&mut self, _req: &Request, inode: u64, _fh: u64, offset: i64,
            size: u32, _flags: i32, _lock_owner: Option<u64>, reply: ReplyData)
    {
        info!("read(inode: {inode:#x?}, offset: {offset:?}, size: {size:?})");

        if !matches!(self.entries.get_type(inode), EntryType::AllTagsTerminal)
        {
            error!("tried to read a file that is not a file! inode: \
                    {inode:#x?}.");
            panic!("tried to read a file that is not a file! inode: \
                    {inode:#x?}.");
        }

        let path = self.entries.get_path(inode);

        let mut buf = String::with_capacity(1024);

        if let Ok(tags) = self.db.tags(path) {
            for tag in tags {
                // unwrap is okay here, because we are writing to an in-memory
                // string buffer.
                writeln!(buf, "{}", crate::db::SimpleTagFormatter::from(&tag))
                    .unwrap();
            }
        }

        if (offset as usize) < buf.len() {
            reply.data(&buf.as_bytes()[offset as usize..]);
        }
    }
}

/// Call this function with a path to mount the filesystem.
///
/// Blocks until the filesystem is unmounted.
///
/// # Errors
/// Returns an error if one is thrown by FUSE.
pub fn mount(mnt_point: &str, db: Database) -> std::io::Result<()> {
    info!("Mounting filesystem at \"{mnt_point}\"");

    // force initialisation of the lazy cell to remember the mount time.
    Lazy::force(&MOUNT_TIME);

    let mnt_options = {
        use fuser::MountOption::*;
        &[AutoUnmount, AllowOther, RO]
    };

    let tagfs = TagFS::new(db);

    fuser::mount2(tagfs, mnt_point, mnt_options)
}

/// Converts a full path (such as "my/long/path") to its final component.
fn sanitise_path<T: AsRef<str>>(path: &str, path_idx: usize,
                                siblings: impl Iterator<Item=T>) -> String
{

    fn basename(path: &str) -> &str {
        let path = camino::Utf8Path::new(path);
        let Some(file_name) = path.file_name() else {
            panic!("invalid path \"{}\" without final component.",
                   path);
        };

        file_name
    }

    let path_basename = basename(path);

    let any_path_same_name = siblings.enumerate()
        .any(|(idx, sibling)|
            idx != path_idx && basename(sibling.as_ref()) == path_basename);

    if any_path_same_name {
        format!("{path_basename}.{path_idx}")
    } else {
        String::from(path_basename)
    }
}

/// Convert a value from the database into a value that is suitable for use as
/// a component of a path in the filesystem.
fn sanitise_value(path: &str) -> String {
    path.replace('/', "_")
}

mod tests {
    #[test]
    fn sanitise_path_test() {
        let path1 = "/my/long/file/path.txt";
        let path2 = "/my/other/long/file/path.txt";
        let path3 = "/some/other/file/path.txt";
        let siblings = vec![path2, "/some/other/file", path3];

        assert_eq!(
            super::sanitise_path(path1, 0, siblings.iter()),
            String::from("path.txt.0")
        );

        let siblings = vec![path1, "/some/other/file", path3];
        assert_eq!(
            super::sanitise_path(path2, 1, siblings.iter()),
            String::from("path.txt.1")
        );

        let path = "/a/very/cool/path.txt";
        let siblings = vec![
            path, "/some/unrelated/other/path.rs", "/another/random/path.jpg"
        ];
        assert_eq!(
            super::sanitise_path(path, 0, siblings.iter()),
            String::from("path.txt")
        );
    }
}
