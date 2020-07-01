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
    use std::process::Command;
    struct DroppableRoute;

    impl DroppableRoute {
        #[fehler::throws(anyhow::Error)]
        fn new() -> Self {
            super::add_default("127.0.0.1")?;

            DroppableRoute
        }
    }

    impl Drop for DroppableRoute {
        fn drop(&mut self) {
            super::delete_default();
        }
    }

    #[test]
    fn test_add_default() {
        let _route =
            DroppableRoute::new().expect("Failed to add default route");

        let netstat_output = Command::new("netstat")
            .arg("-rn")
            .output()
            .expect("Failed to execute netstat");

        let content = String::from_utf8(netstat_output.stdout).unwrap();

        assert!(content.contains("default            127.0.0.1"));
    }

    #[test]
    fn test_delete_default() {
        DroppableRoute::new().expect("Failed to add default route"); // return value dropped

        let netstat_output = Command::new("netstat")
            .arg("-rn")
            .output()
            .expect("Failed to execute netstat");

        let content = String::from_utf8(netstat_output.stdout).unwrap();

        assert!(!content.contains("default            127.0.0.1"));
    }
}
