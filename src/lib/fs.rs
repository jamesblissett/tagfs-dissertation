//! Module that contains the fuse file system for TagFS.
//!
//! The general order in which the FUSE functions are called is as follows:
//!
//! getattr(inode: FUSE_ROOT_ID) -> FileAttr \
//! readdir(inode: FUSE_ROOT_ID, offset: 0) -> [(inode, type, name)] \
//!
//! Then for each name returned by readdir \
//! lookup(name: x, parent_inode: FUSE_ROOT_ID) -> FileAttr

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
    fn readdir_root(&mut self, offset: i64, mut reply: ReplyDirectory) {
        let tags = self.db.all_tags().unwrap();
        for (idx, tag) in tags.iter().enumerate().skip(offset as usize) {
            let child_inode = self.entries.get_or_create_tag_directory(
                FUSE_ROOT_ID, tag
            );

            let done = reply.add(child_inode, (idx + 1) as i64,
                FileType::Directory, tag
            );

            if done { break; }
        }

        reply.ok();
    }

    /// Helper function to reply with the entries for a particular tag value
    /// pair.
    fn readdir_files(&mut self, tag: &str, value: Option<&str>, inode: u64,
                     offset: i64, mut reply: ReplyDirectory)
    {
        if let Ok(children) = self.db.paths_with_tag(tag, value) {
            for (idx, child) in children.iter().enumerate().skip(offset as usize) {
                let display_name = sanitise_path(child, children.iter());
                let child_inode = if let Some(child_inode) = self.entries.try_get_inode(inode, display_name.as_ref()) {
                    child_inode
                } else {
                    self.entries.create_link(
                        inode, display_name.as_ref(), child.as_ref()
                    )
                };

                let done = reply.add(child_inode, (idx + 1) as i64,
                    FileType::Symlink, display_name.as_str()
                );

                if done { break; }
            }
        }
        reply.ok();
    }

    /// Helper function to reply with all the values for a particular tag.
    fn readdir_values(&mut self, tag: &str, inode: u64, offset: i64,
                      mut reply: ReplyDirectory)
    {
        if let Ok(children) = self.db.values(tag) {
            for (idx, child) in children.iter().enumerate().skip(offset as usize) {
                let display_name = sanitise_path(child, children.iter());
                let child_inode = if let Some(child_inode) = self.entries.try_get_inode(inode, display_name.as_ref()) {
                    child_inode
                } else {
                    self.entries.get_or_create_value_directory(inode, display_name.as_ref())
                };

                let done = reply.add(child_inode, (idx + 1) as i64,
                    FileType::Directory, display_name.as_str()
                );

                if done { break; }
            }
        }
        reply.ok();
    }

    fn lookup_root(&mut self, parent: u64, name: &str, reply: ReplyEntry) {
        let matches_tag = self.db.all_tags().unwrap().iter().any(|tag| tag == name);
        if matches_tag {
            let inode = self.entries.get_or_create_tag_directory(parent, name);
            reply.entry(&TTL, self.entries.get_attr(inode), 0);
        } else {
            reply.error(libc::ENOENT);
        }
    }
}

impl fuser::Filesystem for TagFS {

    // look up inode and get its attrs.
    fn getattr(&mut self, _req: &Request<'_>, inode: u64, reply: ReplyAttr) {
        info!("getattr(inode: {:#x?})", inode);

        reply.attr(&TTL, self.entries.get_attr(inode));
    }

    // tells the caller if a file with parent and name exists.
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr,
              reply: ReplyEntry)
    {
        info!("lookup(parent_ino: {:#x?}, name: {:?})", parent, name);

        // for now at least, we are only going to allow valid unicode.
        let name = if let Some(name) = name.to_str() {
            name
        } else {
            error!("tried to lookup non-unicode name \"{:?}\"", name);
            panic!("tried to lookup non-unicode name \"{:?}\"", name);
        };

        if parent == FUSE_ROOT_ID {
            self.lookup_root(parent, name, reply);
        } else if let Some(inode) = self.entries.try_get_inode(parent, name) {
            reply.entry(&TTL, self.entries.get_attr(inode), 0);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    // returns the entries in a directory
    fn readdir(&mut self, _req: &Request, inode: u64, _fh: u64, offset: i64,
               reply: ReplyDirectory)
    {
        info!("readdir(inode: {:#x?}, offset: {:?})", inode, offset);

        if inode == FUSE_ROOT_ID {
            self.readdir_root(offset, reply);
        } else {
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
                // cannot readdir something that is not a directory and the
                // root case is already covered.
                EntryType::Root | EntryType::Link =>
                    unreachable!(),
            }
        }
    }

    fn readlink(&mut self, _req: &Request, inode: u64, reply: ReplyData) {
        info!("readlink(inode: {:#x?})", inode);
        if let Some(target) = self.entries.get_link_target(inode) {
            reply.data(target.as_bytes());
        } else {
            error!("could not find link target for inode: {:#x?}.", inode);
            panic!("could not find link target for inode: {:#x?}.", inode);
        }
    }
}

/// Call this function with a path to mount the filesystem.
///
/// Blocks until the filesystem is unmounted.
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
// TODO: this function looks like a good candidate for some tests :)
/// Converts a full path (such as "my/long/path") to its final component.
fn sanitise_path<T: AsRef<str>>(path: &str, _siblings: impl Iterator<Item=T>) -> String {
    let path = std::path::Path::new(path);
    if let Some(file_name) = path.file_name() {
        String::from(file_name.to_string_lossy())
    } else {
        String::from("unknown")
    }
}
