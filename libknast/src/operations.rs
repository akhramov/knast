mod command_ext;
mod network;
mod utils;

use std::{
    convert::AsRef,
    fs::File,
    io::{BufReader, Error as IoError},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{anyhow, Error};
use baustelle::runtime_config::RuntimeConfig;
use jail::{param::Value, process::Jailed};
use jail::{RunningJail, StoppedJail};
use serde::{Deserialize, Serialize};
use storage::{Storage, StorageEngine};

use crate::filesystem::{prefixed_destination, Mountable};

use command_ext::CommandExt;

const CONTAINER_STATE_STORAGE_KEY: &[u8] = b"CONTAINER_STATE";

#[derive(Deserialize, Serialize, Debug, Clone)]
struct ContainerState {
    pub config: RuntimeConfig,
    pub status: ContainerStatus,
    pub pid: i32,
    pub jid: i32,
    pub bundle: PathBuf,
}

#[derive(
    Deserialize,
    Serialize,
    Debug,
    PartialEq,
    Clone,
    Copy,
    strum_macros::AsRefStr,
)]
#[strum(serialize_all = "lowercase")]
enum ContainerStatus {
    Created,
    Starting,
    Running,
    Stopped,
}

pub struct OciOperations<'a, T: StorageEngine> {
    state: Option<ContainerState>,
    storage: &'a Storage<T>,
    key: String,
}

impl<'a, T: StorageEngine> OciOperations<'a, T> {
    #[fehler::throws]
    pub fn new(storage: &'a Storage<T>, key: impl AsRef<str>) -> Self {
        let state = storage
            .get(CONTAINER_STATE_STORAGE_KEY, key.as_ref().as_bytes())?;

        Self {
            state,
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
    pub fn create(
        mut self,
        path: impl AsRef<Path>,
        nat_interface: Option<impl AsRef<str>>,
    ) {
        if self.state.is_some() {
            anyhow::bail!("Container '{}' already exists!", self.key);
        }

        let config_file = File::open(path.as_ref().join("config.json"))?;
        let reader = BufReader::new(config_file);
        let config: RuntimeConfig = serde_json::from_reader(reader)?;

        self.state = Some(ContainerState {
            config,
            status: ContainerStatus::Created,
            pid: 0,
            jid: 0,
            bundle: path.as_ref().to_owned(),
        });

        let rootfs = self.rootfs()?;
        // Mountpoints validity check.
        for mountpoint in self.mounts()? {
            mountpoint.mount(&rootfs)?;
        }

        let stopped_jail = StoppedJail::new(&rootfs.as_ref())
            .name(&self.key)
            .param("vnet", Value::Int(1))
            .param("allow.raw_sockets", Value::Int(1))
            .param("enforce_statfs", Value::Int(1));

        tracing::info!("Starting a jail for the process");
        let jail = stopped_jail.start()?;

        network::setup(self.storage, &self.key, jail, nat_interface)?;
        self.persist_state(self.retrieve_state()?)?;
    }

    /// Starts previously created container.
    #[fehler::throws]
    pub fn start(mut self) {
        tracing::info!("START command issued");
        let state = self.retrieve_state()?;
        // According to OCI spec & runc implementation, we can only
        // start created containers, not even stopped: https://git.io/JO0pb
        if state.status != ContainerStatus::Created {
            anyhow::bail!("Cannot start {} container", state.status.as_ref());
        }
        let process = state.config.process.clone().ok_or_else(|| {
            anyhow!("Runtime config: process field must be set")
        })?;
        let rootfs = self.rootfs()?;
        let path = rootfs.as_ref();
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

        let mut new_state = state.clone();
        new_state.status = ContainerStatus::Starting;
        self.storage.compare_and_swap(
            CONTAINER_STATE_STORAGE_KEY,
            self.key.as_bytes(),
            Some(state),
            Some(&mut new_state),
        )?;

        let jail = self.retrieve_jail()?;
        let result = Command::new(command)
            .jail(&jail)
            .args(args)
            .env_clear()
            .envs(envs)
            .current_dir(cwd)
            .uid(uid)
            .gid(gid)
            .spawn();

        match result {
            Err(error) => {
                tracing::error!("Error occured {}", error);
                let mut state = state.clone();
                state.status = ContainerStatus::Stopped;
                self.persist_state(&state)?;
                self.state = Some(state);
                self.delete();
                fehler::throw!(error);
            }
            Ok(mut handle) => {
                let mut state = state.clone();
                state.status = ContainerStatus::Running;
                state.pid = handle.id() as _;
                state.jid = jail.jid;
                self.persist_state(&state)?;
                let status = handle.wait()?;
                state.pid = 0;
                state.status = ContainerStatus::Stopped;
                self.persist_state(&state)?;
                tracing::info!("Process exited with {:?}", status);
            }
        }
    }

    // TODO: logs errors, don't return
    /// Frees resources allocated by Runtime for the
    /// container. [OCI lifecycle steps 11-12](https://git.io/JO7NY).
    pub fn delete(self) {
        if let Err(err) = self.cleanup() {
            tracing::error!("Failed to delete container: {}", err);
        }
    }

    /// Sends a signal to the process
    #[fehler::throws]
    pub fn kill(self, signal: i32) {
        tracing::info!("killing container with {}", signal);
        let state = self.retrieve_state()?;
        if state.status != ContainerStatus::Running {
            anyhow::bail!("Cannot kill {} container.", state.status.as_ref());
        }

        let jail = self.retrieve_jail()?;

        utils::run_in_fork(|| {
            jail.attach().map_err(Error::from).and_then(|_| unsafe {
                if libc::kill(state.pid, signal) < 0 {
                    anyhow::bail!(
                        "kill failed: {:?}",
                        IoError::last_os_error()
                    )
                }

                Ok(())
            })
        })?
    }

    #[fehler::throws]
    pub fn state(&self) -> serde_json::Value {
        let state = self.retrieve_state()?;
        let jail = self.retrieve_jail();

        let status_str = state.status.as_ref();
        let status = match (jail, state.status) {
            (Ok(_), ContainerStatus::Running) => "running",
            (Err(_), ContainerStatus::Running) => "stopped",
            _ => status_str,
        };

        serde_json::json!({
            "ociVersion": "1.0.2-dev-freebsd",
            "id": self.key,
            "status": status,
            "pid": state.pid,
            "bundle": state.bundle,
        })
    }

    #[fehler::throws]
    fn rootfs(&self) -> impl AsRef<Path> {
        let state = self.retrieve_state()?;

        let rootfs_path = state
            .config
            .root
            .as_ref()
            .map(|root| root.path.clone())
            .ok_or_else(|| {
                anyhow!("Runtime config: root field must be set")
            })?;

        state.bundle.join(rootfs_path)
    }

    #[fehler::throws]
    fn mounts(&self) -> Vec<impl Mountable> {
        let state = self.retrieve_state()?;

        state.config.mounts.clone().unwrap_or_else(Vec::new)
    }

    #[fehler::throws]
    fn persist_state(&self, state: &ContainerState) {
        self.storage.put(
            CONTAINER_STATE_STORAGE_KEY,
            self.key.as_bytes(),
            state,
        )?;
    }

    #[fehler::throws]
    fn retrieve_state(&'a self) -> &'a ContainerState {
        self.state.as_ref().ok_or_else(|| {
            anyhow!("Container '{}' doesn't exist!", self.key)
        })?
    }

    #[fehler::throws]
    fn retrieve_jail(&'a self) -> RunningJail {
        RunningJail::from_name(&self.key)
            .map_err(|_| anyhow!("Container is not running"))?
    }

    #[fehler::throws]
    fn cleanup(self) {
        self.retrieve_jail()?.defer_cleanup()?;

        let status = &self.state()?["status"];
        if status != "stopped" && status != "created" {
            anyhow::bail!(
                "Cannot delete {} container. Must be stopped first",
                status
            );
        }

        self.storage
            .remove(CONTAINER_STATE_STORAGE_KEY, self.key.as_bytes())?;

        let rootfs = self.rootfs()?;
        for mount in self.mounts()?.iter().rev() {
            mount.unmount(&rootfs)?;
        }

        network::teardown(self.storage, self.key)?;
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::OpenOptions,
        io::{Read, Seek, SeekFrom, Write},
        process::Command,
        sync::Arc,
    };

    use gag::BufferRedirect;
    use storage::SledStorage;
    use tempfile::TempDir;

    use super::*;

    /// Some tests are capturing output, we can't run them
    /// in parallel.
    #[test]
    fn test_linux_container_lifecycle() {
        test_lifecycle();
        test_kill_command();
    }

    #[test]
    #[should_panic(expected = "Cannot kill stopped container")]
    fn test_kill_command_stopped_container() {
        let (storage, tempdir) = prepare_bundle("quitely_stop.sh");

        create_container(storage.clone(), "container3", tempdir.path());
        start_container(storage.clone(), "container3");

        kill_container(storage.clone(), "container3", libc::SIGBUS);
    }

    fn test_lifecycle() {
        run_container(
            "linux",
            "id",
            test_helpers::fixture!("commands_output/id"),
        );
        run_container(
            "linux3",
            "mount",
            test_helpers::fixture!("commands_output/mount"),
        );
        run_container(
            "linux",
            "env",
            test_helpers::fixture!("commands_output/env"),
        );
    }

    // TODO: delete created container
    fn test_kill_command() {
        use std::{thread, time};
        let (storage, tempdir) = prepare_bundle("/bin/trapster.sh");

        create_container(storage.clone(), "trapster", tempdir.path());
        let storage_copy = storage.clone();
        let thread = thread::spawn(move || {
            let output = capture_output(|| {
                start_container(storage_copy.clone(), "trapster");
            });
            assert_eq!(
                output,
                test_helpers::fixture!("commands_output/trapster_sigbus")
            );
        });

        let delay = time::Duration::from_millis(10);
        thread::sleep(delay);

        kill_container(storage.clone(), "trapster", libc::SIGBUS);
        thread::sleep(delay);

        thread.join().unwrap();
    }

    /// Runs the container
    /// Panics if command output is not equal to expected
    /// output
    fn run_container(name: &str, cmd: &str, expected_output: &str) {
        let (storage, tempdir) = prepare_bundle(cmd);
        let path = tempdir.path();

        create_container(storage.clone(), name, path);

        let output = capture_output(|| start_container(storage.clone(), name));
        assert_eq!(output, expected_output);

        delete_container(storage, name);
    }

    fn create_container(storage: Arc<SledStorage>, name: &str, path: &Path) {
        OciOperations::new(&storage.clone(), name)
            .expect("failed to init OCI lifecycle struct")
            .create(path.join("container"), Some("lo0"))
            .expect("failed to create container");
    }

    fn start_container(storage: Arc<SledStorage>, name: &str) {
        OciOperations::new(&storage.clone(), name)
            .expect("failed to init OCI lifecycle struct")
            .start()
            .expect("failed to start container");
    }

    fn kill_container(storage: Arc<SledStorage>, name: &str, signal: i32) {
        OciOperations::new(&storage.clone(), name)
            .expect("failed to init OCI lifecycle struct")
            .kill(signal)
            .expect("failed to send signal to the container");
    }

    /// Delete the container
    /// Panics if there are mounted volumes left
    fn delete_container(storage: Arc<SledStorage>, name: &str) {
        let bundle = OciOperations::new(&storage.clone(), name)
            .expect("failed to init OCI lifecycle struct")
            .state
            .expect("container should be initialized")
            .bundle;
        OciOperations::new(&storage.clone(), name)
            .expect("failed to init OCI lifecycle struct")
            .delete();

        let mount_output = Command::new("/sbin/mount")
            .output()
            .expect("Failed to execute mount");
        let output_string = String::from_utf8(mount_output.stdout).unwrap();

        assert!(!output_string
            .contains(&bundle.into_os_string().into_string().unwrap()));
    }

    /// Captures stdout.
    fn capture_output(fun: impl FnOnce()) -> String {
        let mut buf = BufferRedirect::stdout().unwrap();
        fun();
        let mut output = String::new();
        buf.read_to_string(&mut output).unwrap();

        output
    }

    fn prepare_bundle(cmd: &str) -> (Arc<SledStorage>, TempDir) {
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

        (Arc::new(storage), tmpdir)
    }
}
