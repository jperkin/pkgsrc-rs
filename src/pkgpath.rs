/*
 * Copyright (c) 2024 Jonathan Perkin <jonathan@perkin.org.uk>
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
 * use pkgsrc::pkgpath::*;
 * use std::ffi::OsStr;
 *
 * let p = PkgPath::new("pkgtools/pkg_install").expect("ruh roh");
 * assert_eq!(p.as_path(), OsStr::new("pkgtools/pkg_install"));
 * assert_eq!(p.as_full_path(), OsStr::new("../../pkgtools/pkg_install"));
 *
 * let p = PkgPath::new("../../pkgtools/pkg_install").expect("ruh roh");
 * assert_eq!(p.as_path(), OsStr::new("pkgtools/pkg_install"));
 * assert_eq!(p.as_full_path(), OsStr::new("../../pkgtools/pkg_install"));
 *
 * assert_eq!(PkgPath::new("../../pkg_install"), Err(PkgPathError::InvalidPath));
 * assert_eq!(PkgPath::new("../pkg_install"), Err(PkgPathError::InvalidPath));
 * assert_eq!(PkgPath::new("/pkgtools/pkg_install"), Err(PkgPathError::InvalidPath));
 * ```
 *
 * [`as_full_path`]: PkgPath::as_full_path
 * [`as_path`]: PkgPath::as_path
 */

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

/**
 * A type alias for the result from the creation of a [`PkgPath`], with
 * [`PkgPathError`] returned in [`Err`] variants.
 */
pub type Result<T> = std::result::Result<T, PkgPathError>;

/**
 * PkgPathError
 */
#[derive(Debug, PartialEq)]
pub enum PkgPathError {
    /**
     * Contains an invalid path.
     */
    InvalidPath,
}

/**
 * PkgPath
 */
#[derive(Debug, PartialEq)]
pub struct PkgPath {
    short: PathBuf,
    full: PathBuf,
}

impl PkgPath {
    /**
     * Create a new PkgPath
     */
    pub fn new(path: &str) -> Result<Self> {
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

impl FromStr for PkgPath {
    type Err = PkgPathError;

    fn from_str(s: &str) -> Result<Self> {
        PkgPath::new(s)
    }
}

impl fmt::Display for PkgPathError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            PkgPathError::InvalidPath => {
                write!(f, "String contains an invalid path")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn assert_valid_foobar(s: &str) -> Result<()> {
        let p = PkgPath::new(s)?;
        assert_eq!(p.as_path(), OsStr::new("foo/bar"));
        assert_eq!(p.as_full_path(), OsStr::new("../../foo/bar"));
        Ok(())
    }

    #[test]
    fn pkgpath_test_good_input() -> Result<()> {
        assert_valid_foobar("foo/bar")?;
        assert_valid_foobar("foo//bar")?;
        assert_valid_foobar("foo//bar//")?;
        assert_valid_foobar("../../foo/bar")?;
        assert_valid_foobar("../../foo/bar/")?;
        assert_valid_foobar("..//..//foo//bar//")?;
        Ok(())
    }

    #[test]
    fn pkgpath_test_bad_input() -> Result<()> {
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
        Ok(())
    }
}
