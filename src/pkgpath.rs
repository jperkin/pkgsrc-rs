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

/*! Package path (category/name) handling. */

use std::borrow::Borrow;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;

#[cfg(feature = "serde")]
use serde_with::{DeserializeFromStr, SerializeDisplay};

/**
 * An invalid path was specified trying to create a new [`PkgPath`].
 */
#[derive(Debug, Eq, Error, Ord, PartialEq, PartialOrd)]
pub enum PkgPathError {
    /**
     * Contains an invalid path.
     */
    #[error("Invalid path specified")]
    InvalidPath,
}

/**
 * Handling for `PKGPATH` metadata and relative package directory locations.
 *
 * [`PkgPath`] is a struct for storing the path to a package within pkgsrc.
 *
 * Binary packages contain the `PKGPATH` metadata, for example
 * `pkgtools/pkg_install`, while across pkgsrc dependencies are referred to by
 * their relative location, for example `../../pkgtools/pkg_install`.
 *
 * [`PkgPath`] takes either format as input, validates it for correctness,
 * then stores both internally as [`PathBuf`] entries.
 *
 * Once stored, [`as_path`] returns the short path as a [`Path`], while
 * [`as_full_path`] returns the full relative path as a [`Path`].
 *
 * As [`PkgPath`] uses [`PathBuf`] under the hood, there is a small amount of
 * normalisation performed, for example trailing or double slashes, but
 * otherwise input strings are expected to be precisely formatted, and a
 * [`PkgPathError`] is raised otherwise.
 *
 * ## Examples
 *
 * ```
 * use pkgsrc::PkgPath;
 * use std::ffi::OsStr;
 *
 * let p = PkgPath::new("pkgtools/pkg_install").unwrap();
 * assert_eq!(p.as_path(), OsStr::new("pkgtools/pkg_install"));
 * assert_eq!(p.as_full_path(), OsStr::new("../../pkgtools/pkg_install"));
 *
 * let p = PkgPath::new("../../pkgtools/pkg_install").unwrap();
 * assert_eq!(p.as_path(), OsStr::new("pkgtools/pkg_install"));
 * assert_eq!(p.as_full_path(), OsStr::new("../../pkgtools/pkg_install"));
 *
 * // Missing category path.
 * assert!(PkgPath::new("../../pkg_install").is_err());
 *
 * // Must traverse back to the pkgsrc root directory.
 * assert!(PkgPath::new("../pkg_install").is_err());
 *
 * // Not fully formed.
 * assert!(PkgPath::new("/pkgtools/pkg_install").is_err());;
 * ```
 *
 * [`as_full_path`]: PkgPath::as_full_path
 * [`as_path`]: PkgPath::as_path
 */
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(SerializeDisplay, DeserializeFromStr))]
pub struct PkgPath {
    short: PathBuf,
    full: PathBuf,
}

impl PkgPath {
    /**
     * Create a new PkgPath
     */
    pub fn new(path: &str) -> Result<Self, PkgPathError> {
        let p = PathBuf::from(path);
        let c: Vec<_> = p.components().collect();

        match c.len() {
            //
            // Handle the "category/package" case, adding "../../" to the full
            // PathBuf if the rest is valid.
            //
            2 => match (c[0], c[1]) {
                (Component::Normal(_), Component::Normal(_)) => {
                    let mut f = PathBuf::from("../../");
                    f.push(p.clone());
                    Ok(PkgPath { short: p, full: f })
                }
                _ => Err(PkgPathError::InvalidPath),
            },
            //
            // Handle the "../../category/package" case, removing "../../"
            // from the short PathBuf if it's valid.
            //
            4 => match (c[0], c[1], c[2], c[3]) {
                (
                    Component::ParentDir,
                    Component::ParentDir,
                    Component::Normal(_),
                    Component::Normal(_),
                ) => {
                    let mut s = PathBuf::from(c[2].as_os_str());
                    s.push(c[3].as_os_str());
                    Ok(PkgPath { short: s, full: p })
                }
                _ => Err(PkgPathError::InvalidPath),
            },
            //
            // All other forms of input are invalid.
            //
            _ => Err(PkgPathError::InvalidPath),
        }
    }

    /**
     * Return a [`Path`] reference containing the short version of a PkgPath,
     * for example `pkgtools/pkg_install`.
     */
    pub fn as_path(&self) -> &Path {
        &self.short
    }

    /**
     * Return a [`Path`] reference containing the full version of a PkgPath,
     * for example `../../pkgtools/pkg_install`.
     */
    pub fn as_full_path(&self) -> &Path {
        &self.full
    }
}

impl std::fmt::Display for PkgPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.short.display())
    }
}

impl FromStr for PkgPath {
    type Err = PkgPathError;

    fn from_str(s: &str) -> Result<Self, PkgPathError> {
        PkgPath::new(s)
    }
}

impl AsRef<Path> for PkgPath {
    fn as_ref(&self) -> &Path {
        &self.short
    }
}

impl crate::kv::FromKv for PkgPath {
    fn from_kv(value: &str, span: crate::kv::Span) -> crate::kv::Result<Self> {
        Self::new(value).map_err(|e| crate::kv::Error::Parse {
            message: e.to_string(),
            span,
        })
    }
}

impl Borrow<Path> for PkgPath {
    fn borrow(&self) -> &Path {
        &self.short
    }
}

impl TryFrom<&str> for PkgPath {
    type Error = PkgPathError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn assert_valid_foobar(s: &str) {
        let p = PkgPath::new(s).unwrap();
        assert_eq!(p.as_path(), OsStr::new("foo/bar"));
        assert_eq!(p.as_full_path(), OsStr::new("../../foo/bar"));
    }

    #[test]
    fn pkgpath_test_good_input() {
        assert_valid_foobar("foo/bar");
        assert_valid_foobar("foo//bar");
        assert_valid_foobar("foo//bar//");
        assert_valid_foobar("../../foo/bar");
        assert_valid_foobar("../../foo/bar/");
        assert_valid_foobar("..//..//foo//bar//");
    }

    #[test]
    fn pkgpath_test_bad_input() {
        let err = Err(PkgPathError::InvalidPath);
        assert_eq!(PkgPath::new(""), err);
        assert_eq!(PkgPath::new("\0"), err);
        assert_eq!(PkgPath::new("foo"), err);
        assert_eq!(PkgPath::new("foo/"), err);
        assert_eq!(PkgPath::new("./foo"), err);
        assert_eq!(PkgPath::new("./foo/"), err);
        assert_eq!(PkgPath::new("../foo"), err);
        assert_eq!(PkgPath::new("../foo/"), err);
        assert_eq!(PkgPath::new("../foo/bar"), err);
        assert_eq!(PkgPath::new("../foo/bar/"), err);
        assert_eq!(PkgPath::new("../foo/bar/ojnk"), err);
        assert_eq!(PkgPath::new("../foo/bar/ojnk/"), err);
        assert_eq!(PkgPath::new("../.."), err);
        assert_eq!(PkgPath::new("../../"), err);
        assert_eq!(PkgPath::new("../../foo"), err);
        assert_eq!(PkgPath::new("../../foo/"), err);
        assert_eq!(PkgPath::new("../../foo/bar/ojnk"), err);
        assert_eq!(PkgPath::new("../../foo/bar/ojnk/"), err);
        // ".. /" gets parsed as a Normal file named ".. ".
        assert_eq!(PkgPath::new(".. /../foo/bar"), err);
    }

    #[test]
    fn pkgpath_as_ref() {
        let p = PkgPath::new("pkgtools/pkg_install").unwrap();

        // AsRef<Path> returns the short path
        let path: &Path = p.as_ref();
        assert_eq!(path, Path::new("pkgtools/pkg_install"));

        // Test that it works with generic functions expecting AsRef<Path>
        fn takes_asref(p: impl AsRef<Path>) -> bool {
            p.as_ref().starts_with("pkgtools")
        }
        assert!(takes_asref(&p));
    }
}
