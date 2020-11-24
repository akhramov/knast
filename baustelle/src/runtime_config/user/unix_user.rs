use std::convert::AsRef;
use std::fs::File;
use std::path::Path;

use anyhow::Error;
use csv::{Error as CsvError, ReaderBuilder};
use serde::{de::DeserializeOwned, Deserialize, Deserializer};

/// Abstraction over /etc/passwd & /etc/group
pub struct EtcConf<T: DeserializeOwned>(
    Box<dyn Iterator<Item = Result<T, CsvError>>>,
);

#[derive(Deserialize, Debug)]
pub struct EtcPasswdEntry {
    pub username: String,
    _password: String,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Deserialize, Debug)]
pub struct EtcGroupEntry {
    pub groupname: String,
    _password: String,
    pub gid: u32,
    #[serde(deserialize_with = "comma_delimited_string")]
    pub users: Vec<String>,
}

fn comma_delimited_string<'de, D>(
    deserializer: D,
) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer)
        .map(|string| string.split(',').map(String::from).collect())
}

impl<T: DeserializeOwned + 'static> EtcConf<T> {
    #[fehler::throws]
    pub fn new(file: impl AsRef<Path>) -> Self {
        let file = File::open(file)?;
        let csv_reader = ReaderBuilder::new()
            .has_headers(false)
            .delimiter(b':')
            .comment(Some(b'#'))
            .from_reader(file);

        let result = csv_reader.into_deserialize();

        Self(Box::new(result))
    }
}

impl<T: DeserializeOwned> Iterator for EtcConf<T> {
    type Item = Result<T, CsvError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_etc_passwd_enumeration() {
        let path = test_helpers::fixture_path!("unix/happy_path/etc/passwd");
        let passwd: EtcConf<EtcPasswdEntry> = EtcConf::new(path).unwrap();

        let result: Vec<_> = passwd.collect();
        let second_user = &result[1].as_ref().unwrap();

        assert_eq!(second_user.username, "door");
    }

    #[test]
    fn test_etc_group_enumeration() {
        let path = test_helpers::fixture_path!("unix/happy_path/etc/group");
        let groups: EtcConf<EtcGroupEntry> = EtcConf::new(path).unwrap();

        let result: Vec<_> = groups.collect();
        let first_group = result[0].as_ref().unwrap();

        assert_eq!(first_group.users, &["root", "akhramov", "donald_watson"]);
    }

    #[test]
    fn test_malformed_file() {
        let path = test_helpers::fixture_path!("unix/malformed/etc/passwd");
        let groups: EtcConf<EtcGroupEntry> = EtcConf::new(path).unwrap();
        let result: Vec<_> = groups.collect();
        let first_err = result[0].as_ref().unwrap_err();

        assert_eq!(
            test_helpers::fixture!("unix/malformed/etc/malformed_error_msg"),
            format!("{}", first_err)
        );
    }
}
