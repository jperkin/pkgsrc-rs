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
 * Package name parsing into base, version, and revision components.
 *
 * In pkgsrc, every package has a `PKGNAME` that uniquely identifies a specific
 * version of a package.
 *
 * ```text
 * PKGNAME = PKGBASE-PKGVERSION
 * PKGVERSION = VERSION[nbPKGREVISION]
 * ```
 *
 * For example, `mktool-1.4.2nb3` breaks down as:
 *
 * - **PKGBASE**: `mktool` - the package name
 * - **PKGVERSION**: `1.4.2nb3` - the full version string
 * - **VERSION**: `1.4.2` - the upstream version
 * - **PKGREVISION**: `3` - the pkgsrc-specific revision
 *
 * The `PKGBASE` and `PKGVERSION` are separated by the last hyphen (`-`) in the
 * string. The `PKGREVISION` suffix (`nb` followed by a number) indicates
 * pkgsrc-specific changes that do not correspond to an upstream release.
 *
 * # Examples
 *
 * ```
 * use pkgsrc::PkgName;
 *
 * let pkg = PkgName::new("nginx-1.25.3nb2");
 * assert_eq!(pkg.pkgbase(), "nginx");
 * assert_eq!(pkg.pkgversion(), "1.25.3nb2");
 * assert_eq!(pkg.pkgrevision(), Some(2));
 *
 * // Package with hyphenated name
 * let pkg = PkgName::new("p5-libwww-6.77");
 * assert_eq!(pkg.pkgbase(), "p5-libwww");
 * assert_eq!(pkg.pkgversion(), "6.77");
 * assert_eq!(pkg.pkgrevision(), None);
 *
 * // Package without revision
 * let pkg = PkgName::new("curl-8.5.0");
 * assert_eq!(pkg.pkgbase(), "curl");
 * assert_eq!(pkg.pkgversion(), "8.5.0");
 * assert_eq!(pkg.pkgrevision(), None);
 * ```
 *
 * # PKGREVISION
 *
 * The `PKGREVISION` is incremented by pkgsrc maintainers when:
 *
 * - A dependency is updated and the package needs rebuilding
 * - pkgsrc-specific patches are modified
 * - Build or packaging changes are made
 *
 * For version comparison, `1.0nb1` > `1.0` > `1.0rc1`. See the [`dewey`] module
 * for details on version comparison rules.
 *
 * [`dewey`]: crate::dewey
 */

use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

#[cfg(feature = "serde")]
use serde_with::{DeserializeFromStr, SerializeDisplay};

/**
 * Parse a `PKGNAME` into its constituent parts.
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
#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(SerializeDisplay, DeserializeFromStr))]
pub struct PkgName {
    pkgname: String,
    split: usize,
}

/**
 * Return the `PKGBASE` portion of a package name, i.e. everything before
 * the final `-`, or the full input if no `-` is present.
 */
#[must_use]
pub fn pkgbase(pkgname: &str) -> &str {
    pkgname.rsplit_once('-').map_or(pkgname, |(b, _)| b)
}

/**
 * Return the `PKGVERSION` portion of a package name, i.e. everything after
 * the final `-`, or the empty string if no `-` is present.
 */
#[must_use]
pub fn pkgversion(pkgname: &str) -> &str {
    pkgname.rsplit_once('-').map_or("", |(_, v)| v)
}

/**
 * Return the `PKGVERSION_NOREV` portion of a package version, i.e. the
 * version with any trailing `nb<n>` revision marker stripped.
 *
 * Splits at the final `nb` substring, matching the behaviour of
 * [`pkgrevision`].  Returns the input unchanged when no `nb` marker is
 * present.
 */
#[must_use]
pub fn pkgversion_norev(pkgversion: &str) -> &str {
    pkgversion
        .rsplit_once("nb")
        .map_or(pkgversion, |(before, _)| before)
}

/**
 * Return the `PKGREVISION` parsed from a package version, i.e. the
 * integer following the final `nb`.
 *
 * Returns [`None`] when no `nb` marker is present, [`Some(0)`] when the
 * marker is present but the digits cannot be parsed as an [`i64`] (or
 * are absent entirely).
 */
#[must_use]
pub fn pkgrevision(pkgversion: &str) -> Option<i64> {
    pkgversion
        .rsplit_once("nb")
        .map(|(_, v)| v.parse::<i64>().unwrap_or(0))
}

impl PkgName {
    /**
     * Create a new [`PkgName`] from a [`str`] reference.
     */
    #[must_use]
    pub fn new(pkgname: &str) -> Self {
        let split = pkgname.rfind('-').unwrap_or(pkgname.len());
        Self {
            pkgname: pkgname.to_string(),
            split,
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
        &self.pkgname[..self.split]
    }

    /**
     * Return a [`str`] reference containing the full `PKGVERSION` of the
     * package name, i.e. everything after the final `-`.  If no `-` was found
     * in the [`str`] used to create this [`PkgName`] then this will be an
     * empty string.
     */
    #[must_use]
    pub fn pkgversion(&self) -> &str {
        if self.split < self.pkgname.len() {
            &self.pkgname[self.split + 1..]
        } else {
            ""
        }
    }

    /**
     * Return a [`str`] reference containing the `PKGVERSION_NOREV` of the
     * package name, i.e. the version with any `nb<x>` revision marker
     * stripped.
     */
    #[must_use]
    pub fn pkgversion_norev(&self) -> &str {
        pkgversion_norev(self.pkgversion())
    }

    /**
     * Return the `PKGREVISION` of the package name.  See [`pkgrevision`]
     * for the parsing rules.
     */
    #[must_use]
    pub fn pkgrevision(&self) -> Option<i64> {
        pkgrevision(self.pkgversion())
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

impl FromStr for PkgName {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

impl AsRef<str> for PkgName {
    fn as_ref(&self) -> &str {
        &self.pkgname
    }
}

impl Borrow<str> for PkgName {
    fn borrow(&self) -> &str {
        &self.pkgname
    }
}

// Hash must be consistent with Borrow<str> - only hash the pkgname field
// so that HashMap::get("foo-1.0") works when the key is PkgName::new("foo-1.0")
impl Hash for PkgName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pkgname.hash(state);
    }
}

impl crate::kv::FromKv for PkgName {
    fn from_kv(value: &str, _span: crate::kv::Span) -> crate::kv::Result<Self> {
        Ok(Self::new(value))
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
    fn pkgname_from_str() -> Result<(), std::convert::Infallible> {
        use std::str::FromStr;

        let pkg = PkgName::from_str("mktool-1.3.2nb2")?;
        assert_eq!(pkg.pkgname(), "mktool-1.3.2nb2");

        let pkg: PkgName = "foo-2.0".parse()?;
        assert_eq!(pkg.pkgbase(), "foo");
        Ok(())
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
    fn pkgname_as_ref() {
        let pkg = PkgName::new("mktool-1.3.2nb2");
        let s: &str = pkg.as_ref();
        assert_eq!(s, "mktool-1.3.2nb2");

        // Test that it works with generic functions expecting AsRef<str>
        fn takes_asref(s: impl AsRef<str>) -> usize {
            s.as_ref().len()
        }
        assert_eq!(takes_asref(&pkg), 15);
    }

    #[test]
    fn pkgname_borrow() {
        use std::collections::HashMap;

        // Test that PkgName can be used as HashMap key with &str lookup
        let mut map: HashMap<PkgName, i32> = HashMap::new();
        map.insert(PkgName::new("foo-1.0"), 42);

        // Can look up by &str due to Borrow<str>
        assert_eq!(map.get("foo-1.0"), Some(&42));
        assert_eq!(map.get("bar-2.0"), None);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn pkgname_serde() -> Result<(), serde_json::Error> {
        let pkg = PkgName::new("mktool-1.3.2nb2");
        let se = serde_json::to_string(&pkg)?;
        let de: PkgName = serde_json::from_str(&se)?;
        assert_eq!(se, "\"mktool-1.3.2nb2\"");
        assert_eq!(pkg, de);
        assert_eq!(de.pkgname(), "mktool-1.3.2nb2");
        assert_eq!(de.pkgbase(), "mktool");
        assert_eq!(de.pkgversion(), "1.3.2nb2");
        assert_eq!(de.pkgrevision(), Some(2));
        Ok(())
    }
}
