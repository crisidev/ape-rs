# Rustup-sysroot patches for the Strategy-2 prototype

Strategy 2 (`extern static` for divergent libc constants) requires
changes to **rustup-managed source files** that aren't in this
repository. This directory holds both:

1. The mechanical artifacts: real `.patch` files and wrapper
   scripts (`apply.sh` / `revert.sh`).
2. The narrative below, which explains *why* each patch exists. When
   a nightly drifts and a hunk fails to apply, fall back to the
   narrative; the explanations are keyed off libc identifiers, not
   line numbers, so they stay valid under modest churn.

* [Why these patches are necessary](#why-these-patches-are-necessary)
* [Layout](#layout)
* [Nightly targeted](#nightly-targeted)
* [Apply](#apply)
* [Revert](#revert)
* [Regenerating the patches](#regenerating-the-patches)
* [What this is / isn't](#what-this-is-/-isn't)

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
