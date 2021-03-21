#[allow(
    non_camel_case_types,
    non_snake_case,
    dead_code,
    non_upper_case_globals
)]
mod bindings;

use std::{
    fs::{File, OpenOptions},
    io::Error as StdError,
    mem,
    net::Ipv4Addr,
    os::unix::io::AsRawFd,
};

use anyhow::{anyhow, Error};
use bindings::{
    pfioc_pooladdr, pfioc_rule, pfioc_table, pfioc_trans,
    pfioc_trans_pfioc_trans_e, pfr_addr, pfr_table, PFI_AFLAG_NOALIAS,
    PFR_TFLAG_PERSIST, PF_ADDR_DYNIFTL, PF_NAT, PF_RULESET_NAT,
};
use common_lib::AsSignedBytes;
use ipnetwork::Ipv4Network;
use libc::{ioctl, AF_INET};

use super::nat::Nat;

const PF_DEVICE_PATH: &str = "/dev/pf";
const ANCHOR: [i8; 12] = unsafe { mem::transmute(*b"knast_anker\0") };
const TABLE_NAME: [i8; 6] = unsafe { mem::transmute(*b"jails\0") };

const DIOCXBEGIN: u64 = 0xc0104451;
const DIOCXCOMMIT: u64 = 0xc0104452;
const DIOCXROLLBACK: u64 = 0xc0104453;
const DIOCBEGINADDRS: u64 = 0xc4704433;
const DIOCADDADDR: u64 = 0xc4704434;
const DIOCADDRULE: u64 = 0xcbe04404;
const DIOCRADDTABLES: u64 = 0xc450443d;
const DIOCRADDADDRS: u64 = 0xc4504443;

// https://github.com/freebsd/freebsd-src/blob/098dbd7ff7f3da9dda03802cdb2d8755f816eada/sbin/pfctl/pfctl_parser.h
const PF_NAT_PORT_RANGE: [u16; 2] = [50001, 65535];

pub struct Pf {
    pf_device: File,
}

impl Pf {
    #[fehler::throws]
    pub fn new(interface: &str) -> Self {
        Self {
            pf_device: OpenOptions::new().write(true).open(&PF_DEVICE_PATH)?,
        }
        .initialize(interface)?
    }

    /// Initializes NAT rule
    fn initialize(self, interface: &str) -> Result<Self, Error> {
        self.transaction(None, |handle, ticket, pool_ticket| {
            add_rule(handle, ticket, pool_ticket, |mut result| {
                result.anchor_call[0..ANCHOR.len()].copy_from_slice(&ANCHOR);

                result
            })
        })?
        .transaction(
            Some(&ANCHOR),
            |handle, ticket, pool_ticket| {
                add_address(handle, pool_ticket, interface)?;

                add_rule(handle, ticket, pool_ticket, |mut result| {
                    result.anchor[0..ANCHOR.len()].copy_from_slice(&ANCHOR);
                    result.rule.ifname[0..interface.len()]
                        .copy_from_slice(interface.as_signed_bytes());
                    result.rule.src.addr.type_ = 3; // tblname
                    result.rule.af = AF_INET as _;
                    result.rule.rpool.proxy_port = PF_NAT_PORT_RANGE;

                    unsafe {
                        result.rule.src.addr.v.tblname[0..TABLE_NAME.len()]
                            .copy_from_slice(&TABLE_NAME)
                    };

                    result
                })
            },
        )
    }

    #[fehler::throws]
    fn transaction<T>(
        self,
        anchor: Option<&[i8]>,
        body: impl FnOnce(i32, u32, u32) -> Result<T, Error>,
    ) -> Self {
        let (data, nat_request) = transaction_struct(anchor);
        let handle = self.pf_device.as_raw_fd();

        begin_transaction(handle, &data)?;
        let pool_address = begin_addresses(handle)?;

        match body(handle, nat_request.ticket, pool_address.ticket) {
            Ok(_) => commit_transaction(handle, &data)?,
            err => {
                rollback_transaction(handle, &data)?;
                err?;
            }
        }

        self
    }
}

impl Nat for Pf {
    #[fehler::throws]
    fn add(&self, subnet: &str) {
        let handle = self.pf_device.as_raw_fd();

        create_table(handle)?;
        add_address_to_table(handle, subnet)?;
    }
}

#[fehler::throws]
fn create_table(handle: i32) {
    let mut result: pfioc_table = unsafe { mem::zeroed() };
    let mut table = table_struct();
    table.pfrt_flags = PFR_TFLAG_PERSIST;

    result.pfrio_esize = mem::size_of::<pfr_table>() as _;
    result.pfrio_size = 1;
    result.pfrio_buffer = &table as *const _ as _;

    if unsafe { ioctl(handle, DIOCRADDTABLES, &result) } < 0 {
        fehler::throw!(anyhow!(
            "add NAT rule : ioctl(DIOCRADDTABLES) failed: {}",
            StdError::last_os_error()
        ))
    };
}

#[fehler::throws]
fn add_address_to_table(handle: i32, address: &str) {
    let parsed_address: Ipv4Network = address.parse()?;
    let mut result: pfioc_table = unsafe { mem::zeroed() };
    let mut address: pfr_addr = unsafe { mem::zeroed() };
    let table = table_struct();

    address.pfra_af = AF_INET as _;
    address.pfra_net = parsed_address.prefix();
    address.pfra_u._pfra_ip4addr.s_addr =
        u32::from_be(parsed_address.network().into());

    result.pfrio_table = table;
    result.pfrio_esize = mem::size_of::<pfr_addr>() as _;
    result.pfrio_size = 1;
    result.pfrio_buffer = &address as *const _ as _;

    if unsafe { ioctl(handle, DIOCRADDADDRS, &result) } < 0 {
        fehler::throw!(anyhow!(
            "add NAT rule : ioctl(DIOCRADDADDRS) failed: {}",
            StdError::last_os_error()
        ))
    };
}

fn table_struct() -> pfr_table {
    let mut table: pfr_table = unsafe { mem::zeroed() };

    table.pfrt_anchor[0..ANCHOR.len()].copy_from_slice(&ANCHOR);
    table.pfrt_name[0..TABLE_NAME.len()].copy_from_slice(&TABLE_NAME);

    table
}

#[fehler::throws]
fn rollback_transaction(handle: i32, transaction_struct: &pfioc_trans) {
    if unsafe { ioctl(handle, DIOCXROLLBACK, transaction_struct) } < 0 {
        fehler::throw!(anyhow!(
            "initialize NAT: ioctl(DIOCXROLLBACK) failed: {}",
            StdError::last_os_error()
        ))
    };
}

#[fehler::throws]
fn commit_transaction(handle: i32, transaction_struct: &pfioc_trans) {
    if unsafe { ioctl(handle, DIOCXCOMMIT, transaction_struct) } < 0 {
        fehler::throw!(anyhow!(
            "initialize NAT: ioctl(DIOCXCOMMIT) failed: {}",
            StdError::last_os_error()
        ))
    };
}

#[fehler::throws]
fn begin_transaction(handle: i32, transaction_struct: &pfioc_trans) {
    if unsafe { ioctl(handle, DIOCXBEGIN, transaction_struct) } < 0 {
        fehler::throw!(anyhow!(
            "initialize NAT: ioctl(DIOCXBEGIN) failed: {}",
            StdError::last_os_error()
        ))
    };
}

#[fehler::throws]
fn begin_addresses(handle: i32) -> pfioc_pooladdr {
    let result: pfioc_pooladdr = unsafe { mem::zeroed() };

    if unsafe { ioctl(handle, DIOCBEGINADDRS, &result) } < 0 {
        fehler::throw!(anyhow!(
            "initialize NAT: ioctl(DIOCBEGINADDRS) failed: {}",
            StdError::last_os_error()
        ))
    };

    result
}

#[fehler::throws]
fn add_address(
    handle: i32,
    pool_ticket: u32,
    interface: &str,
) -> pfioc_pooladdr {
    let mut result: pfioc_pooladdr = unsafe { mem::zeroed() };

    result.ticket = pool_ticket;
    result.af = AF_INET as _;
    result.addr.addr.type_ = PF_ADDR_DYNIFTL as _;
    result.addr.addr.iflags = PFI_AFLAG_NOALIAS as _;
    unsafe {
        result.addr.addr.v.ifname[0..interface.len()]
            .copy_from_slice(interface.as_signed_bytes());

        result.addr.addr.v.a.mask.pfa.v4.s_addr =
            Ipv4Addr::from([255, 255, 255, 255]).into();
    }

    if unsafe { ioctl(handle, DIOCADDADDR, &result) } < 0 {
        fehler::throw!(anyhow!(
            "initialize NAT: ioctl(DIOCADDADDR) failed: {}",
            StdError::last_os_error()
        ))
    };

    result
}

#[fehler::throws]
fn add_rule(
    handle: i32,
    ticket: u32,
    pool_ticket: u32,
    overrides: impl Fn(pfioc_rule) -> pfioc_rule,
) -> pfioc_rule {
    let mut result: pfioc_rule = unsafe { mem::zeroed() };
    result.ticket = ticket;
    result.pool_ticket = pool_ticket;
    result.rule.action = PF_NAT as _;
    result.rule.rtableid = -1;

    result = overrides(result);

    if unsafe { ioctl(handle, DIOCADDRULE, &result) } < 0 {
        fehler::throw!(anyhow!(
            "initialize NAT: ioctl(DIOCADDRULE) failed: {}",
            StdError::last_os_error()
        ))
    };

    result
}

fn transaction_struct(
    anchor_name: Option<&[i8]>,
) -> (pfioc_trans, Box<pfioc_trans_pfioc_trans_e>) {
    let mut anchor = [0; 1024];

    if let Some(anchor_name) = anchor_name {
        anchor[0..anchor_name.len()].copy_from_slice(anchor_name);
    }

    let boxed_nat_request = Box::new(pfioc_trans_pfioc_trans_e {
        rs_num: PF_RULESET_NAT as _,
        anchor,
        ticket: 0,
    });

    (
        pfioc_trans {
            size: 1,
            esize: mem::size_of::<pfioc_trans_pfioc_trans_e>() as _,
            array: &*boxed_nat_request as *const _
                as *mut pfioc_trans_pfioc_trans_e,
        },
        boxed_nat_request,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test_helpers::jailed_test]
    fn test_anchor_is_created() {
        create_nat("wlan0", "172.24.0.0/24");
        assert!(
            get_anchors().contains("knast_anker"),
            "Anchor wasn't created"
        );
    }

    #[test_helpers::jailed_test]
    fn test_nat_rules_are_populated() {
        let interface = "wlan0";
        create_nat(interface, "172.24.0.0/24");
        assert!(get_anchor_rules("knast_anker").contains(&format!(
            "nat on {interface} inet from <jails> to any -> ({interface}:0)",
            interface = interface
        )));
    }

    #[test_helpers::jailed_test]
    fn test_table_contents() {
        let subnet = "172.24.0.0/24";
        create_nat("wlan0", subnet);
        assert!(get_table_entries("knast_anker", "jails").contains(subnet));
    }

    fn create_nat(interface: &str, subnet: &str) {
        Pf::new(interface)
            .and_then(|nat| nat.add(subnet))
            .expect("failed to create NAT");
    }

    fn get_anchors() -> String {
        pfctl(&["-s", "Anchors"]).expect("(pfctl) Failed to get anchors")
    }

    fn get_anchor_rules(anchor: &str) -> String {
        pfctl(&["-a", anchor, "-sn"])
            .expect("(pfctl) Failed to get anchor rules")
    }

    fn get_table_entries(anchor: &str, table: &str) -> String {
        pfctl(&["-a", anchor, "-t", table, "-T", "show"])
            .expect("(pfctl) Failed to get table contents")
    }

    #[fehler::throws]
    fn pfctl(args: &[&str]) -> String {
        String::from_utf8_lossy(
            &Command::new("pfctl").args(args).output()?.stdout,
        )
        .into()
    }
}
