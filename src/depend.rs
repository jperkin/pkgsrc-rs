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

use crate::{Pattern, PatternError, PkgPath, PkgPathError};
use std::str::FromStr;
use thiserror::Error;

/**
 * Parse `DEPENDS` and other package dependency types.
 *
 * pkgsrc uses a few different ways to express package dependencies.  The most
 * common looks something like this, where a dependency on any version of mutt
 * is expressed, with mutt most likely to be found at `mail/mutt` (though not
 * always).
 *
 * ```text
 * DEPENDS+=    mutt-[0-9]*:../../mail/mutt
 * ```
 *
 * There are a few different types, expressed in [`DependType`].
 *
 * A `DEPENDS` match is essentially of the form "[`Pattern`]:[`PkgPath`]"
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Depend {
    /**
     * A [`Pattern`] containing the package match.
     */
    pattern: Pattern,
    /**
     * A [`PkgPath`] containing the most likely location for this dependency.
     * Note that when multiple packages that match the pattern are available
     * then this may not be the [`PkgPath`] that is ultimately chosen, if a
     * package at a different location ends up being a better match.
     */
    pkgpath: PkgPath,
}

impl Depend {
    /**
     * Create a new [`Depend`] from a [`str`] slice.  Return a [`DependError`]
     * if it cannot be created successfully.
     *
     * # Example
     *
     * ```
     * use pkgsrc::{Depend, Pattern, PkgPath};
     *
     * let dep = Depend::new("mktool-[0-9]*:../../pkgtools/mktool").unwrap();
     * assert_eq!(dep.pattern(), &Pattern::new("mktool-[0-9]*").unwrap());
     * assert_eq!(dep.pkgpath(), &PkgPath::new("pkgtools/mktool").unwrap());
     *
     * // Invalid, too many ":".
     * assert!(Depend::new("pkg>0::../../cat/pkg").is_err());
     *
     * // Invalid, incorrect Dewey specification.
     * assert!(Depend::new("pkg>0>2:../../cat/pkg").is_err());
     * ```
     */
    pub fn new(s: &str) -> Result<Self, DependError> {
        let v: Vec<_> = s.split(":").collect();
        if v.len() != 2 {
            return Err(DependError::Invalid);
        }
        let pattern = Pattern::new(v[0])?;
        let pkgpath = PkgPath::from_str(v[1])?;
        Ok(Depend { pattern, pkgpath })
    }

    /**
     * Return the [`Pattern`] portion of this [`Depend`].
     */
    pub fn pattern(&self) -> &Pattern {
        &self.pattern
    }

    /**
     * Return the [`PkgPath`] portion of this [`Depend`].
     */
    pub fn pkgpath(&self) -> &PkgPath {
        &self.pkgpath
    }
}

/**
 * Type of dependency (full, build, bootstrap, test, etc.)
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
 * A `DEPENDS` parsing error.
 */
#[derive(Debug, Error)]
pub enum DependError {
    /**
     * An invalid string that doesn't match `<pattern>:<pkgpath>`.
     */
    #[error("Invalid DEPENDS string")]
    Invalid,
    /**
     * A transparent [`PatternError`] error.
     *
     * [`PatternError`]: crate::pattern::PatternError
     */
    #[error(transparent)]
    Pattern(#[from] PatternError),
    /**
     * A transparent [`PkgPathError`] error.
     *
     * [`PkgPathError`]: crate::pkgpath::PkgPathError
     */
    #[error(transparent)]
    PkgPath(#[from] PkgPathError),
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
        let pkgmatch = Pattern::new("mktools-[0-9]").unwrap();
        let pkgpath = PkgPath::new("../../pkgtools/mktools").unwrap();
        let dep = Depend::new("mktools-[0-9]:../../pkgtools/mktools")?;
        assert_eq!(dep.pattern(), &pkgmatch);
        assert_eq!(dep.pkgpath(), &pkgpath);
        let dep = Depend::new("mktools-[0-9]:pkgtools/mktools")?;
        assert_eq!(dep.pattern(), &pkgmatch);
        assert_eq!(dep.pkgpath(), &pkgpath);
        Ok(())
    }

    #[test]
    fn test_bad() {
        // Missing ":" separator.
        let dep = Depend::new("pkg");
        assert!(matches!(dep, Err(DependError::Invalid)));

        // Too many ":" separators.
        let dep = Depend::new("pkg-[0-9]*::../../pkgtools/pkg");
        assert!(matches!(dep, Err(DependError::Invalid)));

        // Invalid Pattern
        let dep = Depend::new("pkg>2>3:../../pkgtools/pkg");
        assert!(matches!(dep, Err(DependError::Pattern(_))));

        // Invalid PkgPath
        let dep = Depend::new("ojnk:foo");
        assert!(matches!(dep, Err(DependError::PkgPath(_))));
    }
}
