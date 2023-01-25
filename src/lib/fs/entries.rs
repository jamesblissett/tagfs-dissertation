//! stores inodes and their relationships.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};

use fuser::{FileAttr, FileType, FUSE_ROOT_ID};
use log::error;

use crate::fs::INodeGenerator;
use crate::fs::MOUNT_TIME;

#[derive(Debug)]
pub struct Entries {
    inode_generator: INodeGenerator,

    /// inode -> (name, attr)
    attrs: HashMap<u64, (OsString, FileAttr)>,

    /// inode -> link_target
    targets: HashMap<u64, OsString>,

    /// NOTE: we are duplicating data between attrs and names (OsString). \
    /// parent_inode -> (name -> inode)
    names: HashMap<u64, HashMap<OsString, u64>>,
}

impl Entries {

    /// Create a new entries struct and initialise it with the root inode.
    pub fn new() -> Self {
        let mut attrs = HashMap::new();
        attrs.insert(FUSE_ROOT_ID, (
            OsString::from("/"),
            FileAttr {
                ino: FUSE_ROOT_ID,
                size: 0,
                blocks: 0,
                atime: *MOUNT_TIME,
                mtime: *MOUNT_TIME,
                ctime: *MOUNT_TIME,
                crtime: *MOUNT_TIME,
                kind: FileType::Directory,
                perm: 0o755,
                nlink: 1,
                uid: 1000,
                gid: 1000,
                rdev: 0,
                flags: 0,
                blksize: 512,
            }));

        Self {
            inode_generator: INodeGenerator::new(),
            names: HashMap::new(),
            targets: HashMap::new(),
            attrs,
        }
    }

    /// Helper function to get or create a directory inode.
    pub fn get_or_create_inode_directory(&mut self, parent_inode: u64,
                                         name: &OsStr) -> u64
    {
        self.get_or_create_inode(parent_inode, name, FileType::Directory)
    }

    /// Returns the inode of a parent name pair, or creates it if it does not
    /// exist.
    fn get_or_create_inode(&mut self, parent_inode: u64, name: &OsStr,
                           kind: FileType) -> u64
    {
        let children = self.names.entry(parent_inode).or_default();
        if let Some(inode) = children.get(name) {
            *inode
        } else {
            let inode = self.inode_generator.next();
            children.insert(name.to_os_string(), inode);

            self.attrs.insert(inode, (
                name.to_os_string(),
                FileAttr {
                    ino: inode,
                    size: 0,
                    blocks: 0,
                    atime: *MOUNT_TIME,
                    mtime: *MOUNT_TIME,
                    ctime: *MOUNT_TIME,
                    crtime: *MOUNT_TIME,
                    kind,
                    perm: 0o755,
                    nlink: 1,
                    uid: 1000,
                    gid: 1000,
                    rdev: 0,
                    flags: 0,
                    blksize: 512,
            }));

            inode
        }
    }

    /// Create and store attributes for a symlink.
    pub fn create_link(&mut self, parent_inode: u64, name: &OsStr,
                       target: &OsStr) -> u64
    {
        let children = self.names.entry(parent_inode).or_default();
        let inode = self.inode_generator.next();

        children.insert(name.to_os_string(), inode);
        self.targets.insert(inode, target.to_os_string());

        self.attrs.insert(inode, (
            name.to_os_string(),
            FileAttr {
                ino: inode,
                size: target.len() as u64,
                blocks: 0,
                atime: *MOUNT_TIME,
                mtime: *MOUNT_TIME,
                ctime: *MOUNT_TIME,
                crtime: *MOUNT_TIME,
                kind: FileType::Symlink,
                perm: 0o755,
                nlink: 1,
                uid: 1000,
                gid: 1000,
                rdev: 0,
                flags: 0,
                blksize: 512,
        }));

        inode
    }

    /// Attempt to return the inode of the requested entry, if it cannot be
    /// found return None.
    pub fn try_get_inode(&mut self, parent_inode: u64, name: &OsStr)
        -> Option<u64>
    {
        let children = self.names.entry(parent_inode).or_default();
        children.get(name).copied()
    }

    pub fn get_link_target(&mut self, inode: u64) -> Option<&OsString> {
        self.targets.get(&inode)
    }

    /// Get the attributes for an inode.
    ///
    /// To call this function with an inode that does not exist is a programming
    /// error, therefore we panic if it does not exist.
    pub fn get_attr(&mut self, inode: u64) -> &FileAttr {
        if let Some((_name, attr)) = self.attrs.get(&inode) {
            attr
        } else {
            error!("tried to lookup non existent inode: {:#x?}.", inode);
            panic!("tried to lookup non existent inode: {:#x?}.", inode);
        }
    }

    /// Get the name of an inode.
    ///
    /// To call this function with an inode that does not exist is a programming
    /// error, therefore we panic if it does not exist.
    pub fn get_name(&mut self, inode: u64) -> &OsStr {
        if let Some((name, _attr)) = self.attrs.get(&inode) {
            name
        } else {
            error!("tried to lookup non existent inode: {:#x?}.", inode);
            panic!("tried to lookup non existent inode: {:#x?}.", inode);
        }
    }
}
