mod bindings;

use anyhow::Error;

use bindings::{rtmsg, Operation};

/// Add default route
///
/// This operation may fail for several reasons, such as
/// unreacheable network, default route already exists and
/// so on.
///
/// # Examples
/// add net default 172.23.0.1
///
/// ```rust,no_run
/// use netzwerk::route;
///
/// route::add_default("172.23.0.1")
///     .expect("Add net default failed.");
/// ```
#[fehler::throws]
pub fn add_default(address: &str) {
    rtmsg(Operation::Add, Some(address))?;
}

/// Delete default route
///
/// # Examples
/// delete net default
///
/// ```rust,no_run
/// use netzwerk::route;
///
/// route::delete_default()
///     .expect("Delete net default failed");
/// ```
#[fehler::throws]
pub fn delete_default() {
    rtmsg(Operation::Delete, None)?;
}

#[cfg(test)]
mod test {
    use super::*;
    use std::process::Command;

    #[test_helpers::jailed_test]
    fn test_add_default() {
        setup_lo();
        add_default("127.0.0.1").expect("failed to add default route");

        let content = routing_tables_content()
            .expect("(netstat) failed to get routing tables content");

        assert!(content.contains("default            127.0.0.1"));
    }

    #[test_helpers::jailed_test]
    fn test_delete_default() {
        setup_lo();
        add_default("127.0.0.1").expect("failed to add default route");
        delete_default().expect("failed to delete default route");

        let content = routing_tables_content()
            .expect("(netstat) failed to get routing tables content");

        assert!(!content.contains("default            127.0.0.1"));
    }

    #[fehler::throws]
    fn routing_tables_content() -> String {
        String::from_utf8(Command::new("netstat").arg("-rn").output()?.stdout)?
    }

    fn setup_lo() {
        use crate::interface::Interface;

        Interface::new("lo0")
            .expect("failed to get iface socket")
            .address("127.0.0.1", "127.255.255.255", "255.0.0.0")
            .expect("failed to assign expected address");
    }
}
