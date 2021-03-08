mod bindings;

use std::ffi::CStr;
use std::mem;

use anyhow::Error;
use libc::{AF_INET, SOCK_DGRAM};

use crate::common_bindings::Socket;
use bindings::{
    bridge_addm, bridge_delm, create_interface, destroy_interface, ifreq,
    jail_interface, rename_interface, set_interface_address,
};

/// A structure incapsulating network interface requests
///
/// This one can be thought as a wrapper around ifconfig:
/// the one stop for interface creation, destruction, inet
/// addresses assignment, bridge members manipulation.
///
/// # Examples
///
/// Create if_bridge(4) interface.
///
/// ```rust,no_run
/// use netzwerk::interface::Interface;
///
/// Interface::new("bridge")
///     .expect("Failed to create iface socket")
///     .create()
///     .expect("Failed to create interface")
///     .name("bruecke")
///     .expect("Failed to rename interface");
/// ```
pub struct Interface {
    request: ifreq,
    socket: Socket,
}

impl Interface {
    /// Initialize `Interface` structure.
    ///
    /// The structure consists of an `ifreq` structure used
    /// for interface ioctls and a corresponding socket.
    /// This call may fail if system fails to allocate the
    /// socket.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use netzwerk::interface::Interface;
    ///
    /// Interface::new("bridge")
    ///     .expect("Failed to create iface socket");
    /// ```
    #[fehler::throws]
    pub fn new(iface: &str) -> Self {
        let socket = Socket::new(AF_INET, SOCK_DGRAM)?;
        let mut request: ifreq = unsafe { mem::zeroed() };

        request.ifr_name[0..iface.len()].copy_from_slice(iface.as_bytes());

        Self { request, socket }
    }

    /// Create an interface
    ///
    /// This call fails if corresponding kernel does not
    /// support this type of interface. For example, if
    /// GENERIC kernel is used, one should load if_bridge.ko
    /// before attempting to create bridge interfaces.
    ///
    /// # Examples
    /// Create if_bridge(4) interface
    ///
    /// ```rust,no_run
    /// use netzwerk::interface::Interface;
    ///
    /// Interface::new("bridge")
    ///     .expect("Failed to create iface socket")
    ///     .create()
    ///     .expect("Failed to create interface");
    /// ```
    #[fehler::throws]
    pub fn create(self) -> Self {
        create_interface(&self.socket, &self.request)?;

        self
    }

    /// Rename the interface
    ///
    /// # Examples
    /// Create if_bridge(4) interface and rename it to
    /// "werft0"
    ///
    /// ```rust,no_run
    /// use netzwerk::interface::Interface;
    ///
    /// Interface::new("bridge")
    ///     .expect("Failed to create iface socket")
    ///     .create()
    ///     .expect("Failed to create interface")
    ///     .name("werft0")
    ///     .expect("Failed to rename interface");
    /// ```
    #[fehler::throws]
    pub fn name(mut self, name: &str) -> Self {
        rename_interface(&self.socket, &mut self.request, name)?;

        self
    }

    /// Get interface's name
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use netzwerk::interface::Interface;
    ///
    /// Interface::new("bridge")
    ///     .expect("Failed to create iface socket")
    ///     .create()
    ///     .expect("Failed to create interface")
    ///     .get_name()
    ///     .expect("Failed to rename interface");
    /// ```
    #[fehler::throws]
    pub fn get_name(&self) -> &str {
        let cstr =
            unsafe { CStr::from_ptr(self.request.ifr_name.as_ptr() as _) };

        cstr.to_str()?
    }

    /// Set inet address, broadcast address & netmask
    ///
    /// # Examples
    /// Create if_bridge(4) interface and set its address to
    /// 172.24.0.1/24
    ///
    /// ```rust,no_run
    /// use netzwerk::interface::Interface;
    ///
    /// Interface::new("bridge")
    ///     .expect("Failed to create iface socket")
    ///     .create()
    ///     .expect("Failed to create interface")
    ///     .address("172.24.0.1", "172.24.0.255", "255.255.255.0")
    ///     .expect("Failed to assign inet address");
    /// ```
    #[fehler::throws]
    pub fn address(self, addr: &str, broadcast: &str, mask: &str) -> Self {
        set_interface_address(
            &self.socket,
            &self.request.ifr_name,
            addr,
            broadcast,
            mask,
        )?;

        self
    }

    /// Put interface into the jail
    ///
    /// This method consumes self, since interface is moved
    /// into the jail.
    ///
    /// # Examples
    /// Create if_bridge(4) interface and put it into the
    /// jail with id = 2
    ///
    /// ```rust,no_run
    /// use netzwerk::interface::Interface;
    ///
    /// Interface::new("bridge")
    ///     .expect("Failed to create iface socket")
    ///     .create()
    ///     .expect("Failed to create interface")
    ///     .vnet(2)
    ///     .expect("Failed to assign inet vnet");
    /// ```
    #[fehler::throws]
    pub fn vnet(mut self, jid: i32) {
        jail_interface(&self.socket, &mut self.request, jid)?;
    }

    // TODO: should be in its own struct?
    /// Add bridge member(s)
    ///
    /// # Examples
    /// Create if_bridge(4) interface and add epair0b,
    /// epair1b as its members
    ///
    /// ```rust,no_run
    /// use netzwerk::interface::Interface;
    ///
    /// Interface::new("bridge")
    ///     .expect("Failed to create iface socket")
    ///     .create()
    ///     .expect("Failed to create interface")
    ///     .bridge_addm(&["epair0b", "epair1b"])
    ///     .expect("Failed to assign inet address");
    /// ```
    #[fehler::throws]
    pub fn bridge_addm(&self, members: &[&str]) {
        members
            .iter()
            .map(|member| {
                bridge_addm(&self.socket, &self.request.ifr_name, member)
            })
            .collect::<Result<_, _>>()?;
    }

    // TODO: should be in its own struct?
    /// Remove bridge member(s)
    ///
    /// # Examples
    /// Remove interfaces epair0b, epair1b from bridge
    /// bridge0.
    ///
    /// ```rust,no_run
    /// use netzwerk::interface::Interface;
    ///
    /// Interface::new("bridge0")
    ///     .expect("Failed to create iface socket")
    ///     .bridge_delm(&["epair0b", "epair1b"])
    ///     .expect("Failed to assign inet address");
    /// ```
    #[fehler::throws]
    pub fn bridge_delm(&self, members: &[&str]) {
        members
            .iter()
            .map(|member| {
                bridge_delm(&self.socket, &self.request.ifr_name, member)
            })
            .collect::<Result<_, _>>()?;
    }

    /// Destroy the interface
    ///
    /// # Examples
    /// destroy epair0a interface
    ///
    /// ```rust,no_run
    /// use netzwerk::interface::Interface;
    ///
    /// Interface::new("epair0a")
    ///     .expect("Failed to create iface socket")
    ///     .destroy()
    ///     .expect("Failed to destroy interface");
    /// ```
    #[fehler::throws]
    pub fn destroy(&self) {
        destroy_interface(&self.socket, &self.request)?;
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::Interface;

    #[fehler::throws(anyhow::Error)]
    fn create_interface(r#type: &str, name: &str) -> Interface {
        Interface::new(r#type)?.create()?.name(name)?.address(
            "172.23.0.1",
            "172.23.255.255",
            "255.255.0.0",
        )?
    }

    #[test_helpers::jailed_test]
    fn test_bridge_creation() {
        let _iface = create_interface("bridge", "werft0")
            .expect("Failed to create interface");

        let ifconfig_output = Command::new("ifconfig")
            .arg("werft0")
            .arg("inet")
            .output()
            .expect("Failed to execute ifconfig");

        assert_eq!(
            test_helpers::fixture!("ifconfig"),
            String::from_utf8(ifconfig_output.stdout).unwrap()
        );
    }

    #[test_helpers::jailed_test]
    fn test_bridge_addm() {
        let bridge = create_interface("bridge", "werft0")
            .expect("Failed to create interface");

        let _pair = create_interface("epair", "werftpair")
            .expect("Failed to create interface");

        let _pair2 = create_interface("epair", "werftpair2")
            .expect("Failed to create interface");

        bridge
            .bridge_addm(&["werftpair", "werftpair2"])
            .expect("Failed to addm");

        let ifconfig_output = Command::new("ifconfig")
            .arg("werft0")
            .output()
            .expect("failed to execute ifconfig");

        let content = String::from_utf8(ifconfig_output.stdout).unwrap();

        assert!(content.contains("member: werftpair"));
        assert!(content.contains("member: werftpair2"));
    }

    #[test_helpers::jailed_test]
    fn test_bridge_delm() {
        let bridge = create_interface("bridge", "werft0")
            .expect("Failed to create interface");

        let _pair = create_interface("epair", "werftpair")
            .expect("Failed to create interface");

        let _pair2 = create_interface("epair", "werftpair2")
            .expect("Failed to create interface");

        bridge
            .bridge_addm(&["werftpair", "werftpair2"])
            .expect("Failed to addm");

        bridge.bridge_delm(&["werftpair2"]).expect("Failed to addm");

        let ifconfig_output = Command::new("ifconfig")
            .arg("werft0")
            .output()
            .expect("failed to execute ifconfig");

        let content = String::from_utf8(ifconfig_output.stdout).unwrap();

        assert!(content.contains("member: werftpair"));
        assert!(!content.contains("member: werftpair2"));
    }

    #[test_helpers::jailed_test]
    fn test_bridge_destroy() {
        Command::new("ifconfig")
            .arg("bridge")
            .arg("create")
            .arg("name")
            .arg("werft0")
            .output()
            .expect("Failed to execute ifconfig");

        let iface =
            Interface::new("werft0").expect("Failed to init interface");

        iface.destroy().expect("Failed to destroy interface");

        let ifconfig_output = Command::new("ifconfig")
            .arg("werft0")
            .output()
            .expect("failed to execute ifconfig");

        assert_eq!(Vec::<u8>::new(), ifconfig_output.stdout);
        assert_eq!(
            b"ifconfig: interface werft0 does not exist\n".to_vec(),
            ifconfig_output.stderr
        );
    }

    #[test_helpers::jailed_test]
    fn test_vnet() {
        use jail::process::Jailed;
        use jail::StoppedJail;

        let jail =
            StoppedJail::new("/").param("vnet", jail::param::Value::Int(1));

        let running = jail.start().expect("Couldn't start jail");

        let bridge = create_interface("bridge", "werft0").unwrap();

        bridge.vnet(running.jid).unwrap();

        let ifconfig_output = Command::new("ifconfig")
            .jail(&running)
            .output()
            .expect("failed to execute ifconfig in the jail");

        let content = String::from_utf8(ifconfig_output.stdout).unwrap();

        assert!(content.contains("werft0"));

        running.stop().expect("Failed to stop the jail!");
    }
}
