# pkgsrc-rs

[![Downloads](https://img.shields.io/crates/d/pkgsrc.svg)](https://crates.io/crates/pkgsrc)
[![Crates.io](https://img.shields.io/crates/v/pkgsrc.svg)](https://crates.io/crates/pkgsrc)
[![Documentation](https://docs.rs/pkgsrc/badge.svg)](https://docs.rs/pkgsrc)
[![License](https://img.shields.io/crates/l/pkgsrc.svg)](https://github.com/jperkin/pkgsrc-rs)

A Rust interface to the pkgsrc infrastructure, binary package archives, and the
pkg\_install pkgdb.

This is being developed alongside:

 * [bob](https://github.com/jperkin/bob), a pkgsrc package builder.
 * [mktool](https://github.com/jperkin/mktool), a collection of tools that
   provide fast alternate implementations for various pkgsrc/mk scripts.
 * [pm](https://github.com/jperkin/pm), an exploration of what a binary package
   manager might look like (not currently being developed).

You should expect things to change over time as each interface adapts to better
support these utilities, though I will still make sure to use semver versioning
accordingly to avoid gratuitously breaking downstream utilities.

## Example

This is a simple implementation of `pkg_info(8)` that supports the default
output format, i.e. list all currently installed packages and their single-line
comment.

```rust
use anyhow::Result;
use pkgsrc::metadata::FileRead;
use pkgsrc::pkgdb::PkgDB;

fn main() -> Result<()> {
    let pkgdb = PkgDB::open("/var/db/pkg")?;

    for pkg in pkgdb {
        let pkg = pkg?;
        println!("{:<19} {}", pkg.pkgname(), pkg.comment()?);
    }

    Ok(())
}
```

See [`examples/pkg_info.rs`](https://github.com/jperkin/pkgsrc-rs/blob/master/examples/pkg_info.rs)
for a more complete implementation.

## Features

* [`archive`](https://docs.rs/pkgsrc/latest/pkgsrc/archive/): Read and write
  binary packages, supporting both unsigned (compressed tarballs) and signed
  (`ar(1)` archives with GPG signatures) formats. Includes low-level streaming
  API and high-level `Package` type for fast metadata access.
* [`digest`](https://docs.rs/pkgsrc/latest/pkgsrc/digest/): Cryptographic
  hashing using BLAKE2s, MD5, RMD160, SHA1, SHA256, and SHA512, with special
  handling for pkgsrc patch files.
* [`distinfo`](https://docs.rs/pkgsrc/latest/pkgsrc/distinfo/): Parse and
  process `distinfo` files containing checksums for distfiles and patches.
* [`kv`](https://docs.rs/pkgsrc/latest/pkgsrc/kv/): Key-value parsing utilities.
* [`pkgdb`](https://docs.rs/pkgsrc/latest/pkgsrc/pkgdb/): Handle local pkg
  databases, supporting the regular file-backed repository.
* [`plist`](https://docs.rs/pkgsrc/latest/pkgsrc/plist/): Parse and generate
  packing lists (`PLIST` files) with support for all `@` commands.
* [`summary`](https://docs.rs/pkgsrc/latest/pkgsrc/summary/): Parse and generate
  `pkg_summary(5)` metadata with full validation and span-aware error reporting.
* [`Pattern`](https://docs.rs/pkgsrc/latest/pkgsrc/struct.Pattern.html),
  [`Depend`](https://docs.rs/pkgsrc/latest/pkgsrc/struct.Depend.html),
  [`Dewey`](https://docs.rs/pkgsrc/latest/pkgsrc/struct.Dewey.html): Package
  matching with `pkg_match()` semantics, verified correct against a large corpus
  of real-world matches.

## MSRV

The current requirements are:

* `edition = "2024"`
* `rust-version = "1.85.1"`

# License

This project is licensed under the [ISC](https://opensource.org/licenses/ISC) license.
