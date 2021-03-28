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

const JAIL_NUMBER: i32 = 5;

extern "C" {
    fn jail_attach(jid: i32) -> i32;
}

fn create_interfaces() -> Result<String> {
    let bridge = Interface::new("bridge")
        .expect("Failed to create iface socket")
        .create()?
        .name("werft0")?
        .address("172.24.0.1", "172.24.0.255", "255.255.255.0")?;

    let pair_a = Interface::new("epair")?
        .create()?;

    let name = pair_a.get_name()?;
    let len = name.len();
    let name_b = &[&name[..len - 1], "b"].join("");

    let pair_b = Interface::new(name_b)
        .expect("Failed to create iface socket");

    pair_b
        .vnet(JAIL_NUMBER) /* Transfer interface to the jail #5 */
        .expect("Failed to move interface to the jail");

    bridge.bridge_addm(&[name])?;

    Ok(String::from(name_b))
}


fn main() {
    let name = create_interfaces()
        .expect("Failed to create interfaces");

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            if unsafe { jail_attach(JAIL_NUMBER) } < 0 {
                panic!("Failed to attach to jail {}", JAIL_NUMBER);
            };

            let pair_b = Interface::new(&name)
                .expect("Failed to create iface socket");

            pair_b
                .address("172.24.0.2", "172.24.0.255", "255.255.255.0")
                .unwrap();
            route::add_default("172.24.0.1").unwrap();
        },
        _ => (),

    }
}
