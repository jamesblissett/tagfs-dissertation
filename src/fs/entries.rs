use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::time::UNIX_EPOCH;

use fuser::FileAttr;
use log::{error, warn, info, debug, trace};

use crate::INodeGenerator;

type NameID = u64;

pub struct Entries {
    inode_generator: INodeGenerator,
    name_id_generator: INodeGenerator,

    names: HashMap<OsString, NameID>,
    lookup_map: HashMap<(NameID, u64), u64>,
    attrs: HashMap<u64, FileAttr>,
}

impl Entries {
    pub fn new() -> Self {
        Self {
            inode_generator: INodeGenerator::new(),
            name_id_generator: INodeGenerator::new(),
            lookup_map: HashMap::new(),
            names: HashMap::new(),
            attrs: HashMap::new(),
        }
    }

    pub fn get_or_create_inode(&mut self, parent_inode: u64, name: &OsStr)
        -> u64
    {
        let name_id = if let Some(name_id) = self.names.get(name) {
            *name_id
        } else {
            let name_id = self.name_id_generator.next();
            self.names.insert(name.to_os_string(), name_id);
            name_id
        };

        if let Some(inode) = self.lookup_map.get(&(name_id, parent_inode)) {
            *inode
        } else {
            let inode = self.inode_generator.next();
            self.lookup_map.insert((name_id, parent_inode), inode);

            self.attrs.insert(inode, FileAttr {
                ino: inode,
                size: 0,
                blocks: 0,
                atime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                crtime: UNIX_EPOCH,
                kind: fuser::FileType::Directory,
                perm: 0o755,
                nlink: 1,
                uid: 1000,
                gid: 1000,
                rdev: 0,
                flags: 0,
                blksize: 512,
            });
            inode
        }
    }

    pub fn get_attr(&mut self, inode: u64) -> &FileAttr {
        if let Some(attr) = self.attrs.get(&inode) {
            attr
        } else {
            error!("tried to lookup non existent inode: {:#x?}.", inode);
            panic!("tried to lookup non existent inode: {:#x?}.", inode);
        }
    }
}
