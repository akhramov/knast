use anyhow::Error;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use super::descriptor::Descriptor;
use crate::reqwest_ext::ReqwestResponseExt;
use crate::v2::client::Client;

/// Diverges from OCI spec.
/// OCI media type is
/// application/vnd.oci.image.manifest.v1+json
const MEDIA_TYPE: &str =
    "application/vnd.docker.distribution.manifest.v2+json";

/// Represents [OCI Image Manifest](https://git.io/JvptH)
#[derive(Serialize, Deserialize, Debug)]
pub struct Manifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    media_type: Option<String>,
    pub config: Descriptor,
    pub layers: Vec<Descriptor>,
    pub annotations: Option<HashMap<String, String>>,
}

impl Manifest {
    /// Pull an OCI manifest from a registry
    /// This function operates +only+ on digests.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use registratur::v2::client::Client;
    /// use registratur::v2::domain::manifest::Manifest;
    ///
    /// let ref client = Client::build("registry-1.docker.io").unwrap();
    ///
    /// async {
    ///     let manifest =
    ///         Manifest::pull(client, "library/nginx", "sha256:6036ab").await;
    ///     println!("Got Manifest: {:?}", manifest.unwrap());
    /// };
    /// ```
    #[fehler::throws]
    pub async fn pull(client: &Client<'_>, name: &str, digest: &str) -> Self {
        use reqwest::{header, Method};

        let path = format!("/v2/{}/manifests/{}", name, digest);

        let result = client
            .request(Method::GET, &path, |request| {
                request.header(header::ACCEPT, MEDIA_TYPE)
            })
            .await?
            .read(None::<fn(usize)>, Some(digest))
            .await?;

        serde_json::from_slice(&result)?
    }
}

#[cfg(test)]
mod tests {
    use serde_json;

    use super::Manifest;

    #[test]
    fn test_deserialization() {
        let fixture = test_helpers::fixture!("manifest.json");

        let manifest: Manifest = serde_json::from_str(fixture)
            .expect("failed to deserialize manifest");

        assert_eq!(manifest.layers[2].size, 73109);
        assert_eq!(
            manifest
                .annotations
                .and_then(|mut x| x.remove("com.example.key1")),
            Some(String::from("value1"))
        );
    }
}
