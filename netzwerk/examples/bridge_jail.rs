// Set up network inside a jail:
//
// * Create a bridge and an epair
// * Assign an ip address to the jail
// * Move one edge of the epair into the jail
// * Give that edge an ip address
// * Configure default route inside the jail
extern crate anyhow;
extern crate netzwerk;
extern crate nix;

use anyhow::Result;
use netzwerk::interface::Interface;
use netzwerk::route;
use nix::unistd::{fork, ForkResult};

extern "C" {
    fn jail_attach(jid: i32) -> i32;
}

fn create_interfaces(jail_number: i32) -> Result<String> {
    let bridge = Interface::new("bridge")
        .expect("Failed to create iface socket")
        .create()?
        .name("knast0")?;

    let pair_a = Interface::new("epair")?.create()?.address(
        "172.24.0.1",
        "172.24.0.255",
        "255.255.255.0",
    )?;

    let name = pair_a.get_name()?;
    let len = name.len();
    let name_b = &[&name[..len - 1], "b"].join("");

    let pair_b =
        Interface::new(name_b).expect("Failed to create iface socket");

    pair_b
        .vnet(jail_number) /* Transfer interface to the jail */
        .expect("Failed to move interface to the jail");

    bridge.bridge_addm(&[name])?;

    Ok(String::from(name_b))
}

fn main() {
    let jid = std::env::args()
        .nth(1)
        .expect("USAGE: bridge_jail JID")
        .parse()
        .expect("Failed to parse jail id");

    let name = create_interfaces(jid).expect("Failed to create interfaces");

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            if unsafe { jail_attach(jid) } < 0 {
                panic!("Failed to attach to jail {}", jid);
            };

            let pair_b =
                Interface::new(&name).expect("Failed to create iface socket");

            pair_b
                .address("172.24.0.2", "172.24.0.255", "255.255.255.0")
                .unwrap();
            route::add_default("172.24.0.1").unwrap();
        }
        _ => (),
    }
}
