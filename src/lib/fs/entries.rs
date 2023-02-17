//! stores inodes and their relationships.

use std::collections::HashMap;

use fuser::{FileAttr, FileType, FUSE_ROOT_ID};
use log::error;

use crate::fs::INodeGenerator;
use crate::fs::MOUNT_TIME;

/// Hardcoded name of the query directory.
static QUERY_DIR_NAME: &str = "?";

/// Each inode is one and only one of the types described in [`Entry`].
#[derive(Debug)]
enum Entry {
    /// Root inode - should only ever be one. Path /.
    Root {
        attr: FileAttr,
    },
    /// QueryDirectory inode - should only ever be one. Path: /?.
    QueryDir {
        attr: FileAttr,
    },
    /// Path: /?/query
    QueryResultDir {
        display_name: String, query: String, attr: FileAttr,
    },
    /// Path: /tag
    TagDir {
        name: String, attr: FileAttr,
    },
    /// Path: /tag/value
    ValueDir {
        display_name: String, value: String, tag: String, attr: FileAttr,
    },
    /// Symlink to a real file.
    Link {
        name: String,
        /// References TagMappingID in the database.
        target: u64,
        attr: FileAttr,
    }
}

/// public type enum to avoid exposing the entry enum.
#[derive(Debug)]
pub enum EntryType {
    Root,
    QueryDir,
    QueryResultDir,
    TagDir,
    Link,
    ValueDir,
}

#[derive(Debug)]
/// Tracks the mapping of inodes to entries.
pub struct Entries {
    /// generates a unique inode.
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

    /// Returns the inode of the query directory, or creates it if it does not
    /// exist.
    pub fn get_or_create_query_directory(&mut self) -> u64 {
        let children = self.names.entry(FUSE_ROOT_ID).or_default();
        if let Some(inode) = children.get(QUERY_DIR_NAME) {
            *inode
        } else {
            let inode = self.inode_generator.next();
            children.insert(QUERY_DIR_NAME.to_string(), inode);

            self.attrs.insert(inode, Entry::QueryDir {
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

    /// Returns the inode of a query result directory, or creates it if it does
    /// not exist.
    pub fn get_or_create_query_result_dir(&mut self, query: &str, name: &str)
        -> u64
    {
        let query_dir_inode = self.get_or_create_query_directory();
        let children = self.names.entry(query_dir_inode).or_default();
        if let Some(inode) = children.get(name) {
            *inode
        } else {
            let inode = self.inode_generator.next();
            children.insert(name.to_string(), inode);

            self.attrs.insert(inode, Entry::QueryResultDir {
                display_name: name.to_string(),
                query: query.to_string(),
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
                                         name: &str, value: &str) -> u64
    {
        let children = self.names.entry(parent_inode).or_default();
        if let Some(inode) = children.get(name) {
            *inode
        } else {
            let inode = self.inode_generator.next();
            children.insert(name.to_string(), inode);

            self.attrs.insert(inode, Entry::ValueDir {
                display_name: name.to_string(),
                value: value.to_string(),
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
                       target: u64, target_len: u64) -> u64
    {
        let children = self.names.entry(parent_inode).or_default();
        let inode = self.inode_generator.next();

        children.insert(name.to_string(), inode);

        self.attrs.insert(inode, Entry::Link {
            name: name.to_string(),
            target,
            attr: FileAttr {
                ino: inode,
                size: target_len,
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
        self.names.get(&parent_inode).and_then(|children|
            children.get(name).copied())
    }

    /// Attempt to return the inode of the requested entry, if it cannot be
    /// found return None. Also ensure that is a link entry and it matches the
    /// target tag_mapping_id. This way we will never reuse old entries
    /// with invalid tag_mapping_ids.
    pub fn try_get_link_inode(&self, parent_inode: u64, name: &str,
                         tag_mapping_id: u64) -> Option<u64>
    {
        let inode = self.try_get_inode(parent_inode, name);

        inode
            .and_then(|inode| self.attrs.get(&inode))
            .and_then(|entry| match entry {
                Entry::Link { target, .. } if *target == tag_mapping_id => inode,
                _ => None,
            })
    }

    /// Get the parent tag associated with a value by inode.
    ///
    /// This is only valid when called with an [`Entry::ValueDir`] inode.
    pub fn get_parent_tag(&self, inode: u64) -> &str {
        if let Some(entry) = self.attrs.get(&inode) {
            if let Entry::ValueDir { tag, .. } = entry {
                return tag;
            }
        }
        error!("tried to lookup parent tag of non ValueDir entry: {inode:#x?}.");
        panic!("tried to lookup parent tag of non ValueDir entry: {inode:#x?}.");
    }

    /// Get the target of a link by inode.
    pub fn get_link_target(&self, inode: u64) -> Option<u64> {
        self.attrs.get(&inode).map(|entry| {
            match entry {
                Entry::Root { .. } | Entry::QueryDir { .. }
                | Entry::QueryResultDir { .. } | Entry::TagDir { .. }
                | Entry::ValueDir { .. } =>
                    panic!("programming error - directory is not a link and has no target."),
                Entry::Link { target, .. } => *target,
            }
        })
    }

    /// Get the query related to a [`Entry::QueryResultDir`].
    pub fn get_query(&self, inode: u64) -> &str {
        if let Some(entry) = self.attrs.get(&inode) {
            if let Entry::QueryResultDir { query, .. } = entry {
                return query;
            }
        }
        error!("tried to lookup query of non QueryResultDir entry: {inode:#x?}.");
        panic!("tried to lookup query of non QueryResultDir entry: {inode:#x?}.");
    }

    /// Get the attributes for an inode.
    ///
    /// To call this function with an inode that does not exist is a programming
    /// error, therefore we panic if it does not exist.
    pub fn get_attr(&self, inode: u64) -> &FileAttr {
        if let Some(entry) = self.attrs.get(&inode) {
            match entry {
                Entry::Root { attr }
                | Entry::QueryDir { attr }
                | Entry::QueryResultDir { attr, .. }
                | Entry::TagDir { attr, .. }
                | Entry::ValueDir { attr, .. }
                | Entry::Link { attr, .. } => attr,
            }
        } else {
            error!("tried to lookup non existent inode: {inode:#x?}.");
            panic!("tried to lookup non existent inode: {inode:#x?}.");
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
                Entry::QueryDir { .. } => QUERY_DIR_NAME,

                Entry::QueryResultDir { display_name: name, .. }
                | Entry::TagDir { name, .. }
                | Entry::ValueDir { display_name: name, .. }
                | Entry::Link { name, .. } => name,
            }
        } else {
            error!("tried to lookup non existent inode: {inode:#x?}.");
            panic!("tried to lookup non existent inode: {inode:#x?}.");
        }
    }

    /// Get the tag value of an inode.
    ///
    /// To call this function with an inode that does not exist is a
    /// programming error, therefore we panic if it does not exist.
    /// Additionally it is a programming error to call this function with an
    /// inode type that is not a [`Entry::ValueDir`] therefore we also panic in
    /// this case.
    pub fn get_tag_value(&self, inode: u64) -> &str {
        if let Some(entry) = self.attrs.get(&inode) {
            if let Entry::ValueDir { value, .. } = entry {
                return value;
            }
        }
        error!("tried to lookup non existent inode: {inode:#x?}.");
        panic!("tried to lookup non existent inode: {inode:#x?}.");
    }

    /// Get the type of an inode.
    ///
    /// To call this function with an inode that does not exist is a programming
    /// error, therefore we panic if it does not exist.
    pub fn get_type(&self, inode: u64) -> EntryType {
        if let Some(entry) = self.attrs.get(&inode) {
            match entry {
                Entry::Root { .. } => EntryType::Root,
                Entry::QueryDir { .. } => EntryType::QueryDir,
                Entry::QueryResultDir { .. } => EntryType::QueryResultDir,
                Entry::TagDir { .. } => EntryType::TagDir,
                Entry::ValueDir { .. } => EntryType::ValueDir,
                Entry::Link { .. } => EntryType::Link,
            }
        } else {
            error!("tried to lookup non existent inode: {inode:#x?}.");
            panic!("tried to lookup non existent inode: {inode:#x?}.");
        }
    }
}
