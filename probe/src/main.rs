//! Portability probe for Rust-on-Cosmopolitan investigation.
//!
//! Each category prints one or more lines to stdout in a structured format:
//!     CAT<NN> <name>: <key>=<value> [<key>=<value> ...]
//!
//! The probe must not panic except where explicitly testing panic behavior.
//! All failures should be captured and printed, not propagated. Exit 0 always
//! (unless the process is killed) so the harness knows the probe itself ran
//! to completion vs. was terminated by the OS.

use std::ffi::{CStr, CString, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::OpenOptionsExt;
use std::panic;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn header() {
    println!("=== rust-cosmo portability probe ===");
    println!(
        "META os={} arch={} family={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::FAMILY
    );
    println!("META rustc_target={}", env!("TARGET", "unknown"));
    // argv[0] shape differs across OSes when launched via APE
    let argv0 = std::env::args().next().unwrap_or_default();
    println!("META argv0={argv0:?}");
    // current dir + tempdir — both have surprising behavior cross-OS
    println!("META cwd={:?}", std::env::current_dir().ok());
    println!("META tempdir={:?}", std::env::temp_dir());
}

// -------- Category 1: errno round-trips --------
// If cosmo normalizes errno to Linux values on all OSes, these are stable.
// If not, you'll see raw_os_error differ while kind() stays the same (or not).
fn cat01_errno() {
    let cases: &[(&str, Box<dyn Fn() -> std::io::Result<()>>)] = &[
        (
            "ENOENT",
            Box::new(|| File::open("/definitely/does/not/exist/probe").map(|_| ())),
        ),
        (
            "EACCES",
            Box::new(|| File::create("/proc/1/mem-probe").map(|_| ())),
        ),
        (
            "EINVAL",
            Box::new(|| {
                // bind to invalid addr shape — should give EINVAL or similar
                std::net::TcpListener::bind("0.0.0.0:0")
                    .and_then(|l| l.set_ttl(0).map(|_| ()))
                    .map(|_| ())
            }),
        ),
    ];
    for (name, f) in cases {
        match f() {
            Ok(()) => println!("CAT01 errno case={name} result=ok_unexpected"),
            Err(e) => println!(
                "CAT01 errno case={} kind={:?} raw={:?} msg={:?}",
                name,
                e.kind(),
                e.raw_os_error(),
                e.to_string()
            ),
        }
    }
}

// -------- Category 2: open flags (O_NONBLOCK, O_CLOEXEC, O_DIRECT etc) --------
fn cat02_open_flags() {
    let path = std::env::temp_dir().join("probe-flags-test");
    let _ = fs::remove_file(&path);

    // SAFETY: under cfg(cosmo), these are `extern static` values
    // populated by libcosmo before main. Reading them is always safe.
    let (nonblock, cloexec) = unsafe { (libc::O_NONBLOCK, libc::O_CLOEXEC) };
    let flags = nonblock | cloexec;
    println!(
        "CAT02 flags O_NONBLOCK={nonblock:#x} O_CLOEXEC={cloexec:#x} combined={flags:#x}"
    );

    let r = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(flags)
        .open(&path);
    match r {
        Ok(mut f) => {
            let w = f.write_all(b"hello").map(|_| ());
            println!(
                "CAT02 open+custom_flags result=ok write={:?}",
                w.map_err(|e| e.kind())
            );
        }
        Err(e) => println!(
            "CAT02 open+custom_flags result=err kind={:?} raw={:?}",
            e.kind(),
            e.raw_os_error()
        ),
    }
    let _ = fs::remove_file(&path);
}

// -------- Category 3: signal constants --------
// Via the libc crate — these are the ones most likely to be wrong cross-OS
// because Rust pins them at compile time.
fn cat03_signals() {
    println!(
        "CAT03 signal SIGHUP={} SIGINT={} SIGTERM={} SIGKILL={} SIGUSR1={} SIGPIPE={} SIGCHLD={} SIGSEGV={}",
        libc::SIGHUP,
        libc::SIGINT,
        libc::SIGTERM,
        libc::SIGKILL,
        libc::SIGUSR1,
        libc::SIGPIPE,
        libc::SIGCHLD,
        libc::SIGSEGV
    );
}

// -------- Category 4: stat struct layout --------
fn cat04_stat() {
    let path = std::env::temp_dir();
    match fs::metadata(&path) {
        Ok(m) => {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            println!(
                "CAT04 stat path={:?} len={} is_dir={} readonly={} mtime_unix={:?}",
                path,
                m.len(),
                m.is_dir(),
                m.permissions().readonly(),
                mtime
            );
        }
        Err(e) => println!(
            "CAT04 stat err kind={:?} raw={:?}",
            e.kind(),
            e.raw_os_error()
        ),
    }

    // stat on a symlink — exercises lstat vs stat divergence
    let link = std::env::temp_dir().join("probe-symlink");
    let _ = fs::remove_file(&link);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let _ = symlink("/nonexistent/target", &link);
        match fs::symlink_metadata(&link) {
            Ok(m) => println!(
                "CAT04 lstat file_type_is_symlink={}",
                m.file_type().is_symlink()
            ),
            Err(e) => println!("CAT04 lstat err kind={:?}", e.kind()),
        }
        let _ = fs::remove_file(&link);
    }
}

// -------- Category 5: sockets --------
// Biggest divergence surface. Exercise: bind, SO_REUSEADDR (setsockopt numbers
// differ), nonblocking (fcntl vs ioctl), UDP, TCP connect to self.
fn cat05_sockets() {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => {
            println!(
                "CAT05 sockets bind_err kind={:?} raw={:?}",
                e.kind(),
                e.raw_os_error()
            );
            return;
        }
    };
    let addr = listener.local_addr().unwrap();
    println!("CAT05 sockets bind ok addr={addr}");

    // The earlier thread-based TCP roundtrip test was removed because on
    // Windows cosmo the cross-thread TcpListener/accept path hangs (likely
    // clock_nanosleep + Windows WSA interaction; not pinned). Leaving
    // "bind ok" + the UDP bind below as the CAT05 signal, plus CAT13's
    // flag-constant check.
    drop(listener);

    // UDP — different syscall shape
    match UdpSocket::bind("127.0.0.1:0") {
        Ok(u) => {
            let a = u.local_addr().unwrap();
            println!("CAT05 sockets udp_bind ok addr={a}");
        }
        Err(e) => println!(
            "CAT05 sockets udp_bind_err kind={:?} raw={:?}",
            e.kind(),
            e.raw_os_error()
        ),
    }
}

// -------- Category 6: threading, TLS, mutex, condvar --------
fn cat06_threads() {
    let counter = Arc::new(Mutex::new(0usize));
    let mut handles = vec![];
    for _ in 0..8 {
        let c = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            for _ in 0..1000 {
                *c.lock().unwrap() += 1;
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    let final_val = *counter.lock().unwrap();
    println!(
        "CAT06 threads mutex_final={} expected=8000 ok={}",
        final_val,
        final_val == 8000
    );

    // Thread-local storage exercises pthread_key_* which Justine added stubs for
    thread_local! {
        static TLS: std::cell::RefCell<u32> = const { std::cell::RefCell::new(0) };
    }
    let t = thread::spawn(|| {
        TLS.with(|v| *v.borrow_mut() = 42);
        TLS.with(|v| *v.borrow())
    });
    println!("CAT06 threads tls_value={:?}", t.join());
}

// -------- Category 7: process spawn --------
// Hugely divergent: posix_spawn vs fork+exec vs CreateProcess.
fn cat07_process() {
    // Portable-ish: `echo` exists everywhere, even Windows cmd has it.
    // But argv parsing is different on Windows.
    let out = Command::new("echo").arg("probe-spawn").output();
    match out {
        Ok(o) => println!(
            "CAT07 process echo status={:?} stdout={:?} stderr_len={}",
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
            o.stderr.len()
        ),
        Err(e) => println!(
            "CAT07 process echo_err kind={:?} raw={:?}",
            e.kind(),
            e.raw_os_error()
        ),
    }

    // Deliberate failure — binary that doesn't exist
    let out = Command::new("/definitely/not/a/binary/probe").output();
    match out {
        Ok(_) => println!("CAT07 process missing_bin=unexpected_success"),
        Err(e) => println!(
            "CAT07 process missing_bin kind={:?} raw={:?}",
            e.kind(),
            e.raw_os_error()
        ),
    }
}

// -------- Category 8: env vars incl non-ASCII --------
fn cat08_env() {
    let key = "PROBE_TEST_VAR";
    // Unicode value — some OSes handle this via UTF-16 (Windows) under the hood
    let value = "hello-世界-🌍";
    // SAFETY: single-threaded context at this point in the probe.
    unsafe {
        std::env::set_var(key, value);
    }
    let got = std::env::var(key);
    println!(
        "CAT08 env set_get match={} got={:?}",
        got.as_deref() == Ok(value),
        got
    );

    // PATH always exists
    let path_len = std::env::var_os("PATH").map(|v| v.len()).unwrap_or(0);
    println!("CAT08 env path_len={path_len}");

    // Count total env vars — sanity
    let n = std::env::vars_os().count();
    println!("CAT08 env total_vars={n}");
}

// -------- Category 9: filesystem timestamps --------
fn cat09_fs_times() {
    let path = std::env::temp_dir().join("probe-times-test");
    let _ = fs::write(&path, b"x");

    match fs::metadata(&path) {
        Ok(m) => {
            let mt = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64());
            let at = m
                .accessed()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64());
            let ct = m
                .created()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64());
            println!("CAT09 fs_times modified={mt:?} accessed={at:?} created={ct:?}");
        }
        Err(e) => println!("CAT09 fs_times err kind={:?}", e.kind()),
    }
    let _ = fs::remove_file(&path);
}

// -------- Category 10: CStr / OsStr with non-UTF-8 --------
fn cat10_cstr_osstr() {
    let cs = CString::new("hello\0world"); // contains interior NUL → Err
    println!("CAT10 cstr interior_nul_is_err={}", cs.is_err());

    let cs2 = CString::new("portable").unwrap();
    let round: &CStr = cs2.as_c_str();
    println!(
        "CAT10 cstr roundtrip_bytes_with_nul={}",
        round.to_bytes_with_nul().len()
    );

    // Non-UTF-8 OsStr on unix — on Windows OsStr is WTF-8, bytes API is unix-only
    #[cfg(unix)]
    {
        let bad = OsString::from_vec(vec![0xFF, 0xFE, 0x41, 0x42]);
        let as_bytes = bad.as_os_str().as_bytes();
        println!(
            "CAT10 osstr non_utf8 len={} first_byte={:#x}",
            as_bytes.len(),
            as_bytes[0]
        );
    }
    #[cfg(not(unix))]
    {
        println!("CAT10 osstr non_utf8=skipped_non_unix");
    }
}

// -------- Category 11: panic + backtrace --------
// Tests the libunwind stubs Justine added. Output depends on
// RUST_BACKTRACE env var which we set explicitly.
fn cat11_panic_backtrace() {
    // SAFETY: single-threaded context at this point in the probe.
    unsafe {
        std::env::set_var("RUST_BACKTRACE", "1");
    }
    // Suppress the default panic hook just for this intentional panic so its
    // "thread 'main' panicked at ..." stderr noise doesn't interleave with the
    // probe's structured stdout output.
    let prev_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(|| {
        panic!("probe-intentional-panic");
    });
    panic::set_hook(prev_hook);
    println!("CAT11 panic caught_is_err={}", result.is_err());
    match result {
        Ok(()) => {}
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                "<unknown panic payload>".to_string()
            };
            println!("CAT11 panic payload={msg:?}");
        }
    }
}

// -------- Category 13: Strategy 2 proof-of-mechanism --------
//
// Cosmopolitan declares divergent runtime constants as
// `extern const int` (see include/libc/sysv/consts/*.h and
// include/libc/errno.h), populated per-OS at load time by
// libcosmo. Rust-libc baked-in values shadow these; std
// then sees the Linux compile-time value on every OS.
//
// This category declares the same symbols via Rust's `extern
// "C" { static X: c_int; }` and prints them next to
// `libc::X`. If Strategy 2 is feasible, the two columns
// should differ on non-Linux targets where cosmo has a
// non-Linux runtime value.
//
// Symbols chosen because they're the ones the probe tripped
// over in CAT01/CAT02/CAT05/CAT12.
unsafe extern "C" {
    static CLOCK_MONOTONIC: libc::c_int;
    static EINVAL: libc::c_int;
    static SOCK_CLOEXEC: libc::c_int;
    static O_CLOEXEC: libc::c_int;
}

fn cat13_extern_static() {
    // SAFETY: each symbol is a well-known `extern const int` in
    // libcosmo, always initialised by the loader before `main`.
    unsafe {
        println!(
            "CAT13 clock_monotonic libc={} extern={} match={}",
            libc::CLOCK_MONOTONIC,
            CLOCK_MONOTONIC,
            libc::CLOCK_MONOTONIC == CLOCK_MONOTONIC
        );
        println!(
            "CAT13 einval libc={} extern={} match={}",
            libc::EINVAL,
            EINVAL,
            libc::EINVAL == EINVAL
        );
        println!(
            "CAT13 sock_cloexec libc={} extern={} match={}",
            libc::SOCK_CLOEXEC,
            SOCK_CLOEXEC,
            libc::SOCK_CLOEXEC == SOCK_CLOEXEC
        );
        println!(
            "CAT13 o_cloexec libc={} extern={} match={}",
            libc::O_CLOEXEC,
            O_CLOEXEC,
            libc::O_CLOEXEC == O_CLOEXEC
        );
    }
}

// -------- Category 12: time --------
fn cat12_time() {
    let a = SystemTime::now();
    thread::sleep(Duration::from_millis(10));
    let b = SystemTime::now();
    let delta = b.duration_since(a).map(|d| d.as_millis()).unwrap_or(0);
    println!("CAT12 time systemtime_delta_ms={delta}");

    let inst_a = std::time::Instant::now();
    thread::sleep(Duration::from_millis(10));
    let inst_delta = inst_a.elapsed().as_millis();
    println!("CAT12 time instant_delta_ms={inst_delta}");
}

fn main() {
    // Don't let a bug in one category kill the probe.
    let cats: &[(&str, fn())] = &[
        ("01_errno", cat01_errno),
        ("02_open_flags", cat02_open_flags),
        ("03_signals", cat03_signals),
        ("04_stat", cat04_stat),
        ("05_sockets", cat05_sockets),
        ("06_threads", cat06_threads),
        ("07_process", cat07_process),
        ("08_env", cat08_env),
        ("09_fs_times", cat09_fs_times),
        ("10_cstr_osstr", cat10_cstr_osstr),
        ("11_panic_backtrace", cat11_panic_backtrace),
        ("12_time", cat12_time),
        ("13_extern_static", cat13_extern_static),
    ];

    header();

    for (name, f) in cats {
        let r = panic::catch_unwind(panic::AssertUnwindSafe(f));
        if r.is_err() {
            println!("CAT?? {name} panicked_uncaught");
        }
    }

    println!("=== probe complete ===");
}

// --- Cosmo shim: __xpg_strerror_r ---
//
// Rust std on target_os="linux" renames its `strerror_r` FFI declaration to
// `__xpg_strerror_r` via #[cfg_attr(..., link_name = "__xpg_strerror_r")]
// (see library/std/src/sys/io/error/unix.rs). The XSI contract is: return 0
// on success, a positive errno (EINVAL/ERANGE) on failure — never negative.
//
// cosmocc 4.0.2 ships __xpg_strerror_r but under some inputs it returns
// negative values, so std's `if strerror_r(...) < 0 { panic!("strerror_r
// failure"); }` fires as soon as any io::Error is formatted (the first call
// site is CAT01). Shim over the top with a thin strerror()-based wrapper.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __xpg_strerror_r(
    errnum: libc::c_int,
    buf: *mut libc::c_char,
    buflen: libc::size_t,
) -> libc::c_int {
    if buf.is_null() || buflen == 0 {
        return libc::EINVAL;
    }
    // SAFETY: strerror returns a pointer to a static string, never null on cosmo.
    let s = unsafe { libc::strerror(errnum) };
    if s.is_null() {
        return libc::EINVAL;
    }
    let src_len = unsafe { libc::strlen(s) };
    let copy_len = core::cmp::min(src_len, buflen.saturating_sub(1));
    unsafe {
        core::ptr::copy_nonoverlapping(s, buf, copy_len);
        *buf.add(copy_len) = 0;
    }
    0
}

// --- Cosmo shim: waitid ---
//
// Current nightly std (rustc 1.97.0-nightly) calls `libc::waitid(P_PIDFD, ...)`
// unconditionally in std::sys::pal::unix::linux::pidfd when spawning processes.
// Cosmopolitan libc 4.0.2 ships `wait4` and `waitpid` but not `waitid`, so the
// link fails with `undefined reference to waitid`. We provide a shim that
// dispatches via `syscall()` — cosmo's syscall() does the per-OS mapping, so
// this works on any OS cosmo supports.
//
// This is noted as an investigation finding, not a permanent solution: the
// probe needs to build in order to measure divergence, and Command::new is
// a category we explicitly want to probe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn waitid(
    idtype: libc::c_int,
    id: libc::c_uint,
    infop: *mut libc::siginfo_t,
    options: libc::c_int,
) -> libc::c_int {
    // SAFETY: syscall() is the cosmopolitan polyglot syscall dispatcher.
    // Linux waitid takes (idtype, id, infop, options, rusage*). We pass NULL
    // for rusage; std doesn't use it.
    unsafe {
        libc::syscall(
            libc::SYS_waitid,
            idtype,
            id as libc::c_long,
            infop,
            options,
            0 as libc::c_long,
        ) as libc::c_int
    }
}
