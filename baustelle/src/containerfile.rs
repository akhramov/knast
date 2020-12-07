mod instruction;

use std::io::Read;
use std::path::PathBuf;

use anyhow::Error;
use dockerfile_parser::Dockerfile as Containerfile;

use registratur::v2::client::Client;


use crate::{
    fetcher::Fetcher,
    storage::Storage,
};

pub struct Builder<'a> {
    fetcher: Fetcher<'a>,
    storage_folder: PathBuf,
}

impl<'a> Builder<'a> {
    #[fehler::throws]
    pub fn new(
        registry_url: &'a str,
        architecture: String,
        os: Vec<String>,
        storage: &'a Storage,
    ) -> Self {
        let client = Client::build(registry_url)?;
        let fetcher = Fetcher::new(storage, client, architecture, os);

        Self {
            fetcher,
            storage_folder: storage.folder(),
        }
    }

    #[fehler::throws]
    pub fn interpet(&self, file: impl Read) {
        let containerfile = Containerfile::from_reader(file)?;

        for stage in containerfile.iter_stages() {
            for instruction in &stage.instructions {
                self::instruction::execute(&self, instruction)?;
            }
        }
    }

    pub fn fetcher(&self) -> &Fetcher<'a> {
        &self.fetcher
    }
}
