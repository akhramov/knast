mod command_ext;
mod network;
mod utils;

use std::{
    convert::AsRef,
    fs::File,
    io::{BufReader, Error as IoError},
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::filesystem::{prefixed_destination, Mountable};
use anyhow::{anyhow, Error};
pub use baustelle::runtime_config::{Process, Root, RuntimeConfig};
use jail::{param::Value, process::Jailed};
use jail::{RunningJail, StoppedJail};
use nix::{
    sys::wait::{waitpid, WaitStatus},
    unistd::Pid,
};
use serde::{Deserialize, Serialize};
use storage::{Storage, StorageEngine};

use command_ext::CommandExt;

const CONTAINER_CONFIG_STORAGE_KEY: &[u8] = b"CONTAINER_CONFIG";
const CONTAINER_PROCESSES_STORAGE_KEY: &[u8] = b"CONTAINER_PROCESSES";
const OCI_VERSION: &str = "1.0.2-dev-freebsd";
const MAIN_PROCESS_EXEC_ID: &str = "";

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
pub enum ProcessStatus {
    Created,
    Starting,
    Running,
    Stopped,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OciStatus {
    pub oci_version: String,
    pub status: ProcessStatus,
    pub pid: i32,
    pub jid: i32,
    pub exit_status: Option<i32>,
    pub exited_at: SystemTime,
}

pub struct OciOperations<'a, T: StorageEngine> {
    storage: &'a Storage<T>,
    key: String,
}

impl<'a, T: StorageEngine> OciOperations<'a, T> {
    #[fehler::throws]
    pub fn new(storage: &'a Storage<T>, key: impl AsRef<str>) -> Self {
        Self {
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
        self,
        path: impl AsRef<Path>,
        nat_interface: Option<impl AsRef<str>>,
    ) {
        if self.get_process(MAIN_PROCESS_EXEC_ID).is_ok() {
            anyhow::bail!("Container '{}' already exists!", self.key);
        }

        let config_file = File::open(path.as_ref().join("config.json"))?;
        let reader = BufReader::new(config_file);
        let mut config: RuntimeConfig = serde_json::from_reader(reader)?;
        let rootfs_path = config
            .root
            .as_ref()
            .map(|root| path.as_ref().join(root.path.clone()))
            .ok_or_else(|| {
                anyhow!("Runtime config: root field must be set")
            })?;

        config.root = Some(Root {
            path: rootfs_path,
            readonly: None,
        });

        self.storage.put(
            CONTAINER_CONFIG_STORAGE_KEY,
            self.key.as_bytes(),
            config,
        )?;

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
    }

    /// Starts previously created container.
    #[fehler::throws]
    pub fn start(self) {
        tracing::info!("START command issued");

        self.do_start(MAIN_PROCESS_EXEC_ID, |_| Ok(()))?
    }

    /// Frees resources allocated by Runtime for the
    /// container. [OCI lifecycle steps 11-12](https://git.io/JO7NY).
    pub fn delete(&self) {
        self.do_delete(MAIN_PROCESS_EXEC_ID);
    }

    pub fn do_delete(&self, exec_id: &str) {
        if let Err(err) = self.cleanup(exec_id) {
            tracing::error!("Failed to delete process: {}", err);
        }
    }

    /// Sends a signal to the process
    #[fehler::throws]
    pub fn kill(self, signal: i32) {
        self.do_kill(MAIN_PROCESS_EXEC_ID, signal)?;
    }

    #[fehler::throws]
    pub fn do_kill(self, exec_id: &str, signal: i32) {
        tracing::info!("killing container with {}", signal);
        let state = &self.get_process(exec_id)?;
        if state.status != ProcessStatus::Running {
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
    pub fn state(&self) -> OciStatus {
        self.get_state(MAIN_PROCESS_EXEC_ID)?
    }

    #[fehler::throws]
    pub fn get_state(&self, exec_id: &str) -> OciStatus {
        let mut process = self.get_process(exec_id)?;
        let jail = self.retrieve_jail();

        process.status = match (jail, process.status) {
            (Ok(_), ProcessStatus::Running) => ProcessStatus::Running,
            (Err(_), ProcessStatus::Running) => ProcessStatus::Running,
            (_, status) => status,
        };

        process
    }

    pub fn storage(&'a self) -> &'a Storage<T> {
        self.storage
    }

    pub fn key(&'a self) -> &'a String {
        &self.key
    }

    #[fehler::throws]
    pub fn do_start(
        &self,
        exec_id: &str,
        f: impl FnOnce(&mut Command) -> Result<(), Error>,
    ) {
        let config = self.config()?;
        let process = config.process.clone().ok_or_else(|| {
            anyhow!("Runtime config: process field must be set")
        })?;

        self.do_exec(exec_id, process, f)?
    }

    #[fehler::throws]
    pub fn do_exec(
        &self,
        exec_id: &str,
        process: Process,
        f: impl FnOnce(&mut Command) -> Result<(), Error>,
    ) {
        self.new_process(exec_id)?;
        let process_status = self.get_process(exec_id)?.status;
        // According to OCI spec & runc implementation, we can only
        // start created containers, not even stopped: https://git.io/JO0pb
        if process_status != ProcessStatus::Created {
            anyhow::bail!("Cannot start {} process", process_status.as_ref());
        }
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

        self.update_process(exec_id, |process| {
            process.status = ProcessStatus::Starting;
        })?;

        let jail = self.retrieve_jail()?;
        let mut process = Command::new(command);
        f(&mut process)?;

        let result = process
            .jail(&jail)
            .args(args)
            .env_clear()
            .envs(envs)
            .current_dir(cwd)
            .uid(uid)
            .gid(gid)
            .spawn();
        jail.defer_cleanup()?;

        match result {
            Err(error) => {
                tracing::error!("Error occured {}", error);
                self.update_process(exec_id, |process| {
                    process.status = ProcessStatus::Stopped;
                })?;
                fehler::throw!(error);
            }
            Ok(handle) => {
                tracing::info!("Started child process {:?}", handle);
                self.update_process(exec_id, |process| {
                    process.status = ProcessStatus::Running;
                    process.pid = handle.id() as _;
                    process.jid = jail.jid;
                })?;
            }
        }
    }

    #[fehler::throws]
    pub fn wait(&self) {
        self.do_wait(MAIN_PROCESS_EXEC_ID)?
    }

    #[fehler::throws]
    pub fn do_wait(&self, exec_id: &str) {
        let process = self.get_process(exec_id)?;
        tracing::info!("Waiting for child {:?}", process.pid);

        let exit_status = waitpid(Pid::from_raw(process.pid), None)
            .map(|status| match status {
                WaitStatus::Exited(_, code) => Some(code),
                _ => None,
            })
            .map_err(Error::from)?;

        self.update_process(exec_id, |process| {
            process.pid = 0;
            process.status = ProcessStatus::Stopped;
            process.exit_status = exit_status;
            process.exited_at = SystemTime::now();
        })?;
        tracing::info!("Process exited with {:?}", exit_status);
    }

    #[fehler::throws]
    fn rootfs(&self) -> impl AsRef<Path> {
        let config = self.config()?;

        config
            .root
            .as_ref()
            .map(|root| root.path.clone())
            .ok_or_else(|| anyhow!("Runtime config: root field must be set"))?
    }

    #[fehler::throws]
    fn mounts(&self) -> Vec<impl Mountable> {
        let config = self.config()?;

        config.mounts.clone().unwrap_or_else(Vec::new)
    }

    #[fehler::throws]
    fn config(&self) -> RuntimeConfig {
        self.storage
            .get(CONTAINER_CONFIG_STORAGE_KEY, self.key.as_bytes())?
            .ok_or_else(|| {
                anyhow!("Container '{}' doesn't exist!", self.key)
            })?
    }

    pub fn process_id(&self, exec_id: &str) -> Vec<u8> {
        [self.key.as_bytes(), b"/", exec_id.as_bytes()].concat()
    }

    #[fehler::throws]
    fn get_process(&self, exec_id: &str) -> OciStatus {
        self.storage
            .get(CONTAINER_PROCESSES_STORAGE_KEY, self.process_id(exec_id))?
            .ok_or_else(|| anyhow!("Process '{}' doesn't exist!", exec_id))?
    }

    #[fehler::throws]
    fn update_process(&self, exec_id: &str, f: impl FnOnce(&mut OciStatus)) {
        let process = self.get_process(exec_id)?;
        let mut new_process = process.clone();

        f(&mut new_process);

        self.storage.compare_and_swap(
            CONTAINER_PROCESSES_STORAGE_KEY,
            self.process_id(exec_id),
            Some(process),
            Some(new_process),
        )?;
    }

    #[fehler::throws]
    fn new_process(&self, exec_id: &str) {
        self.storage.compare_and_swap(
            CONTAINER_PROCESSES_STORAGE_KEY,
            self.process_id(exec_id),
            None,
            Some(OciStatus {
                oci_version: OCI_VERSION.into(),
                status: ProcessStatus::Created,
                pid: 0,
                jid: 0,
                exit_status: None,
                exited_at: UNIX_EPOCH,
            }),
        )?;
    }

    #[fehler::throws]
    fn delete_process(&self, exec_id: &str) {
        self.storage.remove(
            CONTAINER_PROCESSES_STORAGE_KEY,
            self.process_id(exec_id),
        )?
    }

    #[fehler::throws]
    fn retrieve_jail(&'a self) -> RunningJail {
        RunningJail::from_name(&self.key)
            .map_err(|_| anyhow!("Container is not running"))?
    }

    #[fehler::throws]
    fn cleanup(&self, exec_id: &str) {
        let status = &self.get_state(exec_id)?.status;
        if status != &ProcessStatus::Stopped
            && status != &ProcessStatus::Created
        {
            anyhow::bail!(
                "Cannot delete {} container. Must be stopped first",
                status.as_ref()
            );
        }

        self.delete_process(exec_id)?;

        let rootfs = self.rootfs()?;
        for mount in self.mounts()?.iter().rev() {
            mount.unmount(&rootfs)?;
        }

        network::teardown(self.storage, self.key.clone())?;
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
    use storage::TestStorage;
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

    fn create_container(storage: Arc<TestStorage>, name: &str, path: &Path) {
        OciOperations::new(&storage.clone(), name)
            .expect("failed to init OCI lifecycle struct")
            .create(path.join("container"), Some("lo0"))
            .expect("failed to create container");
    }

    fn start_container(storage: Arc<TestStorage>, name: &str) {
        OciOperations::new(&storage.clone(), name)
            .expect("failed to init OCI lifecycle struct")
            .start()
            .expect("failed to start container");

        OciOperations::new(&storage.clone(), name)
            .expect("failed to init OCI lifecycle struct")
            .wait()
            .expect("failed to wait container");
    }

    fn kill_container(storage: Arc<TestStorage>, name: &str, signal: i32) {
        OciOperations::new(&storage.clone(), name)
            .expect("failed to init OCI lifecycle struct")
            .kill(signal)
            .expect("failed to send signal to the container");
    }

    /// Delete the container
    /// Panics if there are mounted volumes left
    fn delete_container(storage: Arc<TestStorage>, name: &str) {
        OciOperations::new(&storage.clone(), name)
            .expect("failed to init OCI lifecycle struct")
            .delete();

        let mount_output = Command::new("/sbin/mount")
            .output()
            .expect("Failed to execute mount");
        let output_string = String::from_utf8(mount_output.stdout).unwrap();

        assert!(!output_string.contains(name));
    }

    /// Captures stdout.
    fn capture_output(fun: impl FnOnce()) -> String {
        let mut buf = BufferRedirect::stdout().unwrap();
        fun();
        let mut output = String::new();
        buf.read_to_string(&mut output).unwrap();

        output
    }

    fn prepare_bundle(cmd: &str) -> (Arc<TestStorage>, TempDir) {
        let tmpdir = tempfile::tempdir().unwrap();
        let storage = TestStorage::new(tmpdir.path()).unwrap();
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
