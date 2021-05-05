/// During OCI runtime creation, we need to mount
/// directories, in particular /dev. However, giving the
/// container access to all device nodes is dangerous. The
/// idiomatic tool for this is devfs control facilities,
/// which allows us to dynamically hide & unhide device
/// nodes from the mounted subsystem.
/// This module replicates devfs(8) behavior to hide devfs
/// nodes from the jail.
use std::{
    convert::AsRef, fs::File, io::Error as StdError, mem,
    os::unix::io::AsRawFd, path::Path,
};

use anyhow::{anyhow, Error};
use common_lib::AsSignedBytes;
use libc::{c_char, c_int, gid_t, ioctl, mode_t, uid_t};

const MAGIC: u32 = 0xdb0a087a;
const DRA_BACTS: c_int = 0x1;
const DRB_HIDE: c_int = 0x1;
const DRB_UNHIDE: c_int = 0x2;
const DRC_PATHPTRN: c_int = 0x2;
const DEVFSIO_RAPPLY: u64 = 0x80ec4402;

#[repr(C)]
struct DevfsRule {
    magic: u32,
    id: u32,
    icond: c_int,
    dswflags: c_int,
    pathptrn: [c_char; 200],
    iacts: c_int,
    bacts: c_int,
    uid: uid_t,
    gid: gid_t,
    mode: mode_t,
    incset: u32,
}

pub enum Operation<'a> {
    HideAll,
    Unhide(&'a str),
}

#[fehler::throws]
pub fn apply(path: impl AsRef<Path>, operation: Operation) {
    let file = File::open(path.as_ref())?;
    let mut rule: DevfsRule = unsafe { mem::zeroed() };
    rule.magic = MAGIC;
    rule.iacts = DRA_BACTS;

    match operation {
        Operation::HideAll => {
            rule.bacts = DRB_HIDE;
        }
        Operation::Unhide(node) => {
            rule.bacts = DRB_UNHIDE;
            rule.icond = DRC_PATHPTRN;
            rule.pathptrn[0..node.len()]
                .copy_from_slice(node.as_signed_bytes());
        }
    }

    if unsafe { ioctl(file.as_raw_fd(), DEVFSIO_RAPPLY, &rule) } < 0 {
        fehler::throw!(anyhow!(
            "devfs rule: ioctl(DEVFSIO_RAPPLY) failed: {}",
            StdError::last_os_error()
        ))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::filesystem::mount::{mount, unmount};

    struct MountedDirectory<'a> {
        path: &'a Path,
    }

    impl<'a> MountedDirectory<'a> {
        fn new(path: &'a Path) -> Self {
            mount(&"devfs", &"devfs", &path, std::iter::empty())
                .expect("failed to mount directory");

            Self { path }
        }
    }

    impl<'a> Drop for MountedDirectory<'a> {
        fn drop(&mut self) {
            unmount(&self.path).expect("failed to unmount directory");
        }
    }

    #[test]
    fn test_device_unhide() {
        let tmpdir = tempfile::tempdir().unwrap();

        let _directory = MountedDirectory::new(tmpdir.path());

        assert!(
            tmpdir.path().join("null").exists(),
            "/dev/null must be present"
        );

        apply(tmpdir.path(), Operation::HideAll)
            .expect("Failed to hide all nodes");

        assert!(
            !tmpdir.path().join("null").exists(),
            "hide all hides /dev/null"
        );

        apply(tmpdir.path(), Operation::Unhide("null"))
            .expect("Failed to unhide /dev/null");

        assert!(
            tmpdir.path().join("null").exists(),
            "unhide(null) unhides /dev/null"
        );
    }
}
