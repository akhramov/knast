// Fetch & unpack a centos image.
extern crate baustelle;
extern crate futures;
#[macro_use]
extern crate log;
extern crate registratur;
extern crate tokio;

use baustelle::{Builder, EvaluationUpdate, LayerDownloadStatus};

const CONTAINERFILE: &[u8] = r#"
FROM centos:latest

ENV FOO=/bar
WORKDIR ${FOO}

CMD /bin/sleep 42
"#.as_bytes();

#[tokio::main]
async fn main() {
    env_logger::init();

    info!("Fetching a centos image");

    let builder = Builder::new("amd64".into(), vec!["linux".into()], "./")
        .expect("Failed to build the image builder");

    builder
        .build("https://registry-1.docker.io", CONTAINERFILE, |x| {
            if let EvaluationUpdate::From(LayerDownloadStatus::InProgress(
                name,
                count,
                total,
            )) = x
            {
                info!("{} downloaded {} of {}", name, count, total);
            }
        })
        .await
        .expect("Failed to build the image");

    info!("Fetched an image");
}
