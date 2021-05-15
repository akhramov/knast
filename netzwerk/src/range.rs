use std::{
    collections::BinaryHeap, convert::AsRef, convert::TryFrom,
    iter::FromIterator, net::Ipv4Addr,
};

use anyhow::Error;
use ipnetwork::Ipv4Network;

#[fehler::throws]
pub fn range(range: impl AsRef<str>) -> BinaryHeap<Ipv4Addr> {
    BinaryHeap::from_iter(&Ipv4Network::try_from(range.as_ref())?)
}

#[fehler::throws]
pub fn broadcast(range: impl AsRef<str>) -> Ipv4Addr {
    Ipv4Network::try_from(range.as_ref())?
        .broadcast()
}

#[fehler::throws]
pub fn mask(range: impl AsRef<str>) -> Ipv4Addr {
    Ipv4Network::try_from(range.as_ref())?
        .mask()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range() {
        let mut result = range("172.24.0.2/16").unwrap();

        assert_eq!(result.len(), 256 * 256);
        assert_eq!("172.24.255.255", result.pop().unwrap().to_string());
        assert_eq!("172.24.255.254", result.pop().unwrap().to_string());
    }

    #[test]
    fn test_broadcast() {
        let result = broadcast("172.24.0.2/16").unwrap();

        assert_eq!("172.24.255.255", result.to_string());
    }

    #[test]
    fn test_mask() {
        let result = mask("172.24.0.2/16").unwrap();

        assert_eq!("255.255.0.0", result.to_string());
    }
}
