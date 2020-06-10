use std::ffi::{CStr, CString};
use std::path::Path;

use anyhow::{anyhow, Error};
use libc::{c_char, c_void};

#[link(name = "archive")]
extern "C" {
    fn archive_entry_pathname(entry: *const c_void) -> *const c_char;
    fn archive_entry_set_pathname(
        entry: *const c_void,
        pathname: *const c_char,
    );
}

pub struct ArchiveEntry;

impl ArchiveEntry {
    pub fn pathname(&self) -> String {
        unsafe {
            let string = archive_entry_pathname(self as *const _ as _);

            CStr::from_ptr(string).to_string_lossy().into_owned()
        }
    }

    #[fehler::throws]
    pub fn set_pathname(&self, path: impl AsRef<Path>) {
        let pathname = path
            .as_ref()
            .join(self.pathname())
            .into_os_string()
            .into_string()
            .map_err(|err| anyhow!("Couldn't convert {:?} to string", err))?;

        unsafe {
            let pathname_raw = CString::from_vec_unchecked(pathname.into());

            archive_entry_set_pathname(
                self as *const _ as _,
                pathname_raw.into_raw(),
            );
        }
    }
}
