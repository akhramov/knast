use std::io::Error as StdError;
use std::mem;

use anyhow::{anyhow, Error};
use libc::{sockaddr_in, write, PF_ROUTE, SOCK_RAW};

/* net/route.h */
const RTM_ADD: u8 = 0x1;
const RTM_DELETE: u8 = 0x2;

const RTM_VERSION: u8 = 5;

const RTF_UP: u32 = 0x1;
const RTF_GATEWAY: u32 = 0x2;
const RTF_STATIC: u32 = 0x800;
const RTF_PINNED: u32 = 0x100000;

const RTA_DST: u32 = 0x1;
const RTA_GATEWAY: u32 = 0x2;
const RTA_NETMASK: u32 = 0x4;

use crate::common_bindings::{get_address, Socket};

#[derive(Copy, Clone)]
pub enum Operation {
    Add = RTM_ADD as _,
    Delete = RTM_DELETE as _,
}

#[fehler::throws]
pub fn rtmsg(operation: Operation, address: Option<&str>) {
    let socket = Socket::new(PF_ROUTE, SOCK_RAW)?;

    let header: rt_msghdr = unsafe { mem::zeroed() };

    let payload = [
        get_address(None)?,
        get_address(address)?,
        get_address(None)?,
    ];

    let mut message = rtmsg { header, payload };

    message.header.rtm_type = operation as _;
    message.header.rtm_flags = RTF_UP | RTF_GATEWAY | RTF_STATIC | RTF_PINNED;
    message.header.rtm_version = RTM_VERSION;
    message.header.rtm_addrs = match operation {
        Operation::Add => RTA_DST | RTA_GATEWAY | RTA_NETMASK,
        Operation::Delete => RTA_DST | RTA_NETMASK
    };
    message.header.rtm_seq = 1;
    let len = mem::size_of::<rtmsg<[sockaddr_in; 3]>>();

    message.header.rtm_msglen = len as _;

    if unsafe { write(socket.0, &message as *const _ as _, len) } < 0 {
        fehler::throw!(anyhow!(
            "add net default: write failed: {}",
            StdError::last_os_error()
        ))
    };
}

#[repr(C)]
struct rtmsg<T> {
    pub header: rt_msghdr,
    pub payload: T,
}

// This makes us 64-bit only, right?
#[repr(C)]
struct rt_msghdr {
    pub rtm_msglen: u16,
    pub rtm_version: u8,
    pub rtm_type: u8,
    pub rtm_index: u16,
    _rtm_spare1: u16,
    pub rtm_flags: u32,
    pub rtm_addrs: u32,
    pub rtm_pid: u32,
    pub rtm_seq: u32,
    pub rtm_errno: u32,
    pub rtm_fmask: u32,
    pub rtm_inits: u64,
    _rt_metrics: [u64; 14usize],
}
