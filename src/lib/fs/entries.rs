//! stores inodes and their relationships.

use std::collections::HashMap;

use fuser::{FileAttr, FileType, FUSE_ROOT_ID};
use log::error;

use crate::fs::INodeGenerator;
use crate::fs::MOUNT_TIME;

#[derive(Debug)]
enum Entry {
    Root {
        attr: FileAttr,
    },
    TagDir {
        name: String, attr: FileAttr,
    },
    ValueDir {
        name: String, tag: String, attr: FileAttr,
    },
    Link {
        name: String, target: String, attr: FileAttr,
    }
}

/// public type enum to avoid exposing the entry enum.
#[derive(Debug)]
pub enum EntryType {
    Root,
    TagDir,
    Link,
    ValueDir,
}

#[derive(Debug)]
pub struct Entries {
    inode_generator: INodeGenerator,

    /// inode -> Entry
    attrs: HashMap<u64, Entry>,

    /// NOTE: we are duplicating data between attrs and names (String). \
    /// parent_inode -> (name -> inode)
    names: HashMap<u64, HashMap<String, u64>>,
}

impl Entries {

    /// Create a new entries struct and initialise it with the root inode.
    pub fn new() -> Self {
        let mut attrs = HashMap::new();
        attrs.insert(FUSE_ROOT_ID, Entry::Root {
            attr: FileAttr {
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
            }});

        Self {
            inode_generator: INodeGenerator::new(),
            names: HashMap::new(),
            attrs,
        }
    }

    /// Returns the inode of a parent name pair, or creates it if it does not
    /// exist.
    pub fn get_or_create_tag_directory(&mut self, parent_inode: u64,
                                       name: &str) -> u64
    {
        let children = self.names.entry(parent_inode).or_default();
        if let Some(inode) = children.get(name) {
            *inode
        } else {
            let inode = self.inode_generator.next();
            children.insert(name.to_string(), inode);

            self.attrs.insert(inode, Entry::TagDir {
                name: name.to_string(),
                attr: FileAttr {
                    ino: inode,
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
            }});

            inode
        }
    }

    /// Returns the inode of a parent name pair, or creates it if it does not
    /// exist.
    pub fn get_or_create_value_directory(&mut self, parent_inode: u64,
                                         name: &str) -> u64
    {
        let children = self.names.entry(parent_inode).or_default();
        if let Some(inode) = children.get(name) {
            *inode
        } else {
            let inode = self.inode_generator.next();
            children.insert(name.to_string(), inode);

            self.attrs.insert(inode, Entry::ValueDir {
                name: name.to_string(),
                tag: self.get_name(parent_inode).to_string(),
                attr: FileAttr {
                    ino: inode,
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
            }});

            inode
        }
    }

    /// Create and store attributes for a tag symlink.
    pub fn create_link(&mut self, parent_inode: u64, name: &str,
                       target: &str) -> u64
    {
        let children = self.names.entry(parent_inode).or_default();
        let inode = self.inode_generator.next();

        children.insert(name.to_string(), inode);

        self.attrs.insert(inode, Entry::Link {
            name: name.to_string(),
            target: target.to_string(),
            attr: FileAttr {
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
        }});

        inode
    }

    /// Attempt to return the inode of the requested entry, if it cannot be
    /// found return None.
    pub fn try_get_inode(&self, parent_inode: u64, name: &str)
        -> Option<u64>
    {
        self.names.get(&parent_inode).map(|children|
            children.get(name).copied()
        ).flatten()
    }

    /// Get the parent tag associated with a value by inode.
    ///
    /// This is only valid when called with an Entry::ValueDir inode.
    pub fn get_parent_tag(&self, inode: u64) -> &str {
        if let Some(entry) = self.attrs.get(&inode) {
            if let Entry::ValueDir { tag, .. } = entry {
                tag
            } else {
                error!("tried to lookup parent tag of non ValueDir entry: {:#x?}.", inode);
                panic!("tried to lookup parent tag of non ValueDir entry: {:#x?}.", inode);
            }
        } else {
            error!("tried to lookup non existent inode: {:#x?}.", inode);
            panic!("tried to lookup non existent inode: {:#x?}.", inode);
        }
    }

    /// Get the target of a link by inode.
    pub fn get_link_target(&self, inode: u64) -> Option<&str> {
        self.attrs.get(&inode).map(|entry| {
            match entry {
                Entry::Root { .. } => panic!("programming error - root directory is not a link and has no target."),
                Entry::TagDir { .. } => panic!("programming error - directory is not a link and has no target."),
                Entry::ValueDir { .. } => panic!("programming error - directory is not a link and has no target."),
                Entry::Link { target, .. } => target.as_str(),
            }
        })
    }

    /// Get the attributes for an inode.
    ///
    /// To call this function with an inode that does not exist is a programming
    /// error, therefore we panic if it does not exist.
    pub fn get_attr(&self, inode: u64) -> &FileAttr {
        if let Some(entry) = self.attrs.get(&inode) {
            match entry {
                Entry::Root { attr } => attr,
                Entry::TagDir { attr, .. } => attr,
                Entry::ValueDir { attr, .. } => attr,
                Entry::Link { attr, .. } => attr,
            }
        } else {
            error!("tried to lookup non existent inode: {:#x?}.", inode);
            panic!("tried to lookup non existent inode: {:#x?}.", inode);
        }
    }

    /// Get the name of an inode.
    ///
    /// To call this function with an inode that does not exist is a programming
    /// error, therefore we panic if it does not exist.
    pub fn get_name(&self, inode: u64) -> &str {
        if let Some(entry) = self.attrs.get(&inode) {
            match entry {
                Entry::Root { .. } => "/",
                Entry::TagDir { name, .. } => name,
                Entry::ValueDir { name, .. } => name,
                Entry::Link { name, .. } => name,
            }
        } else {
            error!("tried to lookup non existent inode: {:#x?}.", inode);
            panic!("tried to lookup non existent inode: {:#x?}.", inode);
        }
    }

    /// Get the type of an inode.
    ///
    /// To call this function with an inode that does not exist is a programming
    /// error, therefore we panic if it does not exist.
    pub fn get_type(&self, inode: u64) -> EntryType {
        if let Some(entry) = self.attrs.get(&inode) {
            match entry {
                Entry::Root { .. } => EntryType::Root,
                Entry::TagDir { .. } => EntryType::TagDir,
                Entry::ValueDir { .. } => EntryType::ValueDir,
                Entry::Link { .. } => EntryType::Link,
            }
        } else {
            error!("tried to lookup non existent inode: {:#x?}.", inode);
            panic!("tried to lookup non existent inode: {:#x?}.", inode);
        }
    }
}
