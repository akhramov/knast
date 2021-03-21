/// Handles mounting and unmounting of filesystems.
/// Some filesystems require additional actions on mount, i.e. devfs nodes
/// need to be hidden using the rule subsystem, and so on.
mod devfs;
mod mount;

use std::{convert::AsRef, path::Path};

use anyhow::Error;

use baustelle::runtime_config::Mount;

pub trait Mountable {
    #[fehler::throws]
    fn mount(&self) {
        mount::mount(
            self.kind(),
            self.source(),
            self.destination(),
            self.options().iter().map(|x| x as &dyn AsRef<str>),
        )?;

        self.post_mount_hooks()?;
    }

    #[fehler::throws]
    fn unmount(&self) {
        mount::unmount(&self.destination())?;
    }

    #[fehler::throws]
    fn post_mount_hooks(&self);

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
    fn post_mount_hooks(&self) {
        if self.r#type == "devfs" {
            prepare_devfs(&self.destination)?;
        }
    }
}

/// There's no FreeBSD spec yet, so follow Linux config as possible
/// https://github.com/opencontainers/runtime-spec/blob/1c3f411f041711bbeecf35ff7e93461ea6789220/config-linux.md#default-devices
#[fehler::throws]
fn prepare_devfs(path: impl AsRef<Path>) {
    use devfs::{apply, Operation};

    const DEFAULT_DEVICES: [&str; 9] = [
        "null", "zero", "full", "random", "urandom", "tty", "console", "pts",
        "pts/*",
    ];

    apply(&path, Operation::HideAll)?;

    for device in &DEFAULT_DEVICES {
        apply(&path, Operation::Unhide(device))?
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    #[test]
    fn test_mounting_nullfs() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();

        let dest = destination.path().to_str().unwrap().into();
        let src = source.path().to_str().unwrap().into();

        let mount = Mount {
            destination: dest,
            source: Some(src),
            options: None,
            r#type: "nullfs".into(),
        };

        mount.mount().expect("failed to mount nullfs");

        let mount_output = Command::new("mount")
            .output()
            .expect("Failed to execute mount");

        let output_string = String::from_utf8(mount_output.stdout).unwrap();

        assert!(output_string.contains(&format!(
            "{} on {} (nullfs",
            source.path().display(),
            destination.path().display()
        )));

        mount.unmount().expect("failed to unmount nullfs");
    }

    #[test]
    fn test_mounting_devfs() {
        let destination = tempfile::tempdir().unwrap();

        let dest = destination.path().to_str().unwrap().into();

        let mount = Mount {
            destination: dest,
            source: None,
            options: None,
            r#type: "devfs".into(),
        };

        mount.mount().expect("failed to mount devfs");

        let entries = std::fs::read_dir(destination.path())
            .expect("failed to read the mounted directory")
            .map(|res| res.map(|e| e.file_name()))
            .collect::<Result<Vec<_>, std::io::Error>>()
            .expect("failed to read a filename in the mounted directory");

        mount.unmount().expect("failed to unmount devfs");
        assert_eq!(
            entries,
            vec![
                "random", "urandom", "console", "full", "null", "zero", "pts"
            ]
        );
    }
}
