use anyhow::Error;
use reqwest;
use reqwest::Method;
use url::Url;

mod www_authenticate;

const USER_AGENT: &str =
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// Distribution client implementation, according to
/// [spec](https://docs.docker.com/registry/spec/auth/jwt)
pub struct Client<'a> {
    registry_url: &'a str,
    client: reqwest::Client,
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
}

impl<'a> Client<'a> {
    /// Builds an OCI registry API client
    #[fehler::throws]
    pub fn build(registry_url: &'a str) -> Self {
        let client =
            reqwest::Client::builder().user_agent(USER_AGENT).build()?;

        Self {
            registry_url,
            client,
        }
    }

    /// Performs an authenticated HTTP request against the
    /// registry.
    ///
    /// path will be appended to registry base path. This
    /// function prepares a request and authorizes the
    /// client. Any modifications to the request can
    /// be done via the [`reqwest::RequestBuilder`]
    /// parameter of the `f` closure.
    ///
    /// # Example
    ///
    /// Fetch nginx manifest from docker registry.
    ///
    /// ```rust,no_run
    /// use reqwest::{header, Method};
    ///
    /// use registratur::v2::client::Client;
    /// use registratur::v2::domain::manifest::Manifest;
    ///
    /// let client = Client::build("https://registry-1.docker.io").unwrap();
    ///
    /// let path = "/v2/library/nginx/manifests/latest";
    /// let media = "application/vnd.docker.distribution.manifest.v2+json";
    ///
    /// async {
    ///     let response: Manifest = client.request(Method::GET, path, |client| {
    ///         client.header(header::ACCEPT, media)
    ///     }).await.unwrap().json().await.unwrap();
    ///
    ///     println!("Got Manifest: {:?}", response);
    /// };
    /// ```
    #[fehler::throws]
    pub async fn request<F>(
        &self,
        method: Method,
        path: &str,
        f: F,
    ) -> reqwest::Response
    where
        F: FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    {
        let base = Url::parse(self.registry_url)?;
        let url = base.join(&path)?;

        log::debug!("{} {}", &method, url);

        let builder = self.client.request(method, url.clone());
        let builder = f(builder);

        let token = self.authenticate(url).await?;

        builder.bearer_auth(token).send().await?
    }

    #[fehler::throws]
    async fn authenticate(&self, url: Url) -> String {
        // TODO: test against non-docker registries
        // TODO: login / password auth.
        let challenge_response = self.client.head(url).send().await?;

        let headers = challenge_response.headers();

        let challenge = headers.get("www-authenticate").unwrap().to_str()?;

        let challenge = www_authenticate::WwwAuthenticate::parse(challenge)?;

        let query =
            [("scope", challenge.scope), ("service", challenge.service)];

        self.client
            .get(challenge.realm)
            .query(&query)
            .send()
            .await?
            .json::<TokenResponse>()
            .await?
            .access_token
    }
}

#[cfg(test)]
mod test {
    use super::Client;
    use crate::v2::domain::{
        config::Config,
        layer::Layer,
        manifest::Manifest,
        manifest_index::{ManifestIndex, Platform},
    };

    #[tokio::test]
    async fn docker_registry_integration() {
        #[cfg(feature = "integration_testing")]
        let (url, _mocks) = ("https://registry-1.docker.io", ());
        #[cfg(not(feature = "integration_testing"))]
        let (url, _mocks) = test_helpers::mock_server!("basic.yml");

        let image = "library/nginx";

        /* 0. Create a client. */
        let client =
            Client::build(&url).expect("Failed to build registry client");

        /* 0. Fetch manifest index. */
        let index = ManifestIndex::pull(&client, image, "latest")
            .await
            .expect("Failed to fetch manifest");

        let manifest_digest = &index
            .manifests
            .iter()
            .find(|x| match &x.platform {
                Some(Platform {
                    architecture, os, ..
                }) => architecture == "amd64" && os == "linux",
                None => false,
            })
            .expect("Unable to find appropriate manifest in index")
            .descriptor
            .digest;

        /* 2. Fetch the manifest */
        let manifest =
            Manifest::pull(&client, "library/nginx", manifest_digest)
                .await
                .expect("Failed to fetch manifest");

        /* 3. Fetch the config */
        let config =
            Config::pull(&client, "library/nginx", &manifest.config.digest)
                .await
                .expect("Failed to fetch config");

        assert_eq!(
            config.config.unwrap().cmd.unwrap(),
            vec!["nginx", "-g", "daemon off;"]
        );

        /* 4. Fetch layers */
        let manifested_layer = &manifest.layers[0];
        let size = manifested_layer.size;

        let future = Layer::pull(
            &client,
            "library/nginx",
            &manifested_layer.digest,
            move |x| log::info!("Downloaded {} of {}", x, size),
        );

        let actual_layer = future.await.expect("Failed to fetch layer");

        assert_eq!(manifested_layer.size, actual_layer.len());
    }

    #[tokio::test]
    async fn test_hashsum_mismatch() {
        let (url, _mocks) = test_helpers::mock_server!("basic.yml");

        /* 0. Create a client. */
        let client =
            Client::build(&url).expect("Failed to build registry client");

        let err =
            Manifest::pull(&client, "library/nginx", "this is simply wrong")
                .await
                .unwrap_err();

        let error: &dyn std::error::Error = err.as_ref();
        assert_eq!("Content hash mismatch.", error.to_string());
    }
}
