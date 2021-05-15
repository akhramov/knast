/// Handles mounting and unmounting of filesystems.
/// Some filesystems require additional actions on mount,
/// i.e. devfs nodes need to be hidden using the rule
/// subsystem, and so on.
mod devfs;
mod mount;

use std::{
    convert::AsRef,
    path::{Component, Path, PathBuf},
};

use anyhow::Error;

use baustelle::runtime_config::Mount;

pub trait Mountable {
    #[fehler::throws]
    fn mount(&self, rootfs: impl AsRef<Path>) {
        let kind = self.kind();
        let source = self.source();
        let destination = prefixed_destination(&rootfs, self.destination());

        tracing::info!(
            "Mounting {} fs {:?} -> {:?}",
            kind,
            source,
            destination
        );
        mount::mount(
            kind,
            source,
            &destination,
            self.options().iter().map(|x| x as &dyn AsRef<str>),
        )?;

        self.post_mount_hooks(rootfs)?;
    }

    #[fehler::throws]
    fn unmount(&self, rootfs: impl AsRef<Path>) {
        mount::unmount(&prefixed_destination(rootfs, self.destination()))?;
    }

    fn post_mount_hooks(&self, rootfs: impl AsRef<Path>) -> Result<(), Error>;

    fn kind(&self) -> &String;
    fn source(&self) -> &String;
    fn destination(&self) -> &String;
    fn options(&self) -> Vec<String>;
}

impl Mountable for Mount {
    fn kind(&self) -> &String {
        &self.r#type
    }

    fn source(&self) -> &String {
        if let Some(source) = &self.source {
            return &source;
        }

        self.kind()
    }

    fn destination(&self) -> &String {
        &self.destination
    }

    fn options(&self) -> Vec<String> {
        self.options.clone().unwrap_or_else(|| vec![])
    }

    #[fehler::throws]
    fn post_mount_hooks(&self, rootfs: impl AsRef<Path>) {
        if self.r#type == "devfs" {
            prepare_devfs(&prefixed_destination(rootfs, self.destination()))?;
        }
    }
}

/// There's no FreeBSD spec yet, so follow Linux config as
/// possible https://git.io/JOQal
#[fehler::throws]
fn prepare_devfs(path: impl AsRef<Path>) {
    use devfs::{apply, Operation};

    const DEFAULT_DEVICES: [&str; 10] = [
        "null", "zero", "full", "random", "urandom", "tty", "console", "pts",
        "pts/*", "fd",
    ];

    apply(&path, Operation::HideAll)?;

    for device in &DEFAULT_DEVICES {
        apply(&path, Operation::Unhide(device))?
    }
}

/// For args, cwd, and mountpoints runtime config specifies
/// paths inside containers Therefore, we need to prefix
/// these paths with the rootfs of the container.
pub fn prefixed_destination(
    rootfs: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> PathBuf {
    let mut result = rootfs.as_ref().to_owned();

    for component in destination.as_ref().components() {
        // Sanitization: we don't want "..", "." or "/" here
        if let Component::Normal(component) = component {
            result.push(component);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    #[test]
    fn test_mounting_nullfs() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();

        let rootfs = destination.path();
        let src = source.path().to_str().unwrap().into();

        let mount = Mount {
            destination: "/".into(),
            source: Some(src),
            options: None,
            r#type: "nullfs".into(),
        };

        mount.mount(rootfs).expect("failed to mount nullfs");

        let mount_output = Command::new("/sbin/mount")
            .output()
            .expect("Failed to execute mount");

        let output_string = String::from_utf8(mount_output.stdout).unwrap();

        assert!(output_string.contains(&format!(
            "{} on {} (nullfs",
            source.path().display(),
            rootfs.display()
        )));

        mount.unmount(rootfs).expect("failed to unmount nullfs");
    }

    #[test]
    fn test_mounting_devfs() {
        let destination = tempfile::tempdir().unwrap();

        let rootfs = destination.path();

        let mount = Mount {
            destination: "/".into(),
            source: None,
            options: None,
            r#type: "devfs".into(),
        };

        mount.mount(rootfs).expect("failed to mount devfs");

        let entries = std::fs::read_dir(rootfs)
            .expect("failed to read the mounted directory")
            .map(|res| res.map(|e| e.file_name()))
            .collect::<Result<Vec<_>, std::io::Error>>()
            .expect("failed to read a filename in the mounted directory");

        mount.unmount(rootfs).expect("failed to unmount devfs");
        assert_eq!(
            entries,
            vec![
                "random", "urandom", "console", "full", "null", "zero", "fd",
                "pts"
            ]
        );
    }
}
