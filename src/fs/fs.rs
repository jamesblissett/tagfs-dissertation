use std::collections::HashMap;
use std::ffi::OsStr;

use fuser::{
    FileType, FUSE_ROOT_ID, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request
};
use log::{error, info};

use crate::Entries;

static TTL: std::time::Duration = std::time::Duration::from_secs(1);

struct TagFS {
    tags: HashMap<&'static str, Vec<&'static str>>,
    entries: Entries,
}

impl TagFS {
    pub fn new() -> Self {
        Self {
            tags: HashMap::from_iter([
                ("film-noir", vec!["/home/james/.bashrc", "/bin/bash"]),
                ("western", vec!["/home/james/.bashrc"]),
                ("comedy", vec!["/media/hdd/film/films.rec"]),
            ]),
            entries: Entries::new(),
        }
    }
}

impl fuser::Filesystem for TagFS {
    fn getattr(&mut self, _req: &Request<'_>, inode: u64, reply: ReplyAttr) {
        info!("getattr(inode: {:#x?})", inode);

        reply.attr(&TTL, self.entries.get_attr(inode));
    }

    // tells the caller if a file with parent and name exists.
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr,
              reply: ReplyEntry)
    {
        info!("lookup(parent_ino: {:#x?}, name: {:?})", parent, name);

        if parent == FUSE_ROOT_ID {
            let matches_tag = self.tags.iter().any(|(tag, _)| *tag == name);
            if matches_tag {
                let inode = self.entries.get_or_create_inode_directory(parent, name);
                reply.entry(&TTL, self.entries.get_attr(inode), 0);
            } else {
                reply.error(libc::ENOENT);
            }
        } else {
            if let Some(inode) = self.entries.try_get_inode(parent, name) {
                reply.entry(&TTL, self.entries.get_attr(inode), 0);
            } else {
                reply.error(libc::ENOENT);
            }
        }
    }

    // returns the entries in a directory
    fn readdir(&mut self, _req: &Request, inode: u64, _fh: u64, offset: i64,
               mut reply: ReplyDirectory)
    {
        info!("readdir(inode: {:#x?}, offset: {:?})", inode, offset);

        if inode == FUSE_ROOT_ID {
            for (idx, tag) in self.tags.keys().enumerate().skip(offset as usize) {
                let child_inode = self.entries.get_or_create_inode_directory(
                    FUSE_ROOT_ID, OsStr::new(tag)
                );

                let done = reply.add(child_inode, (idx + 1) as i64,
                    FileType::Directory, tag
                );

                if done { break; }
            }
        } else {
            let name = self.entries.get_name(inode);
            if let Some(name) = name.to_str() {
                if let Some(children) = self.tags.get(name) {
                    for (idx, child) in children.iter().enumerate().skip(offset as usize) {
                        let display_name = sanitise_path(child, children.as_slice());
                        let child_inode = self.entries.get_or_create_inode_link(
                            inode, display_name.as_ref()
                        );

                        self.entries.set_link_target(child_inode, child.as_ref());

                        let done = reply.add(child_inode, (idx + 1) as i64,
                            FileType::Symlink, display_name.as_str()
                        );

                        if done { break; }
                    }
                }
            }
        }
        reply.ok();
    }

    fn readlink(&mut self, _req: &Request, inode: u64, reply: ReplyData) {
        info!("readlink(inode: {:#x?})", inode);
        if let Some(target) = self.entries.get_link_target(inode) {
            if let Some(target) = target.to_str() {
                reply.data(target.as_bytes());
            }
        } else {
            error!("could not find link target for inode: {:#x?}.", inode);
            panic!("could not find link target for inode: {:#x?}.", inode);
        }
    }
}

pub fn mount(mnt_point: &str) {
    info!("Mounting filesystem at \"{mnt_point}\"");

    let mnt_options = {
        use fuser::MountOption::*;
        &[AutoUnmount, AllowOther, RO]
    };

    let tagfs = TagFS::new();

    if let Err(e) = fuser::mount2(tagfs, mnt_point, mnt_options) {
        eprintln!("{:?}", e);
    }
}

// TODO: update this function to take into account the fact that we cannot have
// files with the same name in the same directory.
// TODO: this function looks like a good candidate for some tests :)
fn sanitise_path(path: &str, _siblings: &[&str]) -> String {
    let path = std::path::Path::new(path);
    if let Some(file_name) = path.file_name() {
        String::from(file_name.to_string_lossy())
    } else {
        String::from("unknown")
    }
}
