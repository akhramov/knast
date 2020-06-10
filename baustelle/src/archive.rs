pub mod entry;
pub mod resource;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Error, Result};

use resource::ArchiveResource;

pub struct Archive<'a> {
    content: &'a [u8],
}

impl<'a> Archive<'a> {
    pub fn new(content: &'a [u8]) -> Self {
        Self { content }
    }

    #[fehler::throws]
    pub fn entries(&self) -> impl Iterator<Item = Result<PathBuf>> {
        self.resource()?.map_entries(|entry, _| {
            let os_string: OsString = entry.pathname().into();

            os_string.into()
        })?
    }

    #[fehler::throws]
    pub fn extract(
        &self,
        path: impl AsRef<Path>,
        ignore: impl Fn(String) -> bool,
    ) {
        self.resource()?.extract(path, ignore)?;
    }

    #[fehler::throws]
    fn resource(&self) -> ArchiveResource {
        ArchiveResource::new(&self.content)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_content() {
        let content = test_helpers::bytes_fixture!("foo.tar.gz");

        let archive = Archive::new(content);
        let expected = test_helpers::code_fixture!("foo_archive_entries");

        let actual = archive
            .entries()
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .expect("One or more entry failed to report its pathname");

        assert_eq!(expected, actual);
    }

    #[test]
    fn test_extract() {
        let content = test_helpers::bytes_fixture!("foo.tar.gz");

        let archive = Archive::new(content);
        let dir =
            tempfile::tempdir().expect("failed to create a tmp directory");

        archive
            .extract(dir.path(), |_| false)
            .expect("failed to extract archive");

        let link = std::fs::read_link(dir.path().join("foo/bis"))
            .expect("symlink does not exist");

        assert_eq!("bad/bad", link.to_string_lossy());
    }
}
