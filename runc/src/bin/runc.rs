use std::process::exit;

use clap::{load_yaml, App, ArgMatches};
use libknast::operations::OciOperations;
use storage::{SledStorage, StorageEngine};

fn main() {
    let yaml = load_yaml!("runc.yaml");
    let matches = App::from(yaml).get_matches();
    let home = std::env::var("HOME").unwrap();
    let storage = SledStorage::new(home).unwrap();
    let container_id =
        |matches: &ArgMatches| matches.value_of("ID").unwrap().to_owned();

    if let Some(matches) = matches.subcommand_matches("state") {
        let ops = OciOperations::new(&storage, container_id(matches)).unwrap();

        return state(ops);
    }
    if let Some(matches) = matches.subcommand_matches("create") {
        let ops = OciOperations::new(&storage, container_id(matches)).unwrap();
        let bundle = matches.value_of("BUNDLE").unwrap();
        let interface = matches.value_of("nat-interface").unwrap();

        return create(ops, bundle, interface);
    }
    if let Some(matches) = matches.subcommand_matches("start") {
        let ops = OciOperations::new(&storage, container_id(matches)).unwrap();

        return start(ops);
    }
    if let Some(matches) = matches.subcommand_matches("kill") {
        let ops = OciOperations::new(&storage, container_id(matches)).unwrap();
        let signal = matches.value_of("SIGNAL").unwrap().parse().unwrap();

        return kill(ops, signal);
    }
    if let Some(matches) = matches.subcommand_matches("delete") {
        let ops = OciOperations::new(&storage, container_id(matches)).unwrap();

        return delete(ops);
    }
}

fn state(ops: OciOperations<impl StorageEngine>) {
    match ops.state() {
        Ok(result) => println!("{}", result),
        Err(error) => {
            println!("{}", error);
            exit(1);
        }
    }
}

fn create(
    ops: OciOperations<impl StorageEngine>,
    bundle: &str,
    nat_interface: &str,
) {
    match ops.create(bundle, Some(nat_interface)) {
        Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            exit(1);
        }
    }
}

fn start(ops: OciOperations<impl StorageEngine>) {
    match ops.start() {
        Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            exit(1);
        }
    }
}

fn kill(ops: OciOperations<impl StorageEngine>, signal: i32) {
    match ops.kill(signal) {
        Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            exit(1);
        }
    }
}

fn delete(ops: OciOperations<impl StorageEngine>) {
    ops.delete();
}
