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

use std::iter::Iterator;
use std::ffi::OsStr;

use fuser::{
    FileType, FUSE_ROOT_ID, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request
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
    fn readdir_root(&mut self, offset: i64, mut reply: Option<ReplyDirectory>) {

        // the query directory is the first child of the root, so we only need
        // to return it when offset = 0, otherwise it has already been
        // returned.
        if offset == 0 {
            let query_dir_inode = self.entries.get_or_create_query_directory();
            let done = reply.as_mut().map_or(false, |reply|
                reply.add(query_dir_inode, 1, FileType::Directory,
                    self.entries.get_name(query_dir_inode)));

            if done {
                if let Some(reply) = reply { reply.ok() }
                return;
            }
        }

        let tags = self.db.all_tags().unwrap();
        for (idx, tag) in tags.iter().enumerate().skip(offset as usize) {
            let child_inode = self.entries.get_or_create_tag_directory(
                FUSE_ROOT_ID, tag
            );

            // we add two to the index to account for the query directory.
            let done = reply.as_mut().map_or(false, |reply|
                reply.add(child_inode, (idx + 2) as i64,
                    FileType::Directory, tag));

            if done { break; }
        }
        if let Some(reply) = reply { reply.ok() }
    }

    /// Helper function to reply with the entries for a particular tag value
    /// pair.
    fn readdir_files(&mut self, tag: &str, value: Option<&str>, inode: u64,
                     offset: i64, mut reply: Option<ReplyDirectory>)
    {
        if let Ok(children) = self.db.paths_with_tag(tag, value) {
            for (idx, (child, child_id)) in children.iter().enumerate().skip(offset as usize) {
                let display_name = sanitise_path(child, children.iter().map(|(child, _)| child));
                let child_inode = if let Some(child_inode) = self.entries.try_get_inode(inode, display_name.as_ref()) {
                    child_inode
                } else {
                    self.entries.create_link(inode, display_name.as_ref(),
                        *child_id, child.len() as u64)
                };

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
            for (idx, child) in children.iter().enumerate().skip(offset as usize) {
                let display_name = sanitise_path(child, children.iter());
                let child_inode = if let Some(child_inode) = self.entries.try_get_inode(inode, display_name.as_ref()) {
                    child_inode
                } else {
                    self.entries.get_or_create_value_directory(inode,
                        display_name.as_ref())
                };

                let done = reply.as_mut().map_or(false, |reply|
                    reply.add(child_inode, (idx + 1) as i64,
                        FileType::Directory, display_name.as_str()));

                if done { break; }
            }
        }
        if let Some(reply) = reply { reply.ok() }
    }

    /// Helper function to look up the root inode and create its children if
    /// necessary.
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
            if name == self.entries.get_name(query_dir_inode) {
                reply.entry(&TTL, self.entries.get_attr(query_dir_inode), 0);
                Some(query_dir_inode)
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
        let query = self.entries.get_name(inode);
        // the else case _should_ never happen because we have
        // already rejected any invalid queries.
        if let Ok(paths) = self.db.query(query, false) {
            for (idx, (child, child_id)) in paths.iter().enumerate().skip(offset as usize) {
                let display_name = sanitise_path(child, paths.iter().map(|(child, _)| child));
                let child_inode = if let Some(child_inode) = self.entries.try_get_inode(inode, display_name.as_ref()) {
                    child_inode
                } else {
                    self.entries.create_link(
                        inode, display_name.as_ref(), *child_id, child.len() as u64
                    )
                };

                let done = reply.as_mut().map_or(false, |reply|
                    reply.add(child_inode, (idx + 1) as i64,
                        FileType::Symlink, display_name.as_str()));

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
    fn readdir_helper(&mut self, inode: u64, offset: i64, reply: Option<ReplyDirectory>) {
        let attr = self.entries.get_attr(inode);

        if attr.kind == FileType::Directory {
            let name = self.entries.get_name(inode);

            match self.entries.get_type(inode) {
                EntryType::TagDir => {
                    if let Some(tag) = self.db.get_tag(name) {
                        if tag.takes_value {
                            self.readdir_values(&tag.name, inode, offset, reply);
                        } else {
                            self.readdir_files(&tag.name, None, inode, offset, reply);
                        }
                    } else {
                        panic!("programming error: tried to readdir for directory that is not a tag.");
                    }
                }
                EntryType::ValueDir => {
                    let tag_name = self.entries.get_parent_tag(inode).to_string();
                    self.readdir_files(&tag_name, Some(&name.to_string()), inode, offset, reply);
                }

                // query dir should always look empty to readdir.
                EntryType::QueryDir => {
                    if let Some(reply) = reply { reply.ok() }
                }

                EntryType::QueryResultDir => {
                    self.readdir_query(inode, offset, reply);
                }

                // cannot readdir something that is not a directory and the
                // root is already covered.
                EntryType::Root | EntryType::Link =>
                    unreachable!(),
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
        let name = name.to_str().map_or_else(|| {
                error!("tried to lookup non-unicode name \"{name:?}\"");
                panic!("tried to lookup non-unicode name \"{name:?}\"");
            }, |name| name);

        // We call the readdir helper functions to ensure that we create the
        // child inodes as soon as possible. This ensures that even if readdir
        // is not called, we can operate with just lookup.
        if parent == FUSE_ROOT_ID {
            self.readdir_root(0, None);

            if let Some(inode) = self.lookup_root_child(parent, name, reply) {
                self.readdir_helper(inode, 0, None);
            }
        } else if parent == self.entries.get_or_create_query_directory() {
            info!("Running database query \"{name}\".");

            if self.db.query(name, false).is_err() {
                reply.error(libc::ENOENT);
            } else {
                let inode = self.entries.get_or_create_query_result_dir(name);
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

// TODO: update this function to take into account the fact that we cannot have
// files with the same name in the same directory.
// Once this is done this function will be a good candidate for some tests :)
/// Converts a full path (such as "my/long/path") to its final component.
fn sanitise_path<T: AsRef<str>>(path: &str, _siblings: impl Iterator<Item=T>)
    -> String
{
    let path = std::path::Path::new(path);
    if let Some(file_name) = path.file_name() {
        String::from(file_name.to_string_lossy())
    } else {
        String::from("unknown")
    }
}