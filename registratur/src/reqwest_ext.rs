use anyhow::{anyhow, Result};
use futures::stream::TryStreamExt;
use reqwest::Response;
use ring::digest::{self, SHA256};

#[async_trait::async_trait]
pub trait ReqwestResponseExt {
    /// Provides a facility to report the download progress
    /// and validate that the downloaded content matches
    /// it's hash.
    async fn read(
        self,
        mut f: Option<impl FnMut(usize) + Send + 'static>,
        digest: Option<&str>,
    ) -> Result<Vec<u8>>;
}

#[async_trait::async_trait]
impl ReqwestResponseExt for Response {
    async fn read(
        self,
        mut f: Option<impl FnMut(usize) + Send + 'static>,
        digest: Option<&str>,
    ) -> Result<Vec<u8>> {
        let result = self
            .bytes_stream()
            .try_fold(vec![], move |mut agg, bytes| {
                /* https://github.com/tokio-rs/bytes/issues/ */
                agg.extend(bytes);
                f.as_mut().map(|x| x(agg.len()));
                futures::future::ok(agg)
            })
            .await?;

        let res = digest::digest(&SHA256, &result);

        if &digest.unwrap()[7..] != hex::encode(&res) {
            Err(anyhow!("Content hash mismatch."))
        } else {
            Ok(result)
        }
    }
}
