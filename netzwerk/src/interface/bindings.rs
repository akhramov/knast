use std::io::Error as StdError;
use std::mem;

use anyhow::{anyhow, Error};
use libc::{c_void, ioctl, size_t, sockaddr_in};

use crate::common_bindings::{get_address, Socket};

// FreeBSD 13.0-CURRENT r361779
const SIOCAIFADDR: u64 = 0x8044692b;
const SIOCIFCREATE: u64 = 0xc020697a;
const SIOCSIFNAME: u64 = 0x80206928;
const SIOCIFDESTROY: u64 = 0x80206979;
const SIOCSDRVSPEC: u64 = 0x8028697b;
const SIOCSIFVNET: u64 = 0xc020695a;

const BRDGADD: u64 = 0x0;
const BRDGDEL: u64 = 0x1;

#[repr(C)]
pub struct ifreq {
    pub ifr_name: [u8; 16usize],
    pub ifr_ifru: ifru,
}

#[repr(C)]
pub union ifru {
    pub ifru_addr: sockaddr_in,
    pub ifru_dstaddr: sockaddr_in,
    pub ifru_broadaddr: sockaddr_in,
    pub ifru_flags: [i32; 2usize],
    pub ifru_index: i32,
    pub ifru_jid: i32,
    pub ifru_metric: i32,
    pub ifru_mtu: i32,
    pub ifru_phys: i32,
    pub ifru_media: i32,
    pub ifru_data: *const u8,
    pub ifru_cap: [i32; 2usize],
    pub ifru_fib: u32,
    pub ifru_vlan_pcp: u8,
    _align: [u64; 2usize],
}

#[fehler::throws]
pub fn destroy_interface(socket: &Socket, request: &ifreq) {
    if unsafe { ioctl(socket.0, SIOCIFDESTROY, request) } < 0 {
        fehler::throw!(anyhow!(
            "destroy interface: ioctl(SIOCIFDESTROY) failed: {}",
            StdError::last_os_error()
        ))
    };
}

#[fehler::throws]
pub fn create_interface(socket: &Socket, request: &ifreq) {
    if unsafe { ioctl(socket.0, SIOCIFCREATE, request) } < 0 {
        fehler::throw!(anyhow!(
            "create interface: ioctl(SIOCIFCREATE) failed: {}",
            StdError::last_os_error()
        ))
    };
}

#[fehler::throws]
pub fn rename_interface(socket: &Socket, request: &mut ifreq, name: &str) {
    let new_name = [name, "\0"].concat();
    request.ifr_ifru.ifru_data = new_name.as_ptr();

    {
        if unsafe { ioctl(socket.0, SIOCSIFNAME, request as *mut _) } < 0 {
            fehler::throw!(anyhow!(
                "rename interface: ioctl(SIOCSIFNAME) failed: {}",
                StdError::last_os_error()
            ))
        };
    }

    request.ifr_name[0..new_name.len()].copy_from_slice(&new_name.as_bytes());
}

#[fehler::throws]
pub fn jail_interface(socket: &Socket, request: &mut ifreq, jid: i32) {
    request.ifr_ifru.ifru_jid = jid;

    if unsafe { ioctl(socket.0, SIOCSIFVNET, request as *mut _) } < 0 {
        fehler::throw!(anyhow!(
            "jail interface: ioctl(SIOCSIFVNET) failed: {}",
            StdError::last_os_error()
        ))
    };
}

#[fehler::throws]
pub fn set_interface_address(
    socket: &Socket,
    name: &[u8],
    address: &str,
    broadcast: &str,
    mask: &str,
) {
    let mut request: ifaliasreq = unsafe { mem::zeroed() };

    request.ifra_name[0..name.len()].copy_from_slice(name);
    request.ifra_addr = get_address(Some(&address))?;
    request.ifra_broadaddr = get_address(Some(&broadcast))?;
    request.ifra_mask = get_address(Some(&mask))?;

    if unsafe { ioctl(socket.0, SIOCAIFADDR, &request) } < 0 {
        fehler::throw!(anyhow!(
            "set interface address: ioctl(SIOCAIFADDR) failed: {}",
            StdError::last_os_error()
        ))
    };
}

// TODO: shall we just inline the common portion?
macro_rules! bridge_request {
    ($func:ident, $cmd:expr) => {
        #[fehler::throws]
        pub fn $func(socket: &Socket, name: &[u8], member: &str) {
            let mut bridge_request: ifbreq = unsafe { mem::zeroed() };
            bridge_request.ifbr_ifsname[0..member.len()]
                .copy_from_slice(member.as_bytes());

            let mut request: ifdrv = unsafe { mem::zeroed() };
            request.ifd_name[0..name.len()].copy_from_slice(name);
            request.ifd_cmd = $cmd;
            request.ifd_len = mem::size_of::<ifbreq>();
            request.ifd_data = &bridge_request as *const _ as _;

            if unsafe { ioctl(socket.0, SIOCSDRVSPEC, &request) } < 0 {
                fehler::throw!(anyhow!(
                    "bridge request: ioctl(SIOCSDRVSPEC) failed: {}",
                    StdError::last_os_error()
                ))
            }
        }
    };
}

bridge_request!(bridge_addm, BRDGADD);
bridge_request!(bridge_delm, BRDGDEL);

#[repr(C)]
struct ifaliasreq {
    pub ifra_name: [u8; 16usize],
    pub ifra_addr: sockaddr_in,
    pub ifra_broadaddr: sockaddr_in,
    pub ifra_mask: sockaddr_in,
}

#[repr(C)]
struct ifbreq {
    pub ifbr_ifsname: [u8; 16usize],
    pub ifbr_ifsflags: u32,
    pub ifbr_stpflags: u32,
    pub ifbr_path_cost: u32,
    pub ifbr_portno: u8,
    pub ifbr_priority: u8,
    pub ifbr_proto: u8,
    pub ifbr_role: u8,
    pub ifbr_state: u8,
    pub ifbr_addrcnt: u32,
    pub ifbr_addrmax: u32,
    pub ifbr_addrexceeded: u32,
    _align: [u64; 4usize],
}

#[repr(C)]
struct ifdrv {
    pub ifd_name: [u8; 16usize],
    pub ifd_cmd: u64,
    pub ifd_len: size_t,
    pub ifd_data: *const c_void,
}
