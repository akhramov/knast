// Fetch & unpack a centos image.
use baustelle::{Builder, EvaluationUpdate, LayerDownloadStatus};
use storage::SledStorage;

#[tokio::main]
async fn main() {
    let home = std::env::var("HOME").unwrap();
    let storage = SledStorage::new(home).unwrap();
    let builder = Builder::new("amd64".into(), vec!["linux".into()], storage)
        .expect("Failed to build the image builder");
    tracing_subscriber::fmt().init();

    let image = std::env::args().nth(1).expect("USAGE: fetch_image IMAGE");
    let containerfile = format!("FROM {}", image);

    let rootfs = builder
        .build("https://registry-1.docker.io", containerfile.as_bytes(), |x| {
            if let EvaluationUpdate::From(LayerDownloadStatus::InProgress(
                name,
                count,
                total,
            )) = x
            {
                tracing::info!("{} downloaded {} of {}", name, count, total);
            }
        })
        .await
        .expect("Failed to build the image");

    tracing::info!("Build a container");
    tracing::info!("Bundle located in {:#?}", rootfs);
}
