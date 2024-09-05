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
 * Support for `DEPENDS`.
 */
use crate::pkgpath::PkgPath;
use std::str::FromStr;

/**
 * [`Depend`]
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Depend {
    /**
     * A [`String`] containing the package match.
     */
    pkgmatch: String,
    /**
     * Path portion of this `DEPENDS` entry.  Note that when multiple packages
     * that match are available then this may not be the [`PkgPath`] that is
     * ultimately chosen.
     */
    pkgpath: PkgPath,
}

impl Depend {
    /**
     * Create a new [`Depend`].
     */
    pub fn new(s: &str) -> Result<Self, DependError> {
        let v: Vec<_> = s.split(":").collect();
        if v.len() != 2 {
            return Err(DependError::Invalid);
        }
        let pkgmatch = String::from(v[0]);
        let pkgpath = match PkgPath::from_str(v[1]) {
            Ok(p) => p,
            Err(_) => {
                return Err(DependError::Invalid);
            }
        };
        Ok(Depend { pkgmatch, pkgpath })
    }

    /**
     * Return the string match portion of this [`Depend`].
     */
    pub fn pkgmatch(&self) -> &String {
        &self.pkgmatch
    }

    /**
     * Return the [`PkgPath`] portion of this [`Depend`].
     */
    pub fn pkgpath(&self) -> &PkgPath {
        &self.pkgpath
    }
}

/**
 * DependType
 */
#[derive(Debug, Default)]
pub enum DependType {
    /**
     * A regular full pkgsrc dependency for this package, usually specified
     * using `DEPENDS`.  The default value.
     */
    #[default]
    Full,
    /**
     * A pkgsrc dependency that is only required to build the package, usually
     * specified using `BUILD_DEPENDS`.
     */
    Build,
    /**
     * Dependency required to make the pkgsrc infrastructure work for this
     * package (for example a checksum tool to verify distfiles).
     */
    Bootstrap,
    /**
     * A host tool required to build this package.  May turn into a pkgsrc
     * dependency if the host does not provide a compatible tool.  May be
     * defined using `USE_TOOLS` or `TOOL_DEPENDS`.
     */
    Tool,
    /**
     * A pkgsrc dependency that is only required to run the test suite for a
     * package.
     */
    Test,
}

/**
 * DependError
 */
#[derive(Debug, PartialEq)]
pub enum DependError {
    /// An invalid string
    Invalid,
}

impl FromStr for Depend {
    type Err = DependError;

    fn from_str(s: &str) -> Result<Self, DependError> {
        Depend::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_good() -> Result<(), DependError> {
        let pkgmatch = "mktools-[0-9]";
        let pkgpath = PkgPath::new("../../pkgtools/mktools").unwrap();
        let dep = Depend::new("mktools-[0-9]:../../pkgtools/mktools")?;
        assert_eq!(dep.pkgmatch(), pkgmatch);
        assert_eq!(dep.pkgpath(), &pkgpath);
        let dep = Depend::new("mktools-[0-9]:pkgtools/mktools")?;
        assert_eq!(dep.pkgmatch(), pkgmatch);
        assert_eq!(dep.pkgpath(), &pkgpath);
        Ok(())
    }

    #[test]
    fn test_bad() -> Result<(), DependError> {
        let dep = Depend::new("ojnk");
        assert_eq!(dep, Err(DependError::Invalid));
        let dep = Depend::new("ojnk:foo");
        assert_eq!(dep, Err(DependError::Invalid));
        let dep = Depend::new("ojnk:foo:../../pkgtools/foo");
        assert_eq!(dep, Err(DependError::Invalid));
        Ok(())
    }
}
