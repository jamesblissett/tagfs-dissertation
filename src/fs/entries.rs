use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::time::UNIX_EPOCH;

use fuser::{FileAttr, FileType, FUSE_ROOT_ID};
use log::error;

use crate::INodeGenerator;

#[derive(Debug)]
pub struct Entries {
    inode_generator: INodeGenerator,

    // inode -> (name, attr)
    attrs: HashMap<u64, (OsString, FileAttr)>,

    targets: HashMap<u64, OsString>,

    // NOTE: we are duplicating data between attrs and names (OsString).
    // parent_inode -> (name -> inode)
    names: HashMap<u64, HashMap<OsString, u64>>,
}

impl Entries {
    pub fn new() -> Self {
        let mut attrs = HashMap::new();
        attrs.insert(FUSE_ROOT_ID, (
            OsString::from("/"),
            FileAttr {
                ino: FUSE_ROOT_ID,
                size: 0,
                blocks: 0,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
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

    pub fn get_or_create_inode_directory(&mut self, parent_inode: u64,
                                         name: &OsStr) -> u64
    {
        self.get_or_create_inode(parent_inode, name, FileType::Directory)
    }

    pub fn get_or_create_inode_link(&mut self, parent_inode: u64,
                                    name: &OsStr) -> u64
    {
        self.get_or_create_inode(parent_inode, name, FileType::Symlink)
    }

    // will create a new inode if one cannot be found.
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
                    atime: UNIX_EPOCH,
                    mtime: UNIX_EPOCH,
                    ctime: UNIX_EPOCH,
                    crtime: UNIX_EPOCH,
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

    pub fn try_get_inode(&mut self, parent_inode: u64, name: &OsStr)
        -> Option<u64>
    {
        let children = self.names.entry(parent_inode).or_default();
        children.get(name).copied()
    }

    pub fn set_link_target(&mut self, inode: u64, target: &OsStr) {
        self.targets.insert(inode, target.to_os_string());
    }

    pub fn get_link_target(&mut self, inode: u64) -> Option<&OsString> {
        self.targets.get(&inode)
    }

    pub fn get_attr(&mut self, inode: u64) -> &FileAttr {
        if let Some((_name, attr)) = self.attrs.get(&inode) {
            attr
        } else {
            error!("tried to lookup non existent inode: {:#x?}.", inode);
            panic!("tried to lookup non existent inode: {:#x?}.", inode);
        }
    }

    pub fn get_name(&mut self, inode: u64) -> &OsStr {
        if let Some((name, _attr)) = self.attrs.get(&inode) {
            name
        } else {
            error!("tried to lookup non existent inode: {:#x?}.", inode);
            panic!("tried to lookup non existent inode: {:#x?}.", inode);
        }
    }
}
