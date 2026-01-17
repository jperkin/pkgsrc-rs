/*
 * Copyright (c) 2026 Jonathan Perkin <jonathan@perkin.org.uk>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*!
 * # pkgsrc
 *
 * [pkgsrc](https://www.pkgsrc.org/) is a cross-platform package management
 * system originally developed for NetBSD, now available on many Unix-like
 * operating systems.  This crate provides types and utilities for working
 * with pkgsrc packages, the package database, and pkgsrc infrastructure.
 *
 * It is used by [bob](https://github.com/jperkin/bob), a pkgsrc package
 * builder, and [mktool](https://github.com/jperkin/mktool), a collection of
 * fast alternate implementations for various pkgsrc/mk scripts.
 *
 * ## Modules
 *
 * The crate is organised into modules that handle different aspects of pkgsrc:
 *
 * | Module | Purpose |
 * |--------|---------|
 * | [`archive`] | Read and create binary package archives |
 * | [`depend`] | Parse and match package dependencies |
 * | [`dewey`] | Dewey decimal version comparisons |
 * | [`digest`] | Cryptographic hash functions for file verification |
 * | [`distinfo`] | Parse and verify distinfo files |
 * | [`kv`] | Parse KEY=VALUE formatted data |
 * | [`metadata`] | Read package metadata from `+*` files |
 * | [`pattern`] | Match packages against glob, dewey, and alternate patterns |
 * | [`pkgdb`] | Access the installed package database |
 * | [`pkgname`] | Parse package names into name and version components |
 * | [`pkgpath`] | Parse pkgsrc package paths (category/package) |
 * | [`plist`] | Parse packing list (PLIST) files |
 * | [`scanindex`] | Parse pbulk-index scan output |
 * | [`summary`] | Parse [`pkg_summary(5)`] files |
 *
 * ## Examples
 *
 * Read an installed package's metadata from the package database:
 *
 * ```no_run
 * use pkgsrc::metadata::FileRead;
 * use pkgsrc::PkgDB;
 *
 * let db = PkgDB::open("/var/db/pkg")?;
 * for entry in db {
 *     let pkg = entry?;
 *     println!("{}: {}", pkg.pkgname(), pkg.comment()?);
 * }
 * # Ok::<(), std::io::Error>(())
 * ```
 *
 * Extract files from a binary package:
 *
 * ```no_run
 * use pkgsrc::Archive;
 *
 * let mut archive = Archive::open("/path/to/package.tgz")?;
 * for entry in archive.entries()? {
 *     let entry = entry?;
 *     println!("{}", entry.path()?.display());
 * }
 * # Ok::<(), Box<dyn std::error::Error>>(())
 * ```
 *
 * Parse a pkg_summary file to enumerate available packages:
 *
 * ```no_run
 * use pkgsrc::summary::Summary;
 * use std::fs::File;
 * use std::io::BufReader;
 *
 * let file = File::open("pkg_summary")?;
 * let reader = BufReader::new(file);
 * for entry in Summary::from_reader(reader) {
 *     let pkg = entry?;
 *     println!("{}: {}", pkg.pkgname(), pkg.comment());
 * }
 * # Ok::<(), Box<dyn std::error::Error>>(())
 * ```
 *
 * Match packages using patterns:
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * let pattern = Pattern::new("perl>=5.30")?;
 * assert!(pattern.matches("perl-5.38.0"));
 * assert!(!pattern.matches("perl-5.28.0"));
 * # Ok::<(), pkgsrc::PatternError>(())
 * ```
 *
 * ## Feature Flags
 *
 * - `serde`: Enable serialization and deserialization support via
 *   [serde](https://serde.rs/) for various types.
 *
 * [`pkg_summary(5)`]: https://man.netbsd.org/pkg_summary.5
 */

#![deny(missing_docs)]

extern crate self as pkgsrc;

pub mod archive;
pub mod depend;
pub mod dewey;
pub mod digest;
pub mod distinfo;
pub mod kv;
pub mod metadata;
pub mod pattern;
pub mod pkgdb;
pub mod pkgname;
pub mod pkgpath;
pub mod plist;
pub mod scanindex;
pub mod summary;

pub use crate::archive::Archive;
pub use crate::depend::{Depend, DependError, DependType};
pub use crate::dewey::{Dewey, DeweyError};
pub use crate::digest::Digest;
pub use crate::distinfo::Distinfo;
pub use crate::metadata::Metadata;
pub use crate::pattern::{Pattern, PatternError};
pub use crate::pkgdb::{DBType, PkgDB};
pub use crate::pkgname::PkgName;
pub use crate::pkgpath::{PkgPath, PkgPathError};
pub use crate::plist::Plist;
pub use crate::scanindex::{ScanIndex, ScanIndexIter};
pub use crate::summary::Summary;
