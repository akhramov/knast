use std::io::Error as StdError;
use std::mem;

use anyhow::{anyhow, Error};
use common_lib::AsSignedBytes;
use libc::ioctl;

use crate::{
    bindings::{ifaliasreq, ifbreq, ifdrv, ifreq},
    common_bindings::{get_address, Socket},
};

// FreeBSD 13.0-CURRENT r361779
const SIOCAIFADDR: u64 = 0x8044692b;
const SIOCIFCREATE: u64 = 0xc020697a;
const SIOCSIFNAME: u64 = 0x80206928;
const SIOCIFDESTROY: u64 = 0x80206979;
const SIOCSDRVSPEC: u64 = 0x8028697b;
const SIOCSIFVNET: u64 = 0xc020695a;
const SIOCGIFCAP: u64 = 0xc020691f;

const BRDGADD: u64 = 0x0;
const BRDGDEL: u64 = 0x1;

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
    request.ifr_ifru.ifru_data = new_name.as_ptr() as *mut _;

    {
        if unsafe { ioctl(socket.0, SIOCSIFNAME, request as *mut _) } < 0 {
            fehler::throw!(anyhow!(
                "rename interface: ioctl(SIOCSIFNAME) failed: {}",
                StdError::last_os_error()
            ))
        };
    }

    request.ifr_name[0..new_name.len()]
        .copy_from_slice(new_name.as_str().as_signed_bytes());
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
    name: &[i8],
    address: &str,
    broadcast: &str,
    mask: &str,
) {
    let mut request: ifaliasreq = unsafe { mem::zeroed() };

    request.ifra_name[0..name.len()].copy_from_slice(name);

    // Safety: ifra_addr receives `sockaddr`, which is a generalization of `sockaddr_in`.
    unsafe {
        request.ifra_addr = std::mem::transmute(get_address(Some(&address))?);
        request.ifra_broadaddr =
            std::mem::transmute(get_address(Some(&broadcast))?);
        request.ifra_mask = std::mem::transmute(get_address(Some(&mask))?);
    }

    if unsafe { ioctl(socket.0, SIOCAIFADDR, &request) } < 0 {
        fehler::throw!(anyhow!(
            "set interface address: ioctl(SIOCAIFADDR) failed: {}",
            StdError::last_os_error()
        ))
    };
}

#[fehler::throws]
pub fn check_interface_existence(socket: &Socket, request: &ifreq) -> bool {
    unsafe { ioctl(socket.0, SIOCGIFCAP, request) >= 0 }
}

macro_rules! bridge_request {
    ($func:ident, $cmd:expr) => {
        #[fehler::throws]
        pub fn $func(socket: &Socket, name: &[i8], member: &str) {
            let mut bridge_request: ifbreq = unsafe { mem::zeroed() };
            bridge_request.ifbr_ifsname[0..member.len()]
                .copy_from_slice(member.as_signed_bytes());

            let mut request: ifdrv = unsafe { mem::zeroed() };
            request.ifd_name[0..name.len()].copy_from_slice(name);
            request.ifd_cmd = $cmd;
            request.ifd_len = mem::size_of::<ifbreq>() as _;
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
