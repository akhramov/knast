use serde::{Deserialize, Serialize};

/// Represents [OCI Content Descriptor](https://git.io/JvpqR)
#[derive(Serialize, Deserialize, Debug)]
pub struct Descriptor {
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub digest: String,
    pub size: usize,
    pub urls: Option<Vec<String>>,
}
