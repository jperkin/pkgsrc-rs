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
 * Rust interface to the pkg_install database and the pkgsrc mk
 * infrastructure.
 */

#![deny(missing_docs)]

/*
 * Modules that deserve their own namespace.
 */
pub mod digest;
pub mod distinfo;
pub mod pkgdb;
pub mod plist;
pub mod summary;

/*
 * Modules that are available in the root.
 */
mod depend;
mod dewey;
mod metadata;
mod pattern;
mod pkgname;
mod pkgpath;
mod scanindex;

pub use crate::depend::{Depend, DependError, DependType};
pub use crate::dewey::{Dewey, DeweyError};
pub use crate::metadata::{Metadata, MetadataEntry};
pub use crate::pattern::{Pattern, PatternError};
pub use crate::pkgname::PkgName;
pub use crate::pkgpath::{PkgPath, PkgPathError};
pub use crate::scanindex::ScanIndex;
