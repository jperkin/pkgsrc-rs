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

#[cfg(feature = "serde")]
use serde::Deserialize;

/**
 * Parse a `PKGNAME` into its consituent parts.
 *
 * In pkgsrc terminology a `PKGNAME` is made up of three parts:
 *
 * * `PKGBASE` contains the name of the package
 * * `PKGVERSION` contains the full version string
 * * `PKGREVISION` is an optional package revision denoted by `nb` followed by
 *   a number.
 *
 * The name and version are split at the last `-`, and the revision, if
 * specified, should be located at the end of the version.
 *
 * This module does not enforce strict formatting.  If a `PKGNAME` is not well
 * formed then values may be empty or [`None`].
 *
 * # Examples
 *
 * ```
 * use pkgsrc::PkgName;
 *
 * // A well formed package name.
 * let pkg = PkgName::new("mktool-1.3.2nb2");
 * assert_eq!(pkg.pkgname(), "mktool-1.3.2nb2");
 * assert_eq!(pkg.pkgbase(), "mktool");
 * assert_eq!(pkg.pkgversion(), "1.3.2nb2");
 * assert_eq!(pkg.pkgrevision(), Some(2));
 *
 * // An invalid PKGREVISION that can likely only be created by accident.
 * let pkg = PkgName::new("mktool-1.3.2nb");
 * assert_eq!(pkg.pkgbase(), "mktool");
 * assert_eq!(pkg.pkgversion(), "1.3.2nb");
 * assert_eq!(pkg.pkgrevision(), Some(0));
 *
 * // A "-" in the version causes an incorrect split.
 * let pkg = PkgName::new("mktool-1.3-2");
 * assert_eq!(pkg.pkgbase(), "mktool-1.3");
 * assert_eq!(pkg.pkgversion(), "2");
 * assert_eq!(pkg.pkgrevision(), None);
 *
 * // Not well formed, but still accepted.
 * let pkg = PkgName::new("mktool");
 * assert_eq!(pkg.pkgbase(), "mktool");
 * assert_eq!(pkg.pkgversion(), "");
 * assert_eq!(pkg.pkgrevision(), None);
 *
 * // Doesn't make any sense, but whatever!
 * let pkg = PkgName::new("1.0nb2");
 * assert_eq!(pkg.pkgbase(), "1.0nb2");
 * assert_eq!(pkg.pkgversion(), "");
 * assert_eq!(pkg.pkgrevision(), None);
 * ```
 */
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Deserialize))]
pub struct PkgName {
    pkgname: String,
    pkgbase: String,
    pkgversion: String,
    pkgrevision: Option<i64>,
}

impl PkgName {
    /**
     * Create a new [`PkgName`] from a [`str`] reference.
     */
    pub fn new(pkgname: &str) -> Self {
        let (pkgbase, pkgversion) = match pkgname.rsplit_once('-') {
            Some((b, v)) => (String::from(b), String::from(v)),
            None => (String::from(pkgname), String::from("")),
        };
        let pkgrevision = match pkgversion.rsplit_once("nb") {
            Some((_, v)) => v.parse::<i64>().ok().or(Some(0)),
            None => None,
        };
        PkgName {
            pkgname: pkgname.to_string(),
            pkgbase,
            pkgversion,
            pkgrevision,
        }
    }

    /**
     * Return a [`str`] reference containing the original `PKGNAME` used to
     * create this instance.
     */
    pub fn pkgname(&self) -> &str {
        &self.pkgname
    }

    /**
     * Return a [`str`] reference containing the `PKGBASE` portion of the
     * package name, i.e.  everything up to the final `-` and the version
     * number.
     */
    pub fn pkgbase(&self) -> &str {
        &self.pkgbase
    }

    /**
     * Return a [`str`] reference containing the full `PKGVERSION` of the
     * package name, i.e. everything after the final `-`.  If no `-` was found
     * in the [`str`] used to create this [`PkgName`] then this will be an
     * empty string.
     */
    pub fn pkgversion(&self) -> &str {
        &self.pkgversion
    }

    /**
     * Return an optional `PKGREVISION`, i.e. the `nb<x>` suffix that denotes
     * a pkgsrc revision.  If any characters after the `nb` cannot be parsed
     * as an [`i64`] then [`None`] is returned.  If there are no characters at
     * all after the `nb` then `Some(0)` is returned.
     */
    pub const fn pkgrevision(&self) -> Option<i64> {
        self.pkgrevision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkgname() {
        let pkg = PkgName::new("mktool-1.3.2nb2");
        assert_eq!(pkg.pkgname(), "mktool-1.3.2nb2");
        assert_eq!(pkg.pkgbase(), "mktool");
        assert_eq!(pkg.pkgversion(), "1.3.2nb2");
        assert_eq!(pkg.pkgrevision(), Some(2));

        let pkg = PkgName::new("mktool-1nb3alpha2nb");
        assert_eq!(pkg.pkgbase(), "mktool");
        assert_eq!(pkg.pkgversion(), "1nb3alpha2nb");
        assert_eq!(pkg.pkgrevision(), Some(0));

        let pkg = PkgName::new("mktool");
        assert_eq!(pkg.pkgbase(), "mktool");
        assert_eq!(pkg.pkgversion(), "");
        assert_eq!(pkg.pkgrevision(), None);
    }
}
