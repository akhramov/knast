use std::process::exit;

use clap::{load_yaml, App, ArgMatches};
use knast::operations::OciOperations;
use storage::SledStorage;

fn main() {
    let yaml = load_yaml!("runc.yaml");
    let matches = App::from(yaml).get_matches();

    if let Some(matches) = matches.subcommand_matches("state") {
        return state(matches);
    }
    if let Some(matches) = matches.subcommand_matches("create") {
        return create(matches);
    }
    if let Some(matches) = matches.subcommand_matches("start") {
        return start(matches);
    }
    if let Some(matches) = matches.subcommand_matches("kill") {
        return kill(matches);
    }
    if let Some(matches) = matches.subcommand_matches("delete") {
        return delete(matches);
    }
}

fn state(args: &ArgMatches) {
    let container_id = args.value_of("ID").unwrap();
    let storage = SledStorage::new("~/").unwrap();
    let ops = OciOperations::new(&storage, container_id).unwrap();

    match ops.state() {
        Ok(result) => println!("{}", result),
        Err(error) => {
            println!("{}", error);
            exit(1);
        }
    }
}

fn create(args: &ArgMatches) {
    let container_id = args.value_of("ID").unwrap();
    let bundle = args.value_of("BUNDLE").unwrap();
    let storage = SledStorage::new("~/").unwrap();
    let ops = OciOperations::new(&storage, container_id).unwrap();

    match ops.create(bundle) {
        Ok(_) => println!("Created container {}", container_id),
        Err(error) => {
            println!("{}", error);
            exit(1);
        }
    }
}

fn start(args: &ArgMatches) {
    let container_id = args.value_of("ID").unwrap();
    let storage = SledStorage::new("~/").unwrap();
    let ops = OciOperations::new(&storage, container_id).unwrap();

    match ops.start() {
        Ok(_) => println!("Started container {}", container_id),
        Err(error) => {
            println!("{}", error);
            exit(1);
        }
    }
}

fn kill(args: &ArgMatches) {
    let container_id = args.value_of("ID").unwrap();
    let signal = args.value_of("SIGNAL").unwrap().parse().unwrap();
    let storage = SledStorage::new("~/").unwrap();
    let ops = OciOperations::new(&storage, container_id).unwrap();

    match ops.kill(signal) {
        Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            exit(1);
        }
    }
}

fn delete(args: &ArgMatches) {
    let container_id = args.value_of("ID").unwrap();
    let storage = SledStorage::new("~/").unwrap();
    let ops = OciOperations::new(&storage, container_id).unwrap();

    match ops.delete() {
        Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            exit(1);
        }
    }
}
