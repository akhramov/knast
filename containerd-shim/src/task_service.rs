use std::{
    convert::TryInto,
    path::Path,
    process,
    sync::{mpsc::SyncSender, Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Error;
use libknast::{
    filesystem::Mountable,
    operations::{OciOperations, Process, ProcessStatus},
};
use protobuf::well_known_types::Timestamp;
use storage::{Storage, StorageEngine};
use ttrpc::TtrpcContext;

use super::{
    oci_extensions::{ContainerdExtension, StdioTriple},
    protocols::{
        empty::Empty,
        shim::{
            ConnectRequest, ConnectResponse, CreateTaskRequest,
            CreateTaskResponse, DeleteRequest, DeleteResponse,
            ExecProcessRequest, ResizePtyRequest, ShutdownRequest,
            StartRequest, StartResponse, StateRequest, StateResponse,
            WaitRequest, WaitResponse,
        },
        shim_ttrpc::Task,
        task::Status,
    },
};

#[derive(Debug)]
pub struct TaskService<T: StorageEngine + Send + Sync> {
    storage: Storage<T>,
    shutdown_notifier: SyncSender<()>,
    nat_interface: String,
    start_mutex: Mutex<()>,
}

impl<T: StorageEngine + Send + Sync + 'static> TaskService<T> {
    pub fn new(
        storage: Storage<T>,
        sender: SyncSender<()>,
        nat_interface: String,
    ) -> Arc<Box<dyn Task + Send + Sync>> {
        Arc::new(Box::new(Self {
            storage,
            shutdown_notifier: sender.clone(),
            nat_interface,
            start_mutex: Mutex::new(()),
        }))
    }

    fn operations(&self, id: String) -> Result<OciOperations<T>, Error> {
        OciOperations::new(&self.storage, id)
    }
}

impl<T: StorageEngine + Send + Sync + 'static> Task for TaskService<T> {
    #[tracing::instrument(err, skip(self, _ctx), fields(id = request.id.as_str()))]
    fn state(
        &self,
        _ctx: &TtrpcContext,
        request: StateRequest,
    ) -> ttrpc::Result<StateResponse> {
        let ops = self
            .operations(request.id.clone())
            .map_err(error_response)?;
        let stdio =
            ops.stdio_triple(&request.exec_id).map_err(error_response)?;
        let state = ops.get_state(&request.exec_id).map_err(error_response)?;
        let exit_status: u32 = state
            .exit_status
            .unwrap_or(0)
            .try_into()
            .map_err(error_response)?;
        let exited_at = system_time_to_timestamp(state.exited_at)
            .map(Option::Some)
            .map_err(error_response)?
            .into();

        Ok(StateResponse {
            id: request.id,
            pid: state.pid.try_into().map_err(error_response)?,
            status: state.status.into(),
            stdin: stdio.stdin,
            stdout: stdio.stdout,
            stderr: stdio.stderr,
            // TODO: need terminal flag implementation in libknast
            terminal: false,
            exit_status,
            exited_at,
            exec_id: request.exec_id,
            ..Default::default()
        })
    }

    #[tracing::instrument(
        err, skip(self, _ctx),
        fields(
            id = request.id.as_str(),
            bundle = request.bundle.as_str()
        )
    )]
    fn create(
        &self,
        _ctx: &TtrpcContext,
        request: CreateTaskRequest,
    ) -> ttrpc::Result<CreateTaskResponse> {
        tracing::info!("Creating container");
        let ops = self.operations(request.id).map_err(error_response)?;
        ops.save_stdio_triple(
            "",
            StdioTriple {
                stdin: request.stdin,
                stdout: request.stdout,
                stderr: request.stderr,
                terminal: request.terminal,
            },
        )
        .map_err(error_response)?;
        for mountpoint in request.rootfs {
            let rootfs = Path::new(&request.bundle).join("rootfs");

            mountpoint.mount(rootfs).map_err(error_response)?;
        }

        ops.create(&request.bundle, Some(&self.nat_interface))
            .map_err(error_response)?;

        Ok(CreateTaskResponse::new())
    }

    #[tracing::instrument(err, skip(self, _ctx))]
    fn start(
        &self,
        _ctx: &TtrpcContext,
        request: StartRequest,
    ) -> ttrpc::Result<StartResponse> {
        let _guard = self.start_mutex.lock();

        if !request.exec_id.is_empty() {
            return Ok(StartResponse::new());
        }

        tracing::info!("Starting container");
        let ops = self
            .operations(request.id.clone())
            .map_err(error_response)?;
        <OciOperations<T> as ContainerdExtension>::start(
            ops,
            &request.exec_id,
        )
        .map_err(error_response)?;

        Ok(StartResponse::new())
    }

    #[tracing::instrument(err, skip(self, _ctx), fields(id = request.id.as_str()))]
    fn connect(
        &self,
        _ctx: &TtrpcContext,
        request: ConnectRequest,
    ) -> ttrpc::Result<ConnectResponse> {
        tracing::info!("Connection test");
        let pid = process::id();
        Ok(ConnectResponse {
            shim_pid: pid,
            task_pid: pid,
            ..Default::default()
        })
    }

    #[tracing::instrument(err, skip(self, _ctx), fields(id = request.id.as_str()))]
    fn delete(
        &self,
        _ctx: &TtrpcContext,
        request: DeleteRequest,
    ) -> ttrpc::Result<DeleteResponse> {
        tracing::info!("Deleting container");
        let ops = self.operations(request.id).map_err(error_response)?;
        let state = ops.state().map_err(error_response)?;
        let exit_status: u32 = state
            .exit_status
            .unwrap_or(0)
            .try_into()
            .map_err(error_response)?;
        let exited_at = system_time_to_timestamp(state.exited_at)
            .map(Option::Some)
            .map_err(error_response)?
            .into();
        ops.delete_process(&request.exec_id)
            .map_err(error_response)?;

        Ok(DeleteResponse {
            pid: state.pid.try_into().map_err(error_response)?,
            exit_status,
            exited_at,
            ..Default::default()
        })
    }

    #[tracing::instrument(err, skip(self, _ctx), fields(id = request.id.as_str()))]
    fn wait(
        &self,
        _ctx: &TtrpcContext,
        request: WaitRequest,
    ) -> ttrpc::Result<WaitResponse> {
        {
            let _guard = self.start_mutex.lock();
        }
        let ops = self
            .operations(request.id.clone())
            .map_err(error_response)?;
        ops.do_wait(&request.exec_id).map_err(error_response)?;
        let state = ops.get_state(&request.exec_id).map_err(error_response)?;
        let exit_status: u32 = state
            .exit_status
            .unwrap_or(0)
            .try_into()
            .map_err(error_response)?;
        let exited_at = system_time_to_timestamp(state.exited_at)
            .map(Option::Some)
            .map_err(error_response)?
            .into();
        ops.delete();
        Ok(WaitResponse {
            exit_status,
            exited_at,
            ..Default::default()
        })
    }

    #[tracing::instrument(err, skip(self, _ctx))]
    fn shutdown(
        &self,
        _ctx: &TtrpcContext,
        _req: ShutdownRequest,
    ) -> ::ttrpc::Result<Empty> {
        tracing::info!("Shutdown request received");
        // TODO: reference counting
        Ok(Empty::default())
    }

    #[tracing::instrument(err, skip(self, _ctx))]
    fn exec(
        &self,
        _ctx: &TtrpcContext,
        request: ExecProcessRequest,
    ) -> ttrpc::Result<Empty> {
        tracing::info!("Exec process");
        let _guard = self.start_mutex.lock();
        let process: Process = request
            .spec
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!("Spec is required but wasn't provided")
            })
            .and_then(|spec| Ok(serde_json::from_slice(&spec.value)?))
            .map_err(error_response)?;

        let ops = self.operations(request.id).map_err(error_response)?;
        ops.save_stdio_triple(
            &request.exec_id,
            StdioTriple {
                stdin: request.stdin,
                stdout: request.stdout,
                stderr: request.stderr,
                terminal: request.terminal,
            },
        )
        .map_err(error_response)?;
        ops.exec(&request.exec_id, process)
            .map_err(error_response)?;

        Ok(Empty::default())
    }

    #[tracing::instrument(err, skip(self, _ctx), fields(id = request.id.as_str()))]
    fn resize_pty(
        &self,
        _ctx: &TtrpcContext,
        request: ResizePtyRequest,
    ) -> ttrpc::Result<Empty> {
        tracing::info!("Resizing pty");
        use nix::pty::Winsize;
        let winsize = Winsize {
            ws_row: request.height as _,
            ws_col: request.width as _,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        self.operations(request.id)
            .map_err(error_response)?
            .resize_pty(&request.exec_id, winsize)
            .map_err(error_response)?;

        Ok(Empty::default())
    }
}

impl From<ProcessStatus> for Status {
    fn from(status: ProcessStatus) -> Self {
        match status {
            ProcessStatus::Created => Status::CREATED,
            ProcessStatus::Running => Status::RUNNING,
            ProcessStatus::Stopped => Status::STOPPED,
            _ => Status::UNKNOWN,
        }
    }
}

fn error_response(err: impl ToString) -> ttrpc::Error {
    ttrpc::Error::RpcStatus(ttrpc::get_status(ttrpc::Code::INTERNAL, err))
}

fn system_time_to_timestamp(time: SystemTime) -> Result<Timestamp, Error> {
    let duration = time.duration_since(UNIX_EPOCH)?;

    Ok(Timestamp {
        seconds: duration.as_secs().try_into()?,
        nanos: duration.subsec_nanos().try_into()?,
        ..Default::default()
    })
}
