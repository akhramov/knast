mod filesystem;
mod oci_extensions;
mod protocols;
mod task_service;

use std::{
    fs::remove_file,
    io::Error as StdError,
    sync::mpsc::{self, Receiver},
    thread, time,
};

use anyhow::Error;
use libc::{rfork, RFCFDG, RFPROC};
use libknast::operations::OciOperations;
use storage::TestStorage;
use ttrpc::{client::Client, context, server::Server};

use protocols::{shim::ConnectRequest, shim_ttrpc::TaskClient};
use task_service::TaskService;

const CONNECTION_RETRY_ATTEMPTS: u32 = 3;
const CONNECTION_TIMEOUT_NANOS: i64 = 1_000_000_000;

fn main() {
    let (command, id) = parse_opts();
    match &command[..] {
        "start" => start_command(),
        "delete" => delete_command(id),
        _ => panic!("Unknown command {:?}", command),
    }
}

fn start_command() {
    if parent_process().is_ok() {
        return;
    }

    match unsafe { rfork(RFPROC | RFCFDG) } {
        0 => {
            child_process();
        }
        -1 => {
            eprintln!("rfork failed {:?}", StdError::last_os_error());
        }
        _pid => parent_process().expect("Server is not running"),
    }
}

fn delete_command(id: String) {
    let _guard = setup_logging();
    let storage = storage();
    let ops = OciOperations::new(&storage, id)
        .expect("Failed to initialize runtime");

    ops.delete()
}

fn parent_process() -> Result<(), Error> {
    client().and_then(|client| {
        let request = ConnectRequest::new();
        Ok(client.connect(
            context::with_timeout(CONNECTION_TIMEOUT_NANOS),
            &request,
        )?)
    })?;

    let server_address = server_address()?;

    println!("{}", server_address.as_str());

    Ok(())
}

fn child_process() {
    let _guard = setup_logging();

    match server() {
        Ok((mut server, shutdown_notification)) => {
            server.start().expect("failed to start server");

            tracing::info!(
                "Server is listening at {}",
                server_address().unwrap().as_str()
            );

            if let Err(_) = shutdown_notification.recv() {
                tracing::error!(
                    "Sender dropped. Attempting to shutdown server"
                );
            }

            server.shutdown();
        }
        Err(err) => {
            tracing::error!("Server failed to start due to error: {:?}", err);
        }
    }
}

fn server() -> Result<(Server, Receiver<()>), Error> {
    let (sender, shutdown_notification) = mpsc::sync_channel(1);
    let nat_interface =
        std::env::var("NAT_INTERFACE").unwrap_or_else(|_| "lagg0".into());
    let service = protocols::shim_ttrpc::create_task(TaskService::new(
        storage(),
        sender,
        nat_interface,
    ));
    tracing::info!("Initializing server");
    let address = server_address()?;
    if let Err(error) = remove_file(address.path()) {
        tracing::info!("Previous socket wasn't deleted due to {}", error)
    };
    let server = Server::new()
        .bind(address.as_str())?
        .register_service(service);

    Ok((server, shutdown_notification))
}

fn client() -> Result<TaskClient, Error> {
    use nix::sys::socket::*;

    let socket = socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    )?;

    let sockaddr = UnixAddr::new(server_address()?.path().as_bytes())?;
    let sockaddr = SockAddr::Unix(sockaddr);

    let base: u32 = 2;
    let mut attempt: u32 = 1;
    let mut result;
    loop {
        result = connect(socket, &sockaddr);

        if result.is_err() && attempt < CONNECTION_RETRY_ATTEMPTS {
            let interval = time::Duration::from_secs(base.pow(attempt) as _);
            thread::sleep(interval);
            attempt = attempt + 1;
        } else {
            break;
        }
    }

    result?;

    let client = Client::new(socket);

    Ok(TaskClient::new(client))
}

fn storage() -> TestStorage {
    let home = std::env::var("HOME").unwrap();
    TestStorage::new(home).unwrap()
}

fn setup_logging() -> tracing_appender::non_blocking::WorkerGuard {
    let file_appender =
        tracing_appender::rolling::never("/var/log", "knast.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt().with_writer(non_blocking).init();

    guard
}

/// Returns command and container id
fn parse_opts() -> (String, String) {
    // Spike: this relies on arguments order.
    let mut args = std::env::args().rev();
    let command = args.next().expect("COMMAND is required");

    let id = if command == "start" {
        args.next()
    } else {
        args.next();
        args.next();
        args.next()
    }
    .expect("ID is required");

    (command, id)
}

// TODO: this should be static variable.
fn server_address() -> Result<url::Url, Error> {
    let address = url::Url::parse("unix:///tmp/knast.sock")?;

    Ok(address)
}
