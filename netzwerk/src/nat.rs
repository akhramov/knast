use anyhow::Error;

pub trait Nat {
    fn add(&self, subnet: &str) -> Result<(), Error>;
}
