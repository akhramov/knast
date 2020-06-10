use anyhow::Error;

use crate::reqwest_ext::ReqwestResponseExt;
use crate::v2::client::Client;

const MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar+gzip";

/// Represents [Image Layer Filesystem Changeset](https://git.io/JfkAk)
pub struct Layer;

impl Layer {
    /// Pull an OCI Layer FS Changesetfrom a registry
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use registratur::v2::client::Client;
    /// use registratur::v2::domain::layer::Layer;
    ///
    /// let ref client = Client::build("registry-1.docker.io").unwrap();
    ///
    /// async {
    ///     let config = Layer::pull(
    ///         client,
    ///         "library/nginx",
    ///         "sha256:abde",
    ///         |_| {},
    ///     ).await;
    ///     println!("Got Layer: {:?}", config.unwrap());
    /// };
    /// ```
    #[fehler::throws]
    pub async fn pull<F>(
        client: &Client<'_>,
        name: &str,
        digest: &str,
        progress_callback: F,
    ) -> Vec<u8>
    where
        F: FnMut(usize) + 'static + Send,
    {
        use reqwest::{header, Method};

        let path = format!("/v2/{}/blobs/{}", name, digest);

        let result = &*client
            .request(Method::GET, &path, |request| {
                request.header(header::ACCEPT, MEDIA_TYPE)
            })
            .await?
            .read(Some(progress_callback), Some(&digest))
            .await?;

        result.into()
    }
}
