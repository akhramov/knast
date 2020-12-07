// Fetch & unpack a centos image.
extern crate baustelle;
extern crate futures;
#[macro_use]
extern crate log;
extern crate registratur;
extern crate tokio;

use std::path::Path;

use baustelle::{
    fetcher::{Fetcher, LayerDownloadStatus::*},
    storage::Storage,
    unpacker::Unpacker,
};
use futures::{future, stream::StreamExt};
use registratur::v2::client::Client;
use tokio::runtime::Runtime;

async fn fetch_image(storage: &Storage) -> String {
    let client = Client::build("https://registry-1.docker.io")
        .expect("failed to build the client");

    let architecture = "amd64";
    let os = vec!["linux".into(), "freebsd".into()];
    let fetcher = Fetcher::new(&storage, client, architecture.into(), os);
    let (tx, rx) = futures::channel::mpsc::channel(1);

    let digest_fut = fetcher.fetch("centos", "7.8.2003", tx);
    let updates_fut = rx.collect::<Vec<_>>();

    let (digest, updates) = future::join(digest_fut, updates_fut).await;

    updates.iter().for_each(|x| {
        if let InProgress(name, count, total) = x {
            info!("{} downloaded {} of {}", name, count, total);
        }
    });

    digest.unwrap()
}

fn main() {
    let mut rt = Runtime::new().unwrap();

    let storage = Storage::new("./").expect("Unable to initialize cache");

    let digest = rt.block_on(fetch_image(&storage));

    let unpacker = Unpacker::new(&storage, Path::new("./centos"));

    unpacker
        .unpack(digest.clone())
        .expect("Failed to unpack the archive");

    info!("Fetched an image. Its digest is {:?}", digest);
}
