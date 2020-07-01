use std::io::Error as StdError;
use std::mem;

use anyhow::{anyhow, Error};
use libc::{c_int, c_void, close, sockaddr_in, socket, AF_INET};

extern "C" {
    fn inet_pton(af: i32, src: *const u8, dst: *mut c_void) -> i32;
}

pub struct Socket(pub i32);

impl Socket {
    #[fehler::throws]
    pub fn new(domain: c_int, r#type: c_int) -> Self {
        let sock = unsafe { socket(domain, r#type, 0) };

        if sock < 0 {
            fehler::throw!(anyhow!(
                "cannot open socket: {}",
                StdError::last_os_error()
            ))
        }

        Self(sock)
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        unsafe { close(self.0) };
    }
}

#[fehler::throws]
pub fn get_address(address: Option<&str>) -> sockaddr_in {
    let mut result: sockaddr_in = unsafe { mem::zeroed() };

    result.sin_len = mem::size_of::<sockaddr_in>() as u8;
    result.sin_family = AF_INET as u8;

    let address = match address {
        Some(add) => add,
        None => return result,
    };

    match unsafe {
        inet_pton(
            AF_INET,
            [address, "\0"].concat().as_ptr(),
            &mut result.sin_addr as *mut _ as *mut c_void,
        )
    } {
        0 => {
            fehler::throw!(anyhow!(
                "inet_pton failed: could not parse inet address"
            ))
        }
        -1 => {
            fehler::throw!(anyhow!(
                "inet_pton failed: {}",
                StdError::last_os_error()
            ))
        }
        _ => result,
    }
}
