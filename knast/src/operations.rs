mod command_ext;

use std::{
    convert::AsRef,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{anyhow, Error};
use baustelle::runtime_config::RuntimeConfig;
use jail::StoppedJail;
use jail::{param::Value, process::Jailed};
use serde::{Deserialize, Serialize};
use storage::{Storage, StorageEngine};

use crate::filesystem::{prefixed_destination, Mountable};

use command_ext::CommandExt;

const CONTAINER_STATE_STORAGE_KEY: &[u8] = b"CONTAINER_STATE";

#[derive(Deserialize, Serialize, Debug, Clone)]
struct ContainerState {
    pub config: RuntimeConfig,
    pub status: ContainerStatus,
    pub pid: u32,
    pub jid: u32,
    pub bundle_path: PathBuf,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
enum ContainerStatus {
    Created,
    Starting,
    Running,
    Stopped,
}

pub struct OciOperations<'a, T: StorageEngine> {
    config: Option<ContainerState>,
    storage: &'a Storage<T>,
    key: String,
}

impl<'a, T: StorageEngine> OciOperations<'a, T> {
    #[fehler::throws]
    pub fn new(storage: &'a Storage<T>, key: impl AsRef<str>) -> Self {
        let config = storage
            .get(CONTAINER_STATE_STORAGE_KEY, key.as_ref().as_bytes())?;

        Self {
            config,
            storage,
            key: key.as_ref().into(),
        }
    }

    /// Creates a container according to runtime
    /// configuration in bundle. Fails if container
    /// already exists, or configuration is invalid.
    /// Locks the configuration by creating a copy of
    /// configuration in the storage.
    #[fehler::throws]
    pub fn create(self, path: impl AsRef<Path>) {
        if self.config.is_some() {
            anyhow::bail!("Container '{}' already exists!", self.key);
        }

        let config_file = File::open(path.as_ref().join("config.json"))?;
        let reader = BufReader::new(config_file);
        let config: RuntimeConfig = serde_json::from_reader(reader)?;
        let rootfs_path = config
            .root
            .as_ref()
            .map(|root| root.path.clone())
            .ok_or_else(|| {
            anyhow!("Runtime config: root field must be set")
        })?;
        let rootfs = path.as_ref().join(rootfs_path);
        // Mountpoints validity check.
        for mountpoint in config.mounts.as_ref().unwrap_or(&vec![]) {
            mountpoint.mount(&rootfs)?;
            mountpoint.unmount(&rootfs)?;
        }

        let state = ContainerState {
            config,
            status: ContainerStatus::Created,
            pid: 0,
            jid: 0,
            bundle_path: path.as_ref().to_owned(),
        };

        self.storage.put(
            CONTAINER_STATE_STORAGE_KEY,
            self.key.as_bytes(),
            &state,
        )?;
    }

    /// Starts previously created container.
    #[fehler::throws]
    pub fn start(self) {
        tracing::info!("START command issued");
        if self.config.is_none() {
            anyhow::bail!("Container '{}' doesn't exist!", self.key);
        }
        let state = self
            .config
            .clone()
            .expect("Invariant violation: container state must exist!");
        // According to OCI spec & runc implementation, we can only
        // start created containers, not even stopped: https://git.io/JO0pb
        if state.status != ContainerStatus::Created {
            anyhow::bail!("Cannot start {:?} container", state.status);
        }
        let mut new_state = state.clone();
        new_state.status = ContainerStatus::Starting;
        self.storage.compare_and_swap(
            CONTAINER_STATE_STORAGE_KEY,
            self.key.as_bytes(),
            &state,
            &new_state,
        )?;
        let process = state.config.process.ok_or_else(|| {
            anyhow!("Runtime config: process field must be set")
        })?;
        let rootfs_path =
            state.config.root.map(|root| root.path).ok_or_else(|| {
                anyhow!("Runtime config: root field must be set")
            })?;
        let path = state.bundle_path.join(rootfs_path);
        let envs: Vec<(String, String)> = process
            .env
            .and_then(|envs| {
                envs.iter()
                    .map(|x| {
                        let mut params = x.split("=");

                        Some((params.next()?.into(), params.next()?.into()))
                    })
                    .collect()
            })
            .unwrap_or_else(Vec::new);
        let cwd = prefixed_destination(&path, &process.cwd);
        let uid = process.user.uid;
        let gid = process.user.gid;
        let mut args = process.args.unwrap_or_else(Vec::new).into_iter();
        let command = args
            .next()
            .ok_or_else(|| anyhow!("Runtime config: command is required"))?;
        let args: Vec<_> = args.collect();
        let mounts = state.config.mounts.unwrap_or_else(Vec::new);
        for mountpoint in &mounts {
            mountpoint.mount(&path)?;
        }

        println!("mounted");

        let stopped_jail = StoppedJail::new(&path)
            .name(&self.key)
            .param("vnet", Value::Int(1))
            .param("enforce_statfs", Value::Int(1));

        tracing::info!("Starting a jail for the process");
        let result: Result<_, Error> =
            stopped_jail.start().map_err(Error::from).and_then(|jail| {
                let result = Command::new(command)
                    .jail(&jail)
                    .args(args)
                    .env_clear()
                    .envs(envs)
                    .current_dir(cwd)
                    .uid(uid)
                    .gid(gid)
                    .spawn()
                    .map_err(Error::from);
                jail.defer_cleanup()?;

                result
            });

        match result {
            Err(error) => {
                tracing::error!("Error occured {}", error);
                self.cleanup(&path, &mounts)?;
                fehler::throw!(error);
            }
            Ok(mut handle) => {
                let status = handle.wait()?;
                println!("{:?}", status);
            }
        }
    }

    /// Frees resources allocated by Runtime for the container.
    /// [OCI lifecycle step 12](https://git.io/JO7NY).
    #[fehler::throws]
    fn cleanup(self, rootfs: impl AsRef<Path>, mounts: &[impl Mountable]) {
        for mount in mounts {
            mount.unmount(&rootfs)?;
        }

        self.storage
            .remove(CONTAINER_STATE_STORAGE_KEY, self.key.as_bytes())?;
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::OpenOptions,
        io::{Read, Seek, SeekFrom, Write},
        process::Command,
    };

    use gag::BufferRedirect;
    use storage::SledStorage;

    use super::*;

    #[test]
    fn test_linux_container_lifecycle() {
        run_container(
            "id",
            test_helpers::fixture!("commands_output/id"),
        );
        run_container(
            "mount",
            test_helpers::fixture!("commands_output/mount"),
        );
        run_container(
            "env",
            test_helpers::fixture!("commands_output/env"),
        );
    }

    fn run_container(cmd: &str, expected_output: &str) {
        let tmpdir = tempfile::tempdir().unwrap();
        let storage = SledStorage::new(tmpdir.path()).unwrap();
        let bundle = test_helpers::fixture_path!("container");
        Command::new("cp")
            .arg("-r")
            .arg(bundle)
            .arg(tmpdir.path())
            .status()
            .expect("failed to copy the bundle");
        let mut config_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(tmpdir.path().join("container/config.json"))
            .expect("failed to open config file");
        let reader = BufReader::new(&config_file);
        let mut config: RuntimeConfig =
            serde_json::from_reader(reader).expect("failed to open reader");
        config.process = config.process.and_then(|mut process| {
            process.args = Some(vec![cmd.into()]);

            Some(process)
        });
        let new_config = serde_json::to_string(&config).unwrap();
        config_file.set_len(0).unwrap();
        config_file.seek(SeekFrom::Start(0)).unwrap();
        config_file.write_all(new_config.as_bytes()).unwrap();

        let ops = OciOperations::new(&storage, "container!")
            .expect("failed to init OCI lifecycle struct");
        ops.create(tmpdir.path().join("container"))
            .expect("failed to create container");

        let ops = OciOperations::new(&storage, "container!")
            .expect("failed to init OCI lifecycle struct");
        let mut buf = BufferRedirect::stdout().unwrap();
        ops.start().expect("failed to start container");
        let mut output = String::new();
        buf.read_to_string(&mut output).unwrap();

        assert_eq!(output, expected_output);
    }
}
