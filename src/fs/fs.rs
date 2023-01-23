use std::ffi::{OsStr, OsString};
use std::time::UNIX_EPOCH;

use fuser::{FileAttr, FileType, FUSE_ROOT_ID, ReplyAttr, ReplyDirectory, ReplyEntry, Request};
use log::{error, warn, info, debug, trace};

use crate::Entries;

static TTL: std::time::Duration = std::time::Duration::from_secs(1);

struct TagFS {
    tags: Vec<String>,
    entries: Entries,
}

impl TagFS {
    pub fn new() -> Self {
        Self {
            tags: vec!["film-noir".into(), "western".into(), "comedy".into()],
            entries: Entries::new(),
        }
    }
}

impl fuser::Filesystem for TagFS {
    fn getattr(&mut self, _req: &Request<'_>, inode: u64, reply: ReplyAttr) {
        info!("getattr(inode: {:#x?})", inode);

        if inode == FUSE_ROOT_ID {
            reply.attr(&TTL, &FileAttr {
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
        } else {
            reply.attr(&TTL, self.entries.get_attr(inode));
        }
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr,
              reply: ReplyEntry)
    {
        info!("lookup(parent_ino: {:#x?}, name: {:?})", parent, name);

        if parent == FUSE_ROOT_ID {
            let matches_tag = self.tags.iter().any(|tag| tag.as_str() == name);
            if matches_tag {
                let inode = self.entries.get_or_create_inode(parent, name);
                reply.entry(&TTL, self.entries.get_attr(inode), 0);
            }
        } else {
            // TODO do something else
        }
    }

    fn readdir(&mut self, _req: &Request, inode: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
        info!("readdir(inode: {:#x?}, offset: {:?})", inode, offset);

        if inode == FUSE_ROOT_ID {
            for (idx, tag) in self.tags.iter().enumerate().skip(offset as usize) {
                let child_name = OsString::from(tag);
                let child_inode = self.entries.get_or_create_inode(
                    FUSE_ROOT_ID, &child_name
                );
                let done = reply.add(child_inode, (idx + 1) as i64,
                    FileType::Directory, &child_name
                );

                if done { break; }
            }

            reply.ok();
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
