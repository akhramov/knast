use anyhow::Error;
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use crate::reqwest_ext::ReqwestResponseExt;
use crate::v2::client::Client;

type Empty = HashMap<(), ()>;

/// Diverges from OCI spec.
/// OCI media type is
/// "application/vnd.oci.image.config.v1+json"
const MEDIA_TYPE: &str = "application/vnd.oci.image.config.v1+json";

/// Represents [OCI Image Configuration](https://git.io/Jfv42)
#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub created: Option<DateTime<Local>>,
    pub author: Option<String>,
    pub architecture: String,
    pub os: String,
    pub config: Option<Container>,
    pub rootfs: RootFs,
    pub history: Vec<HistoryItem>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Container {
    #[serde(rename = "User")]
    pub user: Option<String>,
    #[serde(rename = "ExposedPorts")]
    pub exposed_ports: Option<HashMap<String, Empty>>,
    #[serde(rename = "Env")]
    pub env: Option<Vec<String>>,
    #[serde(rename = "Entrypoint")]
    pub entrypoint: Option<Vec<String>>,
    #[serde(rename = "Cmd")]
    pub cmd: Option<Vec<String>>,
    #[serde(rename = "Volumes")]
    pub volumes: Option<HashMap<String, Empty>>,
    #[serde(rename = "WorkingDir")]
    pub working_dir: String,
    #[serde(rename = "labels")]
    pub labels: Option<HashMap<String, String>>,
    #[serde(rename = "StopSignal")]
    pub stop_signal: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RootFs {
    pub r#type: String,
    pub diff_ids: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HistoryItem {
    pub created: Option<DateTime<Local>>,
    pub author: Option<String>,
    pub created_by: Option<String>,
    pub comment: Option<String>,
    pub empty_layer: Option<bool>,
}

impl Config {
    /// Pull an OCI Image config from a registry
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use registratur::v2::client::Client;
    /// use registratur::v2::domain::config::Config;
    ///
    /// let ref client = Client::build("registry-1.docker.io").unwrap();
    ///
    /// async {
    ///     let config = Config::pull(
    ///         client,
    ///         "library/nginx",
    ///         "sha256:abde"
    ///     ).await;
    ///     println!("Got Config: {:?}", config.unwrap());
    /// };
    /// ```
    #[fehler::throws]
    pub async fn pull(client: &Client<'_>, name: &str, digest: &str) -> Self {
        use reqwest::{header, Method};

        let path = format!("/v2/{}/blobs/{}", name, digest);

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
    use chrono::prelude::*;
    use serde_json;

    use super::Config;

    #[test]
    fn test_deserialization() {
        let fixture = test_helpers::fixture!("config.json");

        let config: Config = serde_json::from_str(fixture)
            .expect("failed to deserialize config");

        assert_eq!(config.created.unwrap().weekday(), Weekday::Sat);

        let volumes_map = config.config.unwrap().volumes.unwrap();
        let mut volumes = volumes_map.keys().collect::<Vec<&String>>();

        volumes.sort();

        assert_eq!(volumes, ["/var/job-result-data", "/var/log/my-app-logs"]);
    }
}
