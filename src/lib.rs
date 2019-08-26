/*
 * Copyright (c) 2019 Jonathan Perkin <jonathan@perkin.org.uk>
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
 * Implementation of pkg_install library and database routines in Rust.
 *
 * ## Goals
 *
 * The initial goals are to fully support existing pkg_install packages and the
 * files-based pkgdb, in tandem with developing
 * [pm](https://github.com/jperkin/pm), a Rust alternative to pkg_install and
 * pkgin.
 *
 * After that the aim is to replace the fragile and slow pkgdb backend with
 * sqlite which should provide much faster and more reliable queries and
 * updates.
 */

#![deny(missing_docs)]

pub use crate::metadata::{Metadata, MetadataEntry};
pub use crate::plist::Plist;
pub use crate::pmatch::pkg_match;
pub use crate::summary::{SummaryEntry, SummaryStream};

mod metadata;
mod plist;
mod pmatch;
mod summary;
