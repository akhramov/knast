use std::{
    io::Error, os::unix::process::CommandExt as StdCommandExt,
    process::Command,
};

use libc::{setuid, uid_t};

// A workaround for https://github.com/fubarnetes/libjail-rs/issues/103
pub trait CommandExt {
    fn uid(&mut self, uid: u32) -> &mut Command;
    fn gid(&mut self, gid: u32) -> &mut Command;
}

impl CommandExt for Command {
    fn uid(&mut self, uid: u32) -> &mut Command {
        unsafe {
            self.pre_exec(move || {
                if setuid(uid as uid_t) < 0 {
                    return Err(Error::last_os_error());
                }

                Ok(())
            });
        }

        self
    }

    fn gid(&mut self, gid: u32) -> &mut Command {
        StdCommandExt::gid(self, gid)
    }
}
