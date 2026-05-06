![ape-rs](ape-rs.png)

# ape-rs

Build a Rust binary as a [Cosmopolitan APE][cosmo] (Actually Portable
Executable). A single file that runs natively on Linux, macOS,
FreeBSD, OpenBSD, and Windows.

This branch contains the bare minimum to reproduce the build pipeline
end-to-end, from a "hello world" up to a partial-success async HTTP
client. Development history, journal, and findings live on a separate
branch in this repo.

For the full story, see the blog series
[*One Bin to Rule Them All*][blog].

[cosmo]: https://github.com/jart/cosmopolitan
[blog]: https://blog.crisidev.org/tags/series-one-bin-to-rule-them-all/

* [What's here](#what's-here)
* [Prerequisites](#prerequisites)
* [Reproduction](#reproduction)
    * [1. Clone with submodules](#1.-clone-with-submodules)
    * [2. Download cosmocc 4.0.2](#2.-download-cosmocc-4.0.2)
    * [3. Patch the rustup nightly's std sources](#3.-patch-the-rustup-nightly's-std-sources)
    * [4. Build a workload](#4.-build-a-workload)
        * [`rust-ape-example` — minimal "hello world"](#`rust-ape-example`-—-minimal-"hello-world")
        * [`probe` — portability probe](#`probe`-—-portability-probe)
        * [`ripgrep` — sync grep, full-fat workload](#`ripgrep`-—-sync-grep,-full-fat-workload)
        * [`dog` — DNS client (sync sockets)](#`dog`-—-dns-client-(sync-sockets))
        * [`xh` — async HTTP client (partial success)](#`xh`-—-async-http-client-(partial-success))
* [Running on each host](#running-on-each-host)
* [Cleaning up](#cleaning-up)
* [How the patches work](#how-the-patches-work)

## What's here

- `patches/`: patches applied to a rustup nightly's `std` sources so
  the standard library compiles against our cosmo-aware `libc`.
- `toolchain/fetch-cosmocc.sh`: downloads cosmocc 4.0.2.
- Submodules pointing at our forks of every crate that needed
  `cfg(cosmo)` shims:
  - libraries: `libc-cosmo`, `getrandom-cosmo`, `socket2-cosmo`,
    `mio-cosmo`, `tokio-cosmo`
  - workloads: `rust-ape-example`, `probe`, `ripgrep`, `dog`, `xh`

Pre-built fat APE binaries for each tagged release are attached to the
GitHub release page (built by `.github/workflows/release.yml`).

## Prerequisites

- Linux x86_64 build host.
- `rustup` with a `nightly` toolchain that has the `rust-src` component:
  ```
  rustup install nightly
  rustup +nightly component add rust-src
  ```
- `bash`, `curl` (or `wget`), `unzip`, `patch`, plus a working `cc` for
  the macOS APE loader.

## Reproduction

### 1. Clone with submodules

```bash
git clone --recursive https://github.com/crisidev/ape-rs.git
cd ape-rs
```

### 2. Download cosmocc 4.0.2

```bash
./toolchain/fetch-cosmocc.sh
export COSMO="$(pwd)/toolchain/cosmocc-4.0.2"
```

### 3. Patch the rustup nightly's std sources

```bash
./patches/apply.sh    # reversible — see patches/revert.sh
```

### 4. Build a workload

Each workload has its own `build-fat.sh` that produces a fat
x86_64+aarch64 APE. Pick one or build them all in turn:

#### `rust-ape-example` — minimal "hello world"

```bash
cd rust-ape-example && ./build-fat.sh --release && cd ..
# produces rust-ape-example/hello.com
```

The simplest reproducer. If this works, the toolchain and patches are
healthy.

#### `probe` — portability probe

```bash
cd probe && ./build-fat.sh --release && cd ..
# produces probe/probe.com
```

Structured probe of OS surfaces (errno, fs, sockets, threads, time,
process, panic). Designed to print one line per category — used to
sanity-check what `std` actually does on each of the six target hosts.

#### `ripgrep` — sync grep, full-fat workload

```bash
cd ripgrep && ./build-fat.sh --release && cd ..
# produces ripgrep/rg.com
```

Exercises filesystem syscalls, threading, regex, and a substantial
dependency tree. Runs on all six target hosts.

#### `dog` — DNS client (sync sockets)

```bash
cd dog && ./build-fat.sh --release && cd ..
# produces dog/dog.com
```

Adds raw socket I/O on top of the ripgrep baseline. TLS is disabled in
the cosmo build (`with_tls`/`with_https` features dropped) — `dog`
speaks plain DNS only. Runs on all six target hosts.

#### `xh` — async HTTP client (partial success)

```bash
cd xh && ./build-fat.sh --release && cd ..
# produces xh/xh.com
```

This is the hard one. `xh` pulls in the full `reqwest`+`tokio`+`mio`
async HTTP stack, which forced forks of `socket2`, `mio`, and `tokio`
for cosmo `unsafe { ... }` wraps around extern-static `libc`
constants, plus syscall renames so cosmo's `sys_eventfd2` /
`sys_epoll_pwait` get linked instead of glibc's `eventfd` /
`epoll_wait`. TLS is dropped (HTTP-only) — `aws-lc-sys` and `ring`
both refused to cross-compile through cosmocc.

The resulting `xh.com`:

- runs on **Linux x86_64 + Linux aarch64** (success)
- fails on **FreeBSD, OpenBSD, macOS, Windows** with `ENOSYS` from
  `eventfd`/`epoll_*` because cosmocc only emulates those Linux-isms
  on Linux hosts; mio's compile-time backend selection means
  switching reactors per host isn't possible from a single APE.

So `xh.com` is a single binary that *builds* and *runs* end-to-end on
Linux, but the async-IO reactor doesn't generalise off-Linux. The fork
patches and the partial result are intentionally part of this
reproducer.

## Running on each host

The same `.com` file works on all of them — no recompile, no
per-platform packaging.

| Host                          | Command                                                                                                                                                  |
|-------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------|
| Linux x86_64                  | `./hello.com`                                                                                                                                            |
| Linux aarch64                 | `./hello.com`                                                                                                                                            |
| FreeBSD 14                    | `./hello.com`                                                                                                                                            |
| OpenBSD 7.4                   | `./hello.com` *(partition needs `wxneeded`)*                                                                                                             |
| Windows Server 2022           | `hello.com`                                                                                                                                              |
| macOS arm64 (Apple M-series)  | `cc -O2 -o ape ape-m1.c && ./ape ./hello.com` *(one-time setup; `ape-m1.c` ships in the cosmocc release at `toolchain/cosmocc-4.0.2/bin/ape-m1.c`)*       |

For `xh.com` specifically, only the two Linux rows succeed.

## Cleaning up

```bash
./patches/revert.sh   # restore the rustup nightly's std sources
```

## How the patches work

The `cfg(cosmo)` rust flag (set in each workload's
`.cargo/config.toml`) toggles cosmo-aware code paths in the forked
crates and in the patched `std`. Most patches follow the same shape:
where upstream uses a `libc::FOO` constant whose value diverges
between Linux/macOS/BSD/Windows, the compile-time constant becomes a
runtime extern-static (`__cosmo_FOO`) that cosmocc resolves to the
correct host value at exec time.

See [patches/README.md](./patches/README.md) for more information.
