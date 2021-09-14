use std::path::Path;

use anyhow::Error;
use libknast::filesystem::Mountable;

use super::protocols::mount::Mount;

impl Mountable for Mount {
    fn kind(&self) -> &String {
        &self.field_type
    }

    fn source(&self) -> &String {
        &self.source
    }

    fn destination(&self) -> &String {
        &self.target
    }

    fn options(&self) -> Vec<String> {
        self.options.as_ref().to_vec()
    }

    fn post_mount_hooks(
        &self,
        _rootfs: impl AsRef<Path>,
    ) -> Result<(), Error> {
        Ok(())
    }
}
