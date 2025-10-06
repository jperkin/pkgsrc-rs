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
use serde_with::{DeserializeFromStr, SerializeDisplay};

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
#[cfg_attr(feature = "serde", derive(SerializeDisplay, DeserializeFromStr))]
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
    #[must_use]
    pub fn new(pkgname: &str) -> Self {
        let (pkgbase, pkgversion) = match pkgname.rsplit_once('-') {
            Some((b, v)) => (String::from(b), String::from(v)),
            None => (String::from(pkgname), String::new()),
        };
        let pkgrevision = match pkgversion.rsplit_once("nb") {
            Some((_, v)) => v.parse::<i64>().ok().or(Some(0)),
            None => None,
        };
        Self {
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
    #[must_use]
    pub fn pkgname(&self) -> &str {
        &self.pkgname
    }

    /**
     * Return a [`str`] reference containing the `PKGBASE` portion of the
     * package name, i.e.  everything up to the final `-` and the version
     * number.
     */
    #[must_use]
    pub fn pkgbase(&self) -> &str {
        &self.pkgbase
    }

    /**
     * Return a [`str`] reference containing the full `PKGVERSION` of the
     * package name, i.e. everything after the final `-`.  If no `-` was found
     * in the [`str`] used to create this [`PkgName`] then this will be an
     * empty string.
     */
    #[must_use]
    pub fn pkgversion(&self) -> &str {
        &self.pkgversion
    }

    /**
     * Return an optional `PKGREVISION`, i.e. the `nb<x>` suffix that denotes
     * a pkgsrc revision.  If any characters after the `nb` cannot be parsed
     * as an [`i64`] then [`None`] is returned.  If there are no characters at
     * all after the `nb` then `Some(0)` is returned.
     */
    #[must_use]
    pub const fn pkgrevision(&self) -> Option<i64> {
        self.pkgrevision
    }
}

impl From<&str> for PkgName {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for PkgName {
    fn from(s: String) -> Self {
        Self::new(&s)
    }
}

impl From<&String> for PkgName {
    fn from(s: &String) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for PkgName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.pkgname)
    }
}

impl PartialEq<str> for PkgName {
    fn eq(&self, other: &str) -> bool {
        self.pkgname == other
    }
}

impl PartialEq<&str> for PkgName {
    fn eq(&self, other: &&str) -> bool {
        &self.pkgname == other
    }
}

impl PartialEq<String> for PkgName {
    fn eq(&self, other: &String) -> bool {
        &self.pkgname == other
    }
}

impl std::str::FromStr for PkgName {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkgname_full() {
        let pkg = PkgName::new("mktool-1.3.2nb2");
        assert_eq!(format!("{pkg}"), "mktool-1.3.2nb2");
        assert_eq!(pkg.pkgname(), "mktool-1.3.2nb2");
        assert_eq!(pkg.pkgbase(), "mktool");
        assert_eq!(pkg.pkgversion(), "1.3.2nb2");
        assert_eq!(pkg.pkgrevision(), Some(2));
    }

    #[test]
    fn pkgname_broken_pkgrevision() {
        let pkg = PkgName::new("mktool-1nb3alpha2nb");
        assert_eq!(pkg.pkgbase(), "mktool");
        assert_eq!(pkg.pkgversion(), "1nb3alpha2nb");
        assert_eq!(pkg.pkgrevision(), Some(0));
    }

    #[test]
    fn pkgname_no_version() {
        let pkg = PkgName::new("mktool");
        assert_eq!(pkg.pkgbase(), "mktool");
        assert_eq!(pkg.pkgversion(), "");
        assert_eq!(pkg.pkgrevision(), None);
    }

    #[test]
    fn pkgname_from() {
        let pkg = PkgName::from("mktool-1.3.2nb2");
        assert_eq!(pkg.pkgname(), "mktool-1.3.2nb2");
        let pkg = PkgName::from(String::from("mktool-1.3.2nb2"));
        assert_eq!(pkg.pkgname(), "mktool-1.3.2nb2");
        let s = String::from("mktool-1.3.2nb2");
        let pkg = PkgName::from(&s);
        assert_eq!(pkg.pkgname(), "mktool-1.3.2nb2");
    }

    #[test]
    fn pkgname_partial_eq() {
        let pkg = PkgName::new("mktool-1.3.2nb2");
        assert_eq!(pkg, *"mktool-1.3.2nb2");
        assert_eq!(pkg, "mktool-1.3.2nb2");
        assert_eq!(pkg, "mktool-1.3.2nb2".to_string());
        assert_ne!(pkg, "notmktool-1.0");
    }

    #[test]
    #[cfg(feature = "serde")]
    fn pkgname_serde() {
        let pkg = PkgName::new("mktool-1.3.2nb2");
        let se = serde_json::to_string(&pkg).unwrap();
        let de: PkgName = serde_json::from_str(&se).unwrap();
        assert_eq!(se, "\"mktool-1.3.2nb2\"");
        assert_eq!(pkg, de);
        assert_eq!(de.pkgname(), "mktool-1.3.2nb2");
        assert_eq!(de.pkgbase(), "mktool");
        assert_eq!(de.pkgversion(), "1.3.2nb2");
        assert_eq!(de.pkgrevision(), Some(2));
    }

}
