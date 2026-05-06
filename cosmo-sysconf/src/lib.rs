//! Runtime system-config lookups for Rust-on-Cosmopolitan binaries.
//!
//! A single Cosmopolitan APE binary runs on Linux, macOS, the BSDs, and
//! Windows. Most Rust code that needs system config (e.g. DNS servers)
//! picks a source at compile time: `cfg(unix)` parses
//! `/etc/resolv.conf`, `cfg(windows)` calls `GetAdaptersAddresses` via
//! a Windows-only crate. Under Cosmopolitan we compile
//! `target_os = "linux"`, so the `cfg(unix)` path wins and the Windows
//! code is dead — which then fails at runtime on the Windows host
//! because `/etc/resolv.conf` doesn't exist.
//!
//! This crate closes that gap. At runtime we read Cosmopolitan's
//! `__hostos` bitmask and dispatch to the right source: `resolv.conf`
//! parsing on Unix-ish hosts, `GetAdaptersAddresses()` (via cosmo's
//! Win32 bindings) on Windows.
//!
//! # Cargo config
//!
//! This crate is only useful when compiled with `--cfg=cosmo`, i.e.
//! inside a project that already has the cosmo build scaffolding. On
//! other targets it still compiles (the Windows path is dead), but
//! `system_nameservers` just parses `/etc/resolv.conf` as if it were
//! on Unix.

use std::net::IpAddr;

/// Returns the list of DNS server addresses configured on the host OS,
/// in the order the OS reports them. Empty vec on errors or if no
/// servers are found.
pub fn system_nameservers() -> Vec<IpAddr> {
    #[cfg(cosmo)]
    {
        if cosmo::is_windows() {
            return windows::nameservers().unwrap_or_default();
        }
    }
    unix::nameservers_from_resolv_conf("/etc/resolv.conf").unwrap_or_default()
}

mod unix {
    use std::fs::File;
    use std::io::{self, BufRead, BufReader};
    use std::net::IpAddr;
    use std::path::Path;

    pub(crate) fn nameservers_from_resolv_conf(path: impl AsRef<Path>) -> io::Result<Vec<IpAddr>> {
        let f = File::open(path)?;
        let mut out = Vec::new();
        for line in BufReader::new(f).lines() {
            let line = line?;
            let line = line.split('#').next().unwrap_or("").trim();
            if let Some(rest) = line.strip_prefix("nameserver") {
                let addr = rest.trim();
                if addr.is_empty() {
                    continue;
                }
                if let Ok(ip) = addr.parse::<IpAddr>() {
                    out.push(ip);
                }
            }
        }
        Ok(out)
    }
}

#[cfg(cosmo)]
mod cosmo {
    unsafe extern "C" {
        // cosmo's runtime host-OS bitmask, populated before main() runs.
        // Bit layout comes from libc/dce.h:
        //   _HOSTLINUX=1 _HOSTMETAL=2 _HOSTWINDOWS=4 _HOSTXNU=8
        //   _HOSTOPENBSD=16 _HOSTFREEBSD=32 _HOSTNETBSD=64
        static __hostos: libc::c_int;
    }

    const HOST_WINDOWS: libc::c_int = 4;

    pub(crate) fn is_windows() -> bool {
        // SAFETY: __hostos is a simple c_int populated by cosmo's
        // runtime init before main() runs. Always safe to read.
        (unsafe { __hostos } & HOST_WINDOWS) != 0
    }
}

#[cfg(cosmo)]
mod windows {
    //! `GetAdaptersAddresses` walker via cosmo's Win32 bindings.
    //!
    //! Layout is the same as Microsoft's `IP_ADAPTER_ADDRESSES` in
    //! iphlpapi.h — cosmo exposes it as `struct NtIpAdapterAddresses`
    //! in `libc/nt/struct/ipadapteraddresses.h`. We mirror just the
    //! fields we need (Next, FirstDnsServerAddress), plus the
    //! dns-server node type and `SOCKET_ADDRESS`.

    use std::io;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::os::raw::{c_int, c_void};

    // Sockaddr family values on Windows (same as POSIX).
    const AF_UNSPEC: u32 = 0;
    const AF_INET: u16 = 2;
    const AF_INET6: u16 = 23; // Windows AF_INET6 — differs from POSIX 10,
    // but winsock uses 23. `NtSocketAddress` points at an `SOCKADDR`
    // filled by Windows, so we use Windows's family numbers here.

    // Windows error codes.
    const ERROR_SUCCESS: u32 = 0;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;

    // Flags: skip everything we don't need, keep DNS.
    const GAA_FLAG_SKIP_UNICAST: u32 = 0x0001;
    const GAA_FLAG_SKIP_ANYCAST: u32 = 0x0002;
    const GAA_FLAG_SKIP_MULTICAST: u32 = 0x0004;
    const GAA_FLAG_SKIP_FRIENDLY_NAME: u32 = 0x0020;

    #[repr(C)]
    struct SocketAddress {
        lp_sockaddr: *mut u8,
        i_sockaddr_length: c_int,
    }

    #[repr(C)]
    struct DnsServerAddress {
        length: u32,
        reserved: u32,
        next: *mut DnsServerAddress,
        address: SocketAddress,
    }

    // IP_ADAPTER_ADDRESSES — we only read `next` and
    // `first_dns_server_address`. The rest of the struct is enormous
    // (~600 bytes on x64) but we don't touch those fields. We keep the
    // layout accurate up through the DNS-server pointer and treat the
    // remainder as an opaque blob; the actual allocation size is
    // dictated by GetAdaptersAddresses itself via its out-param, and
    // we let the OS decide.
    #[repr(C)]
    struct AdapterAddresses {
        length: u32,
        if_index: u32,
        next: *mut AdapterAddresses,
        adapter_name: *mut u8,
        first_unicast_address: *mut c_void,
        first_anycast_address: *mut c_void,
        first_multicast_address: *mut c_void,
        first_dns_server_address: *mut DnsServerAddress,
        // … rest of struct deliberately omitted; see cosmo's
        // libc/nt/struct/ipadapteraddresses.h for the full layout.
    }

    unsafe extern "C" {
        fn GetAdaptersAddresses(
            family: u32,
            flags: u32,
            reserved: *mut c_void,
            adapter_addresses: *mut u8,
            size_pointer: *mut u32,
        ) -> u32;
    }

    pub(crate) fn nameservers() -> io::Result<Vec<IpAddr>> {
        let mut size: u32 = 15 * 1024; // 15 KB — docs recommend starting here
        let flags = GAA_FLAG_SKIP_UNICAST
            | GAA_FLAG_SKIP_ANYCAST
            | GAA_FLAG_SKIP_MULTICAST
            | GAA_FLAG_SKIP_FRIENDLY_NAME;

        // Retry loop — up to 3 tries in case the size grows between calls.
        for _ in 0..3 {
            let mut buf = vec![0u8; size as usize];
            let rc = unsafe {
                GetAdaptersAddresses(
                    AF_UNSPEC,
                    flags,
                    std::ptr::null_mut(),
                    buf.as_mut_ptr(),
                    &mut size,
                )
            };
            match rc {
                ERROR_SUCCESS => return Ok(walk(buf.as_ptr() as *const AdapterAddresses)),
                ERROR_BUFFER_OVERFLOW => continue, // size now holds the required length
                other => return Err(io::Error::from_raw_os_error(other as i32)),
            }
        }
        Err(io::Error::other("GetAdaptersAddresses size kept growing"))
    }

    fn walk(mut adapter: *const AdapterAddresses) -> Vec<IpAddr> {
        let mut out = Vec::new();
        while !adapter.is_null() {
            // SAFETY: the OS populated this linked list. We only read
            // the two pointer fields we mirrored exactly.
            let (mut dns, next) = unsafe {
                let a = &*adapter;
                (a.first_dns_server_address, a.next)
            };
            while !dns.is_null() {
                // SAFETY: same — we read only mirrored fields.
                let (addr, len, next_dns) = unsafe {
                    let d = &*dns;
                    (d.address.lp_sockaddr, d.address.i_sockaddr_length, d.next)
                };
                if let Some(ip) = sockaddr_to_ip(addr, len) {
                    out.push(ip);
                }
                dns = next_dns;
            }
            adapter = next;
        }
        out
    }

    fn sockaddr_to_ip(sa: *const u8, len: c_int) -> Option<IpAddr> {
        if sa.is_null() || len < 2 {
            return None;
        }
        // SAFETY: sockaddr_family is always the first u16 of any
        // sockaddr variant (WSA uses the same shape as POSIX).
        let family = unsafe { std::ptr::read_unaligned(sa as *const u16) };
        match family {
            f if f == AF_INET => {
                if (len as usize) < 8 {
                    return None;
                }
                // sockaddr_in: [family u16][port u16][addr 4 bytes]...
                let octets = unsafe { std::ptr::read_unaligned(sa.add(4) as *const [u8; 4]) };
                Some(IpAddr::V4(Ipv4Addr::from(octets)))
            }
            f if f == AF_INET6 => {
                if (len as usize) < 28 {
                    return None;
                }
                // sockaddr_in6: [family u16][port u16][flowinfo u32][addr 16 bytes]...
                let bytes = unsafe { std::ptr::read_unaligned(sa.add(8) as *const [u8; 16]) };
                Some(IpAddr::V6(Ipv6Addr::from(bytes)))
            }
            _ => None,
        }
    }
}
