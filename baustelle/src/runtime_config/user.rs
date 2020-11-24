mod unix_user;

use std::convert::AsRef;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Error, Result};

use nom::{
    branch::alt, bytes::complete::tag, character::complete::alphanumeric1,
    combinator::map_res, sequence::separated_pair, IResult,
};

use serde::de::DeserializeOwned;

use unix_user::{EtcConf, EtcGroupEntry, EtcPasswdEntry};

fn identifier<T>(input: &str) -> IResult<&str, T>
where
    T: FromStr,
{
    map_res(alphanumeric1, FromStr::from_str)(input)
}

fn pair<S, T>(input: &str) -> IResult<&str, (S, T)>
where
    S: FromStr,
    T: FromStr,
{
    separated_pair(identifier, tag(":"), identifier)(input)
}

fn find_entry<T, F>(rootfs: impl AsRef<Path>, predicate: F) -> Result<T>
where
    T: DeserializeOwned + 'static,
    F: Fn(&T) -> bool,
{
    let items = EtcConf::<T>::new(&rootfs)?;

    for maybe_item in items {
        if let Ok(item) = maybe_item {
            if predicate(&item) {
                return Ok(item);
            }
        }
    }

    anyhow::bail!("Entry was not found")
}

fn find_group_by_name(
    rootfs: &Path,
) -> impl Fn(String) -> Result<EtcGroupEntry> {
    let path = Path::new(rootfs).join("etc/group");

    move |groupname| {
        find_entry(&path, |group: &EtcGroupEntry| group.groupname == groupname)
            .context(format!(
                "Group {} was not found in {:?}",
                groupname, path
            ))
    }
}

fn find_user_by_name(
    rootfs: &Path,
) -> impl Fn(String) -> Result<EtcPasswdEntry> {
    let path = Path::new(rootfs).join("etc/passwd");

    move |username| {
        find_entry(&path, |user: &EtcPasswdEntry| user.username == username)
            .context(format!("User {} was not found in {:?}", username, path))
    }
}

fn find_user_by_uid(rootfs: &Path) -> impl Fn(u32) -> Result<EtcPasswdEntry> {
    let path = Path::new(rootfs).join("etc/passwd");

    move |uid| {
        find_entry(&path, |user: &EtcPasswdEntry| user.uid == uid).context(
            format!("User with uid {} was not found in {:?}", uid, path),
        )
    }
}

/// Parses user string to retrieve uid / gid pair
///
/// If user string doesn't contain all required information,
/// then the info is looked up in the container's root
/// filesystem. Namely, in `/etc/passwd` and `/etc/group`
/// files.
///
/// Adhering to Linux specification, these types of user
/// strings are valid: `user`, `uid`, `user:group`,
/// `uid:gid`, `uid:group`, `user:gid`. In practice, docker
/// registry may serve the config with the empty (`""`) user
/// string. This case is to be handled outside the scope of
/// this function.
///
/// ```
#[fehler::throws]
pub fn parse(user: String, rootfs: &Path) -> (u32, u32) {
    let uid_gid = pair::<u32, u32>;

    let uid_group = map_res(pair, |(uid, group)| -> Result<(u32, u32)> {
        Ok((uid, find_group_by_name(rootfs)(group)?.gid))
    });

    let username =
        map_res(identifier, |username: String| -> Result<(u32, u32)> {
            let user = find_user_by_name(rootfs)(username)?;

            Ok((user.uid, user.gid))
        });

    let uid = map_res(identifier, |uid: u32| -> Result<(u32, u32)> {
        let user = find_user_by_uid(rootfs)(uid)?;

        Ok((user.uid, user.gid))
    });

    let user_group =
        map_res(pair, |(username, group)| -> Result<(u32, u32)> {
            let user = find_user_by_name(rootfs)(username)?;
            let group = find_group_by_name(rootfs)(group)?;

            Ok((user.uid, group.gid))
        });

    let user_gid = map_res(pair, |(username, gid)| -> Result<(u32, u32)> {
        let user = find_user_by_name(rootfs)(username)?;

        Ok((user.uid, gid))
    });

    match alt((uid_gid, uid_group, user_group, user_gid, username, uid))(&user)
    {
        Ok((_, res)) => res,
        Err(err) => {
            anyhow::bail!("Failed to parse user config string: {}", err)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    macro_rules! do_parse {
        ($str:expr) => {{
            let user = $str.into();
            let path = test_helpers::fixture_path!("unix/happy_path");

            parse(user, path).unwrap()
        }};
    }

    #[test]
    fn test_uid_gid_parsing() {
        assert_eq!(do_parse!("1001:1002"), (1001, 1002));
    }

    #[test]
    fn test_resolve_gid_from_name() {
        assert_eq!(do_parse!("1337:tests"), (1337, 977));
    }

    #[test]
    fn test_only_username_supplied() {
        assert_eq!(do_parse!("akhramov"), (1001, 1001));
    }

    #[test]
    fn test_only_uid_supplied() {
        assert_eq!(do_parse!("977"), (977, 977));
    }

    #[test]
    fn test_username_groupname_supplied() {
        assert_eq!(do_parse!("tests:games"), (977, 13));
    }

    #[test]
    fn test_username_gid_supplied() {
        assert_eq!(do_parse!("tests:13"), (977, 13));
    }

    #[test]
    fn test_invalid_username() {
        let user = "testsa:13".into();
        let path = test_helpers::fixture_path!("unix/happy_path");

        let error = r#"Parsing Error: ("testsa:13", MapRes)"#;

        assert!(
            format!("{:?}", parse(user, path).unwrap_err()).contains(error)
        );
    }

    #[test]
    fn test_malformed_file() {
        let user = "tests:13".into();
        let path = test_helpers::fixture_path!("unix/malformed");

        assert!(parse(user, path).is_err());
    }

    #[test]
    fn test_nonexistent_file() {
        let user = "tests:13".into();
        let path = test_helpers::fixture_path!("unix/this_is_sad");

        assert!(parse(user, path).is_err());
    }
}
