use std::ffi::CStr;
use std::path::Path;

use anyhow::{anyhow, Error, Result};
use itertools::unfold;
use libc::{c_char, c_int, c_void, size_t};

use super::entry::ArchiveEntry;

const ARCHIVE_EOF: c_int = 1;
const ARCHIVE_OK: c_int = 0;

#[link(name = "archive")]
extern "C" {
    fn archive_read_new() -> *const c_void;
    fn archive_read_close(archive: *const c_void);
    fn archive_read_free(archive: *const c_void);
    fn archive_read_support_filter_gzip(archive: *const c_void);
    fn archive_read_support_format_tar(archive: *const c_void);
    fn archive_read_open_memory(
        archive: *const c_void,
        buffer: *const c_void,
        size: size_t,
    ) -> c_int;
    fn archive_read_next_header(
        archive: *const c_void,
        entry: *const c_void,
    ) -> c_int;
    fn archive_read_data_block(
        archive: *const c_void,
        buff: *mut *const c_void,
        size: *mut size_t,
        offset: *mut i64,
    ) -> c_int;

    fn archive_write_disk_new() -> *const c_void;
    fn archive_write_disk_set_standard_lookup(archive: *const c_void)
        -> c_int;
    fn archive_write_close(archive: *const c_void);
    fn archive_write_free(archive: *const c_void);
    fn archive_write_header(
        archive: *const c_void,
        entry: *const c_void,
    ) -> c_int;
    fn archive_write_data_block(
        archive: *const c_void,
        buff: *const c_void,
        size: size_t,
        offest: i64,
    ) -> c_int;
    fn archive_error_string(archive: *const c_void) -> *const c_char;
}

pub struct ArchiveResource {
    reader: *const c_void,
    writer: *const c_void,
}

impl ArchiveResource {
    #[fehler::throws]
    pub fn new(content: &[u8]) -> Self {
        Self {
            reader: Self::init_reader(content)?,
            writer: Self::init_writer()?,
        }
    }

    #[fehler::throws]
    pub fn map_entries<T, F>(self, mut f: F) -> impl Iterator<Item = Result<T>>
    where
        F: FnMut(&mut ArchiveEntry, &ArchiveResource) -> T,
    {
        unfold(ArchiveEntry, move |entry| {
            let result = unsafe {
                archive_read_next_header(self.reader, &entry as *const _ as _)
            };

            match result {
                ARCHIVE_OK => Some(Ok(f(entry, &self))),
                ARCHIVE_EOF => None,
                _ => Some(Err(report_error(self.reader))),
            }
        })
    }

    #[fehler::throws]
    pub fn extract(
        self,
        path: impl AsRef<Path>,
        ignore: impl Fn(String) -> bool,
    ) {
        self.map_entries::<Result<()>, _>(|entry, resource| {
            entry.set_pathname(&path)?;

            if !ignore(entry.pathname()) {
                resource.extract_entry(entry)
            } else {
                Ok(())
            }
        })?
        .collect::<Result<Vec<_>>>()?;
    }

    #[fehler::throws]
    fn extract_entry(&self, entry: &mut ArchiveEntry) {
        let mut buff = std::ptr::null();
        let mut size = 0;
        let mut offset = 0;

        if unsafe { archive_write_header(self.writer, entry as *const _ as _) }
            != ARCHIVE_OK
        {
            fehler::throw!(report_error(self.writer));
        };

        loop {
            match self.read_data_block(&mut buff, &mut size, &mut offset) {
                Some(Ok(_)) => self.write_data_block(buff, size, offset)?,
                Some(Err(err)) => fehler::throw!(err),
                None => break,
            }
        }
    }

    fn read_data_block(
        &self,
        buff: *mut *const c_void,
        size: *mut size_t,
        offset: *mut i64,
    ) -> Option<Result<()>> {
        let result = unsafe {
            archive_read_data_block(self.reader, buff, size, offset)
        };

        match result {
            ARCHIVE_OK => Some(Ok(())),
            ARCHIVE_EOF => None,
            _ => Some(Err(report_error(self.reader))),
        }
    }

    fn write_data_block(
        &self,
        buff: *const c_void,
        size: size_t,
        offset: i64,
    ) -> Result<()> {
        let result = unsafe {
            archive_write_data_block(self.writer, buff, size, offset)
        };

        match result {
            ARCHIVE_OK => Ok(()),
            _ => Err(report_error(self.writer)),
        }
    }

    #[fehler::throws]
    fn init_reader(content: &[u8]) -> *const c_void {
        let reader = unsafe { archive_read_new() };

        if reader.is_null() {
            Err(report_error(reader))?;
        }

        if unsafe {
            archive_read_support_filter_gzip(reader);
            archive_read_support_format_tar(reader);
            archive_read_open_memory(
                reader,
                content.as_ptr() as _,
                content.len(),
            )
        } != ARCHIVE_OK
        {
            Err(report_error(reader))?;
        }

        reader
    }

    #[fehler::throws]
    fn init_writer() -> *const c_void {
        let writer = unsafe { archive_write_disk_new() };

        if writer.is_null() {
            fehler::throw!(report_error(writer));
        }

        if unsafe { archive_write_disk_set_standard_lookup(writer) }
            != ARCHIVE_OK
        {
            fehler::throw!(report_error(writer));
        }

        writer
    }
}

impl Drop for ArchiveResource {
    fn drop(&mut self) {
        unsafe {
            archive_read_close(self.reader);
            archive_read_free(self.reader);
            archive_write_close(self.writer);
            archive_write_free(self.writer);
        }
    }
}

fn report_error(archive: *const c_void) -> Error {
    let error_string = unsafe {
        let string = archive_error_string(archive);
        CStr::from_ptr(string)
    };

    anyhow!("Archiver error: {:?}", error_string)
}
