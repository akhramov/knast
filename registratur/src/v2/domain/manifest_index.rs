use anyhow::Error;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use super::descriptor::Descriptor;
use crate::v2::client::Client;

/// Diverges from OCI spec.
/// OCI media type is
/// application/vnd.oci.image.index.v1+json
const MEDIA_TYPE: &str =
    "application/vnd.docker.distribution.manifest.list.v2+json";

/// Represents [OCI Image Manifest Index](https://git.io/JfLGL)
#[derive(Serialize, Deserialize, Debug)]
pub struct ManifestIndex {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    pub manifests: Vec<Manifest>,
    pub annotations: Option<HashMap<String, String>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Manifest {
    #[serde(flatten)]
    pub descriptor: Descriptor,
    pub platform: Option<Platform>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Platform {
    pub architecture: String,
    pub os: String,
    #[serde(rename = "os.version")]
    pub os_version: Option<String>,
    #[serde(rename = "os.features")]
    pub os_features: Option<Vec<String>>,
    pub variant: Option<String>,
}

impl ManifestIndex {
    /// Pull an OCI manifest from a registry
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use registratur::v2::client::Client;
    /// use registratur::v2::domain::manifest_index::ManifestIndex;
    ///
    /// let ref client = Client::build("registry-1.docker.io").unwrap();
    ///
    /// async {
    ///     let index = ManifestIndex::pull(client, "library/nginx", "latest");
    ///     println!("Got Manifest Index: {:?}", index.await.unwrap());
    /// };
    /// ```
    #[fehler::throws]
    pub async fn pull(client: &Client<'_>, name: &str, tag: &str) -> Self {
        use reqwest::{header, Method};

        log::debug!("Pulling Manifest Index for {}:{}", name, tag);

        let path = format!("/v2/{}/manifests/{}", name, tag);

        client
            .request(Method::GET, &path, |request| {
                request.header(header::ACCEPT, MEDIA_TYPE)
            })
            .await?
            .json()
            .await?
    }
}

#[cfg(test)]
mod tests {
    use serde_json;

    use super::ManifestIndex;

    #[test]
    fn test_deserialization() {
        let fixture = test_helpers::fixture!("manifest_index.json");

        let index: ManifestIndex = serde_json::from_str(fixture)
            .expect("failed to deserialize index");

        let platform = index.manifests[1].platform.as_ref();

        assert_eq!(platform.unwrap().architecture, "amd64");
    }
}
