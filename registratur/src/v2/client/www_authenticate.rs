use anyhow::{anyhow, Error};

use nom::{
    bytes::complete::take_while,
    character::complete::char,
    sequence::{delimited, preceded, tuple},
    IResult,
};

const QUOTE: char = '"';

/// Represents WWW-Authenticate header
/// Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:library/nginx:pull"
#[derive(Debug)]
pub struct WwwAuthenticate<'a> {
    pub realm: &'a str,
    pub service: &'a str,
    pub scope: &'a str,
}

impl<'a> WwwAuthenticate<'a> {
    pub fn parse(input: &'a str) -> Result<Self, Error> {
        tuple((term, term, term))(input)
            .map(|(_, (realm, service, scope))| {
                Self {
                    realm,
                    service,
                    scope,
                }
            })
            .map_err(|err| {
                anyhow!("Failed to parse WWW-Authenticate header: {:?}", err)
            })
    }
}

pub fn term(input: &str) -> IResult<&str, &str> {
    delimited(preceded(string, char(QUOTE)), string, char(QUOTE))(input)
}

fn string(input: &str) -> IResult<&str, &str> {
    take_while(|c| c != QUOTE)(input)
}

#[cfg(test)]
mod test {
    #[test]
    fn test_parsing() {
        let header = test_helpers::fixture!("www_authenticate");
        let parsed_header = super::WwwAuthenticate::parse(header)
            .expect("Failed to parse WwwAuthenticate header");

        assert_eq!(parsed_header.realm, "https://auth.docker.io/token");
        assert_eq!(parsed_header.service, "registry.docker.io");
        assert_eq!(parsed_header.scope, "repository:library/nginx:pull");
    }
}
