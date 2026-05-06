# Rustup-sysroot patches for the Strategy-2 prototype

Strategy 2 (`extern static` for divergent libc constants) requires
changes to **rustup-managed source files** that aren't in this
repository. This directory holds both:

1. The mechanical artifacts — real `.patch` files and wrapper
   scripts (`apply.sh` / `revert.sh`).
2. The narrative below, which explains *why* each patch exists. When
   a nightly drifts and a hunk fails to apply, fall back to the
   narrative; the explanations are keyed off libc identifiers, not
   line numbers, so they stay valid under modest churn.

## Why these patches are necessary

`[patch.crates-io]` in the project `Cargo.toml` does **not** propagate
to std's `libc` dep when using `-Z build-std`. Without touching std's
own `Cargo.toml`, std keeps pulling the stock libc from the registry,
so the extern-static overrides in `libc-cosmo/` only apply to our
probe crate — not to std itself. For Strategy 2 to cover the `libc::*`
uses *inside std* (which is where the actual portability failures
live), we edit rustup's std Cargo.toml to point its libc dep at our
fork.

Once std resolves libc to our patched fork, a handful of std source
files fail to compile because they use `libc::X` where X is now an
`extern static`, which can't appear in const expressions or match
patterns. Those need small rewrites.

## Layout

```
patches/
├── apply.sh          # applies all patches to a sysroot
├── revert.sh         # reverts via `patch -R`
├── std/              # one patch per modified std source file
│   ├── Cargo.patch
│   ├── src--os--unix--net--ancillary.patch
│   ├── src--os--unix--net--stream.patch
│   ├── src--sys--fs--unix--dir.patch
│   ├── src--sys--fs--unix.patch
│   ├── src--sys--io--error--unix.patch
│   ├── src--sys--pal--unix--futex.patch
│   ├── src--sys--pal--unix--sync--condvar.patch
│   ├── src--sys--process--unix--common.patch
│   ├── src--sys--random--linux.patch
│   ├── src--sys--thread--unix.patch
│   └── src--sys--time--unix.patch
└── README.md         # (this file)
```

The patches are unified diffs with `a/library/std/...` paths. `apply.sh`
targets `$SYSROOT/lib/rustlib/src/rust/library` and strips with `-p2`
(both `a/` and `library/`), so each hunk resolves to `std/...` inside
the target tree.

The Cargo.toml patch contains a literal `@LIBC_COSMO_PATH@` placeholder
for the libc dep path; `apply.sh` substitutes the real path at apply
time so the patch file itself stays machine-independent.

## Nightly targeted

```
rustc 1.97.0-nightly (913e4bea8 2026-04-22)
```

`$STD_ROOT` below always refers to:

```
$SYSROOT/lib/rustlib/src/rust/library
```

where `$SYSROOT=$(rustc +nightly --print sysroot)`.

## Apply

```
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly

# Defaults: active nightly + ../libc-cosmo
./apply.sh

# Or explicit paths:
./apply.sh ~/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library /abs/path/to/libc-cosmo
```

## Revert

```
./revert.sh
```

Same optional args as `apply.sh`; uses `patch -R`. If drift makes
revert fail, reinstall a pristine std source:

```
rustup component remove rust-src --toolchain nightly
rustup component add rust-src --toolchain nightly
```

## Regenerating the patches

If you modify the std sources further:

```
# Fetch originals for files that don't have local .orig backups.
# Swap the commit hash for your nightly's (visible in `rustc --version`).
COMMIT=913e4bea8
for f in library/std/src/sys/fs/unix/dir.rs library/std/src/sys/fs/unix.rs \
         library/std/src/os/unix/net/ancillary.rs library/std/src/sys/time/unix.rs; do
  mkdir -p "/tmp/std-orig/$(dirname $f)"
  curl -fsSL "https://raw.githubusercontent.com/rust-lang/rust/$COMMIT/$f" -o "/tmp/std-orig/$f"
done

# Files with a .orig / .orig-cosmo backup in the sysroot can be diffed
# against those directly. Others diff against /tmp/std-orig.
diff -u --label a/library/std/<path> --label b/library/std/<path> <orig> <current> \
    > patches/std/<name>.patch
```

---

## Per-patch walkthrough

Each heading maps to one file in `std/`. The diffs shown here match
what's in the patch files — the Cargo.toml patch uses the
`@LIBC_COSMO_PATH@` placeholder that `apply.sh` substitutes at apply
time.

### Patch 1 — `std/Cargo.toml`: redirect libc to our fork

File: `Cargo.patch` · Target: `$STD_ROOT/std/Cargo.toml` · Reason: make
std's `libc` dep resolve to our patched path so `-Z build-std` builds
it with our cfg(cosmo)-gated extern statics.

```diff
 [target.'cfg(not(all(windows, target_env = "msvc")))'.dependencies]
-libc = { version = "0.2.185", default-features = false, features = [
-    'rustc-dep-of-std',
-], public = true }
+libc = { path = "@LIBC_COSMO_PATH@", default-features = false, features = ["rustc-dep-of-std"], public = true }
```

### Patch 2 — `sys/fs/unix/dir.rs`: wrap `libc::O_CLOEXEC | O_DIRECTORY` in `unsafe`

File: `src--sys--fs--unix--dir.patch` · Reason: under cfg(cosmo)
`libc::O_CLOEXEC` and `libc::O_DIRECTORY` are `extern static`, so
reading them requires `unsafe`. Two edits in this file:

```diff
 pub fn open_with_c(path: &CStr, opts: &OpenOptions) -> io::Result<Self> {
-    let flags = libc::O_CLOEXEC
-        | libc::O_DIRECTORY
+    // SAFETY (cosmo patch): libc::O_CLOEXEC / O_DIRECTORY are extern
+    // statics populated at load time by libcosmo. Reading is always safe.
+    let flags = unsafe { libc::O_CLOEXEC | libc::O_DIRECTORY }
         | opts.get_access_mode()?
         | opts.get_creation_mode()?
         | (opts.custom_flags as c_int & !libc::O_ACCMODE);
```

```diff
 fn open_file_c(&self, path: &CStr, opts: &OpenOptions) -> io::Result<File> {
-    let flags = libc::O_CLOEXEC
+    // SAFETY (cosmo patch): libc::O_CLOEXEC is an extern static.
+    let flags = unsafe { libc::O_CLOEXEC }
         | opts.get_access_mode()?
         | opts.get_creation_mode()?
         | (opts.custom_flags as c_int & !libc::O_ACCMODE);
```

### Patch 3 — `sys/fs/unix.rs`: wrap `libc::O_CLOEXEC` in `unsafe`

File: `src--sys--fs--unix.patch` · Reason: same as Patch 2.

```diff
 pub fn open_c(path: &CStr, opts: &OpenOptions) -> io::Result<File> {
-    let flags = libc::O_CLOEXEC
+    // SAFETY (cosmo patch): libc::O_CLOEXEC is an extern static.
+    let flags = unsafe { libc::O_CLOEXEC }
         | opts.get_access_mode()?
         | opts.get_creation_mode()?
         | (opts.custom_flags as c_int & !libc::O_ACCMODE);
```

### Patch 4 — `os/unix/net/ancillary.rs`: rewrite match on SOL_SOCKET

File: `src--os--unix--net--ancillary.patch` · Reason: `libc::SOL_SOCKET`
is `extern static` under cfg(cosmo); can't be used as a match pattern.
Also two direct uses need `unsafe`.

(a) Rewrite `match (*cmsg).cmsg_level { libc::SOL_SOCKET => … }` as
`if cmsg_level == libc::SOL_SOCKET { … } else { … }` (preserving
existing target_os-gated SCM_CREDENTIALS/SCM_CREDS arms).

(b) In `add_fds` and `add_creds`, hoist `libc::SOL_SOCKET` into an
unsafe let-binding:

```diff
 pub fn add_fds(&mut self, fds: &[RawFd]) -> bool {
     self.truncated = false;
+    // SAFETY (cosmo patch): libc::SOL_SOCKET is extern static.
+    let sol_socket = unsafe { libc::SOL_SOCKET };
     add_to_ancillary_data(
         &mut self.buffer,
         &mut self.length,
         fds,
-        libc::SOL_SOCKET,
+        sol_socket,
         libc::SCM_RIGHTS,
     )
 }
```

### Patch 5 — `sys/pal/unix/futex.rs`: unsafe binding + reuse

File: `src--sys--pal--unix--futex.patch` · Reason:
`libc::CLOCK_MONOTONIC` is extern static; two uses in this function.

```diff
+    // SAFETY (cosmo patch): libc::CLOCK_MONOTONIC is extern static.
+    let clock_monotonic = unsafe { libc::CLOCK_MONOTONIC };
     let timespec = timeout
-        .and_then(|d| Timespec::now(libc::CLOCK_MONOTONIC).checked_add_duration(&d))
+        .and_then(|d| Timespec::now(clock_monotonic).checked_add_duration(&d))
         .and_then(|t| t.to_timespec());
```

```diff
                 let umtx_timeout = timespec.map(|t| libc::_umtx_time {
                     _timeout: t,
                     _flags: libc::UMTX_ABSTIME,
-                    _clockid: libc::CLOCK_MONOTONIC as u32,
+                    // cosmo patch: libc::CLOCK_MONOTONIC is extern static.
+                    _clockid: clock_monotonic as u32,
                 });
```

### Patch 6 — `sys/pal/unix/sync/condvar.rs`: const → fn

File: `src--sys--pal--unix--sync--condvar.patch` · Reason: can't use
extern static in a `const` item initializer.

```diff
 impl Condvar {
     pub const PRECISE_TIMEOUT: bool = true;
-    const CLOCK: libc::clockid_t = libc::CLOCK_MONOTONIC;
+    // cosmo patch: libc::CLOCK_MONOTONIC is extern static.
+    #[allow(non_snake_case)]
+    fn CLOCK() -> libc::clockid_t {
+        unsafe { libc::CLOCK_MONOTONIC }
+    }
```

Plus two call-site updates from `Self::CLOCK` to `Self::CLOCK()`.

### Patch 7 — `sys/time/unix.rs`: const CLOCK_ID → fn clock_id() + unsafe

File: `src--sys--time--unix.patch` · Reason: same pattern as Patch 6
for `Instant::CLOCK_ID`, plus one unsafe for `SystemTime::now`.

```diff
 pub fn now() -> SystemTime {
-    SystemTime { t: Timespec::now(libc::CLOCK_REALTIME) }
+    // SAFETY (cosmo patch): libc::CLOCK_REALTIME is extern static.
+    SystemTime { t: Timespec::now(unsafe { libc::CLOCK_REALTIME }) }
 }
```

```diff
-    #[cfg(target_vendor = "apple")]
-    pub(crate) const CLOCK_ID: libc::clockid_t = libc::CLOCK_UPTIME_RAW;
-
-    #[cfg(not(target_vendor = "apple"))]
-    pub(crate) const CLOCK_ID: libc::clockid_t = libc::CLOCK_MONOTONIC;
-
-    pub fn now() -> Instant {
-        Instant { t: Timespec::now(Self::CLOCK_ID) }
-    }
+    // cosmo patch: CLOCK_ID was `const`, but under target_env="cosmo"
+    // libc::CLOCK_MONOTONIC is `extern static` which can't appear in a
+    // const expression. Turned into a function that reads the static at
+    // runtime.
+    #[cfg(target_vendor = "apple")]
+    pub(crate) fn clock_id() -> libc::clockid_t {
+        unsafe { libc::CLOCK_UPTIME_RAW }
+    }
+
+    #[cfg(not(target_vendor = "apple"))]
+    pub(crate) fn clock_id() -> libc::clockid_t {
+        unsafe { libc::CLOCK_MONOTONIC }
+    }
+
+    pub fn now() -> Instant {
+        Instant { t: Timespec::now(Self::clock_id()) }
+    }
```

### Patch 8 — `sys/thread/unix.rs`: `Instant::CLOCK_ID` → `Instant::clock_id()`

File: `src--sys--thread--unix.patch` · Reason: three call sites of
`crate::sys::time::Instant::CLOCK_ID`; after Patch 7 they need to be
`Instant::clock_id()`.

### Patch 10 — `sys/io/error/unix.rs`: route errno decode via cosmo's extern-const symbols

File: `src--sys--io--error--unix.patch` · Reason: **the one that
closes F-002.** cosmo doesn't normalise the numeric value of `errno`
across host OSes — on macOS/FreeBSD/OpenBSD/Windows the thread-local
`errno` holds the *native* OS's number, but `libc::E*` in a
Linux-musl build are pinned to Linux's compile-time values. So
std's `match errno { libc::EAGAIN => WouldBlock, … }` mis-classifies
almost every error on non-Linux hosts (Mac errno 35 = EAGAIN gets
labelled `Deadlock` because Linux's EDEADLK is 35).

Cosmo's `libc/errno.h` declares every errno name as
`extern const errno_t X` populated at load time with the host OS's
actual value. Under `cfg(cosmo)` we import those 38 symbols via
`#[link_name]`-aliased extern statics (named `COSMO_*` to avoid
conflicting with `libc::*`) and rewrite `decode_error_kind` as an
if/else cascade against them. `is_interrupted` gets the same
treatment for `EINTR`.

### Patch 11 — `sys/process/unix/common.rs`: `read_output` uses kind, not raw errno

File: `src--sys--process--unix--common.patch` · Reason: F-002 in
`wait_with_output`. The child-pipe poll loop checks
`e.raw_os_error() == Some(libc::EWOULDBLOCK | libc::EAGAIN)` — same
compile-time-baked comparison that breaks on Mac/BSD. Since
Patch 10 already makes `ErrorKind::WouldBlock` land correctly for
the host OS's EAGAIN, switch this check to `e.kind() ==
ErrorKind::WouldBlock` under `cfg(cosmo)`. This is what finally
unblocks `Command::new(…).output()` on Mac/FreeBSD/OpenBSD (was
panicking at `process/mod.rs:67` because `read_output` surfaced the
mis-classified error through `.unwrap()`).

### Patch 12 — `os/unix/net/stream.rs`: unsafe-wrap MSG_NOSIGNAL

File: `src--os--unix--net--stream.patch` · Reason: this is the
companion to `libc-cosmo`'s `MSG_NOSIGNAL` extern-static gate.
Windows's Winsock doesn't understand `MSG_NOSIGNAL` (a Linux-ism
`0x4000`); cosmo rejects the flag with `EINVAL` on the Windows
host. Cosmo's `libc/sysv/consts/msg.h` already exports
`extern const int MSG_NOSIGNAL` (`0x4000` on Linux, `0` on
Windows), so making `libc::MSG_NOSIGNAL` a runtime extern-static
fixes every `send`/`sendto` call site automatically.

The only catch is that reading an extern static requires
`unsafe`. Most callers in std are already inside `unsafe { … }`
blocks wrapping `libc::sendto`, so they need no change. The one
outlier is `<&UnixStream as Write>::write`, which calls the safe
wrapper `send_with_flags(…, MSG_NOSIGNAL)` directly — wrap that
second arg in `unsafe { … }`. With `#[allow(unused_unsafe)]` the
patch is a no-op for non-cosmo builds.

### Patch 9 — `sys/random/linux.rs`: graceful fallback for unknown errno

File: `src--sys--random--linux.patch` · Reason: needed for ripgrep on
cosmo-Windows. Std's `getrandom` path panics if errno doesn't match one
of a few known Linux values (EINTR / EINVAL / EAGAIN / ENOSYS / EPERM).
Cosmo's Windows personality returns native Win32 error codes for
failed getrandom calls (cf. F-002). Under cfg(cosmo), fall back to
`/dev/urandom` silently instead of panicking.

```diff
                     libc::ENOSYS | libc::EPERM => {
                         GETRANDOM_AVAILABLE.store(false, Relaxed);
                         break;
                     }
-                    _ => panic!("failed to generate random data"),
+                    // cosmo patch: under cfg(cosmo), errno is the
+                    // native OS's numeric value (see F-002). Windows
+                    // in particular returns Win32 error codes that
+                    // don't match any Linux errno constant. Degrade
+                    // to /dev/urandom instead of panicking.
+                    #[cfg(cosmo)]
+                    _ => {
+                        GETRANDOM_AVAILABLE.store(false, Relaxed);
+                        break;
+                    }
+                    #[cfg(not(cosmo))]
+                    _ => panic!("failed to generate random data"),
```

---

## What this is / isn't

This is a **prototype** — the rustup edits touch toolchain files, so
they don't survive a `rustup update`, and moving to a new nightly
requires re-applying all 9 patches. That's acceptable for the
investigation because the intent is to prove Strategy 2 works, not to
ship a maintainable config.

The upstream version of this work would be:

1. A PR to `rust-lang/libc` adding `target_env = "cosmo"` (or a
   separate cfg) that gates the extern-static overrides (this repo's
   `libc-cosmo/` is the shape of that change).
2. A PR to `rust-lang/rust` to relax std's const-initializer uses
   (Patches 6/7) and unsafe-wrap the call sites (Patches 2/3/4/5) so
   std compiles against the modified libc without requiring downstream
   edits.
3. A PR to `jart/cosmopolitan` if any cosmo-side fixes are needed
   alongside — for our probe matrix, none are strictly required (the
   runtime extern-const values are already populated correctly).
