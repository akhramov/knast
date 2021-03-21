use std::{convert::AsRef, fs::File, io::BufReader, path::Path};

use anyhow::Error;
use baustelle::runtime_config::RuntimeConfig;

#[fehler::throws]
pub fn perform(id: String, path: impl AsRef<Path>) {
    let config = runtime_config(&path)?;
}

#[fehler::throws]
fn runtime_config(path: impl AsRef<Path>) -> RuntimeConfig {
    let config = File::open(path.as_ref().join("config.json"))?;
    let reader = BufReader::new(config);

    serde_json::from_reader(reader)?
}
