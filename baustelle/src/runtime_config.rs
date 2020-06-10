use std::collections::HashMap;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct RuntimeConfig {
    #[serde(rename = "ociVersion")]
    pub oci_version: String,
    pub root: Option<Root>,
    pub mounts: Option<Vec<Mount>>,
    pub process: Option<Process>,
    pub hooks: Option<Hooks>,
    pub annotations: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
pub struct Root {
    pub path: String,
    pub readonly: Option<bool>,
}

#[derive(Deserialize)]
pub struct Mount {
    pub destination: String,
    pub source: Option<String>,
    pub options: Option<Vec<String>>,
    pub r#type: Option<String>,
}

#[derive(Deserialize)]
pub struct Process {
    pub terminal: Option<bool>,
    #[serde(rename = "consoleSize")]
    pub console_size: Option<ConsoleSize>,
    pub cwd: String,
    pub env: Option<String>,
    pub args: Option<String>,
    pub rlimits: Option<Vec<Rlimit>>,
    pub user: User,
    pub hostname: Option<String>,
    /* commandLine omitted */
}

#[derive(Deserialize)]
pub struct ConsoleSize {
    pub height: u32,
    pub width: u32,
}

#[derive(Deserialize)]
pub struct Rlimit {
    pub r#type: String,
    pub soft: String,
    pub hard: String,
}

#[derive(Deserialize)]
pub struct User {
    pub uid: u32,
    pub gid: u32,
    pub umask: Option<u32>,
    #[serde(rename = "additionalGids")]
    pub additional_gids: Option<Vec<u32>>,
}

#[derive(Deserialize)]
pub struct Hooks {
    prestart: Option<Vec<Hook>>,
    #[serde(rename = "createRuntime")]
    create_runtime: Option<Vec<Hook>>,
    #[serde(rename = "createContainer")]
    create_container: Option<Vec<Hook>>,
    #[serde(rename = "startContainer")]
    start_container: Option<Vec<Hook>>,
    poststart: Option<Vec<Hook>>,
    poststop: Option<Vec<Hook>>,
}

#[derive(Deserialize)]
pub struct Hook {
    path: String,
    args: Option<Vec<String>>,
    env: Option<Vec<String>>,
    timeout: Option<u32>,
}
