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

/*! Package pattern matching with globs and version constraints. */

use crate::PkgName;
use crate::dewey::{Dewey, DeweyError, DeweyOp, DeweyVersion, dewey_cmp};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

#[cfg(feature = "serde")]
use serde_with::{DeserializeFromStr, SerializeDisplay};

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
enum PatternType {
    Alternate,
    Dewey,
    Glob,
    #[default]
    Simple,
}

/**
 * A pattern error.
 *
 * Returned by [`Pattern::new`] when a pattern cannot be parsed, or by
 * [`Pattern::best_match`] when a package version cannot be compared.
 */
#[derive(Debug, Error)]
pub enum PatternError {
    /// An alternate pattern was supplied with unbalanced braces.
    #[error("Unbalanced braces in pattern")]
    Alternate,
    /// Transparent [`DeweyError`]
    #[error(transparent)]
    Dewey(#[from] DeweyError),
    /// Transparent [`glob::PatternError`]
    #[error(transparent)]
    Glob(#[from] glob::PatternError),
}

/**
 * Package pattern matching.
 *
 * Pattern matching is used to specify package requirements for various
 * dependency types.  This module supports all of the pattern match types that
 * are used across pkgsrc.
 *
 * ## Examples
 *
 * Standard UNIX glob matches are probably the most common style of dependency
 * pattern, matching any version of a specific package.  This module uses the
 * [`glob`] crate to perform the match.
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * let m = Pattern::new("mutt-[0-9]*").unwrap();
 * assert_eq!(m.matches("mutt-2.2.13"), true);
 * assert_eq!(m.matches("mutt-vid-1.1"), false);
 * assert_eq!(m.matches("pine-1.0"), false);
 * ```
 *
 * Next most popular are so-called "dewey" matches.  These are used to test
 * for a specific range of versions.
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * let m = Pattern::new("librsvg>=2.12<2.41").unwrap();
 * assert_eq!(m.matches("librsvg-2.11"), false);
 * assert_eq!(m.matches("librsvg-2.12alpha"), false);
 * assert_eq!(m.matches("librsvg-2.13"), true);
 * assert_eq!(m.matches("librsvg-2.41"), false);
 * ```
 *
 * Alternate matches are csh-style `{foo,bar}` either/or matches, matching any
 * of the expanded strings.
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * let m = Pattern::new("{mysql,mariadb,percona}-[0-9]*").unwrap();
 * assert_eq!(m.matches("mysql-8.0.36"), true);
 * assert_eq!(m.matches("mariadb-11.4.3"), true);
 * assert_eq!(m.matches("postgresql-16.4"), false);
 * ```
 *
 * Finally plain, exact string matches can be used, though these are very
 * rare and never recommended.
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * let m = Pattern::new("foobar-1.0").unwrap();
 * assert_eq!(m.matches("foobar-1.0"), true);
 * assert_eq!(m.matches("foobar-1.1"), false);
 * ```
 *
 * If the pattern is invalid, [`Pattern::new`] will return a [`PatternError`].
 *
 * ```
 * use pkgsrc::{PatternError::*, Pattern};
 *
 * // Missing closing bracket or too many *'s.
 * assert!(matches!(Pattern::new("foo-[0-9"), Err(Glob(_))));
 * assert!(matches!(Pattern::new("foo-[0-9]***"), Err(Glob(_))));
 *
 * // Too many or incorrectly-ordered comparisons.
 * assert!(matches!(Pattern::new("foo>1.0<2<3"), Err(Dewey(_))));
 * assert!(matches!(Pattern::new("foo<1>0"), Err(Dewey(_))));
 *
 * // Version component overflow (exceeds i64::MAX).
 * assert!(matches!(Pattern::new("foo>=20251208143052123456"), Err(Dewey(_))));
 *
 * // Unbalanced or incorrectly-ordered braces.
 * assert!(matches!(Pattern::new("{foo,bar}}>1.0"), Err(Alternate)));
 * assert!(matches!(Pattern::new("foo}b{ar>1.0"), Err(Alternate)));
 * ```
 *
 * [`glob`]: https://docs.rs/glob/latest/glob/
 */
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[allow(clippy::struct_field_names)]
#[cfg_attr(feature = "serde", derive(SerializeDisplay, DeserializeFromStr))]
pub struct Pattern {
    matchtype: PatternType,
    pattern: String,
    likely: bool,
    dewey: Option<Dewey>,
    glob: Option<glob::Pattern>,
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.pattern)
    }
}

impl FromStr for Pattern {
    type Err = PatternError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<&str> for Pattern {
    type Error = PatternError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl Pattern {
    /**
     * Compile a pattern.  If the pattern is invalid in any way a
     * [`PatternError`] is returned.
     *
     * # Errors
     *
     * Returns [`PatternError::Alternate`] if braces are unbalanced.
     *
     * Returns [`PatternError::Dewey`] if a dewey pattern is malformed.
     *
     * Returns [`PatternError::Glob`] if a glob pattern is invalid.
     *
     * # Examples
     *
     * ```
     * use pkgsrc::Pattern;
     *
     * let pkgmatch = Pattern::new("librsvg>=2.12<2.41");
     * assert!(pkgmatch.is_ok());
     *
     * // Missing closing brace
     * let pkgmatch = Pattern::new("{mariadb,mysql*-[0-9]");
     * assert!(pkgmatch.is_err());
     * ```
     */
    pub fn new(pattern: &str) -> Result<Self, PatternError> {
        if pattern.contains('{') || pattern.contains('}') {
            let matchtype = PatternType::Alternate;
            /*
             * Verify that braces are correctly balanced.
             */
            let mut stack = vec![];
            for ch in pattern.chars() {
                if ch == '{' {
                    stack.push(ch);
                } else if ch == '}' && stack.pop().is_none() {
                    return Err(PatternError::Alternate);
                }
            }
            if !stack.is_empty() {
                return Err(PatternError::Alternate);
            }
            return Ok(Self {
                matchtype,
                pattern: pattern.to_string(),
                ..Default::default()
            });
        }
        if pattern.contains('>') || pattern.contains('<') {
            let matchtype = PatternType::Dewey;
            let dewey = Some(Dewey::new(pattern)?);
            return Ok(Self {
                matchtype,
                pattern: pattern.to_string(),
                dewey,
                ..Default::default()
            });
        }
        if pattern.contains('*')
            || pattern.contains('?')
            || pattern.contains('[')
            || pattern.contains(']')
        {
            let matchtype = PatternType::Glob;
            let glob = Some(glob::Pattern::new(pattern)?);
            return Ok(Self {
                matchtype,
                pattern: pattern.to_string(),
                glob,
                ..Default::default()
            });
        }
        Ok(Self {
            matchtype: PatternType::Simple,
            pattern: pattern.to_string(),
            ..Default::default()
        })
    }

    /**
     * Return whether a given [`str`] matches the compiled pattern.  `pkg`
     * must be a fully-specified `PKGNAME`.
     *
     * # Example
     *
     * ```
     * use pkgsrc::Pattern;
     *
     * let pkgmatch = Pattern::new("librsvg>=2.12<2.41").unwrap();
     * assert_eq!(pkgmatch.matches("librsvg"), false);
     * assert_eq!(pkgmatch.matches("librsvg-2.11"), false);
     * assert_eq!(pkgmatch.matches("librsvg-2.13"), true);
     * assert_eq!(pkgmatch.matches("librsvg-2.41"), false);
     * ```
     */
    #[must_use]
    pub fn matches(&self, pkg: &str) -> bool {
        /*
         * As a small optimisation, unless the "likely" flag has been set,
         * perform a quick test on the first few characters to see if this can
         * possibly be a match, and if not return early.  This can have quite
         * a decent performance benefit when matching across many thousands of
         * packages.
         */
        if !self.likely && !Self::quick_pkg_match(&self.pattern, pkg) {
            return false;
        }

        /*
         * Delegate match to each type.
         */
        match self.matchtype {
            PatternType::Alternate => Self::alternate_match(&self.pattern, pkg),
            PatternType::Dewey => {
                let Some(dewey) = &self.dewey else {
                    return false;
                };
                dewey.matches(pkg)
            }
            PatternType::Glob => {
                let Some(glob) = &self.glob else {
                    return false;
                };
                glob.matches(pkg)
            }
            PatternType::Simple => self.pattern == pkg,
        }
    }

    /**
     * Given two package names, return the "best" match - that is, the one that
     * is a match with the higher version.  If neither match return [`None`].
     *
     * When versions compare equal, the lexicographically smaller string is
     * returned, to match pkg_install's `pkg_order()`.
     *
     * # Errors
     *
     * Returns [`PatternError::Dewey`] if parsing a package version fails.
     */
    pub fn best_match<'a>(
        &self,
        pkg1: &'a str,
        pkg2: &'a str,
    ) -> Result<Option<&'a str>, PatternError> {
        self.best_match_cmp(pkg1, pkg2, std::cmp::Ordering::Less)
    }

    /**
     * Identical to [`Pattern::best_match`] except when versions compare equal,
     * the lexicographically greater string is returned to match pbulk's
     * `pkg_order()`.
     *
     * # Errors
     *
     * Returns [`PatternError::Dewey`] if parsing a package version fails.
     */
    pub fn best_match_pbulk<'a>(
        &self,
        pkg1: &'a str,
        pkg2: &'a str,
    ) -> Result<Option<&'a str>, PatternError> {
        self.best_match_cmp(pkg1, pkg2, std::cmp::Ordering::Greater)
    }

    fn best_match_cmp<'a>(
        &self,
        pkg1: &'a str,
        pkg2: &'a str,
        tiebreak: std::cmp::Ordering,
    ) -> Result<Option<&'a str>, PatternError> {
        match (self.matches(pkg1), self.matches(pkg2)) {
            (true, false) => Ok(Some(pkg1)),
            (false, true) => Ok(Some(pkg2)),
            (true, true) => {
                let d1 = DeweyVersion::new(PkgName::new(pkg1).pkgversion())?;
                let d2 = DeweyVersion::new(PkgName::new(pkg2).pkgversion())?;
                if dewey_cmp(&d1, &DeweyOp::GT, &d2) {
                    Ok(Some(pkg1))
                } else if dewey_cmp(&d1, &DeweyOp::LT, &d2) {
                    Ok(Some(pkg2))
                } else if pkg1.cmp(pkg2) == tiebreak {
                    Ok(Some(pkg1))
                } else {
                    Ok(Some(pkg2))
                }
            }
            (false, false) => Ok(None),
        }
    }

    /**
     * Return the original pattern string.
     */
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /**
     * Return the package base name this pattern matches, if known.
     *
     * Returns [`Some`] for Dewey, Simple, and Glob patterns where the base
     * name can be determined.  Returns [`None`] for Alternate patterns and
     * Glob patterns where the base name contains a glob.
     *
     * This is useful for building an index to speed up matching:
     *
     * ```
     * use pkgsrc::Pattern;
     *
     * let p = Pattern::new("foo>=1.0")?;
     * assert_eq!(p.pkgbase(), Some("foo"));
     *
     * let p = Pattern::new("foo-1.0")?;
     * assert_eq!(p.pkgbase(), Some("foo"));
     *
     * let p = Pattern::new("foo-[0-9]*")?;
     * assert_eq!(p.pkgbase(), Some("foo"));
     *
     * let p = Pattern::new("foo*-1.0")?;
     * assert_eq!(p.pkgbase(), None);
     * # Ok::<(), pkgsrc::PatternError>(())
     * ```
     */
    #[must_use]
    pub fn pkgbase(&self) -> Option<&str> {
        match self.matchtype {
            PatternType::Dewey => self.dewey.as_ref().map(|d| d.pkgbase()),
            PatternType::Simple => {
                self.pattern.rsplit_once('-').map(|(b, _)| b)
            }
            PatternType::Glob => {
                let end = self
                    .pattern
                    .find(['*', '?', '['])
                    .unwrap_or(self.pattern.len());
                self.pattern[..end].strip_suffix('-')
            }
            PatternType::Alternate => None,
        }
    }

    /**
     * Implement csh-style alternate matches.  [`Pattern::new`] has already
     * verified that the pattern is valid and braces are correctly balanced.
     *
     * The algorithm starts at the right-most opening brace and iteratively works
     * backwards, expanding each alternate match and recursively calling Pattern
     * to verify that there is a match.
     */
    fn alternate_match(pattern: &str, pkg: &str) -> bool {
        for (i, _) in
            pattern.match_indices('{').collect::<Vec<_>>().iter().rev()
        {
            let (first, rest) = pattern.split_at(*i);
            /* This shouldn't fail as new() already verified, but... */
            let Some(n) = rest.find('}') else {
                return false;
            };
            let (matches, last) = rest.split_at(n + 1);
            let matches = &matches[1..matches.len() - 1];

            for m in matches.split(',') {
                let fmt = format!("{first}{m}{last}");
                if let Ok(pat) = Self::new(&fmt) {
                    if pat.matches(pkg) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /**
     * `pkg_install` contains a `quick_pkg_match()` routine to quickly exit if
     * there is no possibility of a match. As it gives a decent speed bump
     * when matching across thousands of packages we include a similar routine.
     */
    fn quick_pkg_match(pattern: &str, pkg: &str) -> bool {
        let mut p1 = pattern.chars();
        let mut p2 = pkg.chars();
        let mut p;

        p = p1.next();
        if p.is_none() || !Self::is_simple_char(p.unwrap()) {
            return true;
        }
        if p != p2.next() {
            return false;
        }

        p = p1.next();
        if p.is_none() || !Self::is_simple_char(p.unwrap()) {
            return true;
        }
        if p != p2.next() {
            return false;
        }
        true
    }

    const fn is_simple_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '-'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_pattern {
        ($pattern:expr, $pkg:expr, $variant:pat, $result:expr) => {
            let p = Pattern::new($pattern).unwrap();
            assert!(matches!(&p.matchtype, $variant));
            assert_eq!(p.matches($pkg), $result);
        };
    }
    macro_rules! assert_pattern_eq {
        ($pattern:expr, $pkg:expr, $variant:pat) => {
            assert_pattern!($pattern, $pkg, $variant, true);
        };
    }
    macro_rules! assert_pattern_ne {
        ($pattern:expr, $pkg:expr, $variant:pat) => {
            assert_pattern!($pattern, $pkg, $variant, false);
        };
    }
    macro_rules! assert_pattern_err {
        ($pattern:expr, $variant:pat) => {
            let p = Pattern::new($pattern);
            assert!(matches!(p, Err($variant)));
        };
    }

    /*
     * csh-style alternate matches, i.e. "{this,that}".
     */
    #[test]
    fn alternate_match_ok() {
        use super::PatternType::Alternate;
        assert_pattern_eq!("a-{b,c}-{d{e,f},g}-h>=1", "a-b-de-h-2", Alternate);
        assert_pattern_eq!("a-{b,c}-{d{e,f},g}-h>=1", "a-b-de-h-2", Alternate);
        assert_pattern_eq!("a-{b,c}-{d{e,f},g}-h>=1", "a-b-df-h-2", Alternate);
        assert_pattern_eq!("a-{b,c}-{d{e,f},g}-h>=1", "a-b-g-h-2", Alternate);
        assert_pattern_eq!("a-{b,c}-{d{e,f},g}-h>=1", "a-c-de-h-2", Alternate);
        assert_pattern_eq!("a-{b,c}-{d{e,f},g}-h>=1", "a-c-df-h-2", Alternate);
        assert_pattern_eq!("a-{b,c}-{d{e,f},g}-h>=1", "a-c-g-h-2", Alternate);
    }
    #[test]
    fn alternate_match_notok() {
        use super::PatternType::Alternate;
        assert_pattern_ne!("a-{b,c}-{d{e,f},g}-h>=1", "a-a-g-h-2", Alternate);
        assert_pattern_ne!("a-{b,c}-{d{e,f},g}-h>=1", "a-b-d-h-2", Alternate);
    }
    #[test]
    fn alternate_match_err() {
        use super::PatternError::Alternate;
        assert_pattern_err!("foo}>=1", Alternate);
        assert_pattern_err!("{foo,bar}}>=1", Alternate);
        assert_pattern_err!("{{foo,bar}>=1", Alternate);
        assert_pattern_err!("}foo,bar}>=1", Alternate);
    }

    /*
     * "Dewey" matches.  Has nothing to do with the Dewey Decimal system, just
     * means a range match.
     */
    #[test]
    fn dewey_match_ok() {
        use super::PatternType::Dewey;
        assert_pattern_eq!("foo>1", "foo-1.1", Dewey);
        assert_pattern_eq!("foo>1", "foo-1.0pl1", Dewey);
        assert_pattern_eq!("foo<1", "foo-1.0alpha1", Dewey);
        assert_pattern_eq!("foo>=1", "foo-1.0", Dewey);
        assert_pattern_eq!("foo<2", "foo-1.0", Dewey);
        assert_pattern_eq!("foo>=1", "foo-1.0", Dewey);
        assert_pattern_eq!("foo>=1<2", "foo-1.0", Dewey);
        assert_pattern_eq!("foo>1<2", "foo-1.0nb2", Dewey);
        assert_pattern_eq!("foo>1.1.1<2", "foo-1.22b2", Dewey);
        //
        assert_pattern_eq!("librsvg>=2.12", "librsvg-2.13", Dewey);
        assert_pattern_eq!("librsvg<2.39", "librsvg-2.13", Dewey);
        assert_pattern_eq!("librsvg<2.40", "librsvg-2.13", Dewey);
        assert_pattern_eq!("librsvg<2.43", "librsvg-2.13", Dewey);
        assert_pattern_eq!("librsvg<2.41", "librsvg-2.13", Dewey);
        assert_pattern_eq!("librsvg>=2.12<2.41", "librsvg-2.13", Dewey);
        /*
         * pkg_install quirks.
         */
        assert_pattern_eq!("pkg>=0", "pkg-", Dewey);
        assert_pattern_eq!("foo>1.1", "foo-1.1blah2", Dewey);
        assert_pattern_eq!("foo>1.1a2", "foo-1.1blah2", Dewey);
    }
    #[test]
    fn dewey_match_notok() {
        use super::PatternType::Dewey;
        assert_pattern_ne!("foo>1alpha<2beta", "foo-2.5", Dewey);
        assert_pattern_ne!("foo>1", "foo-0.5", Dewey);
        assert_pattern_ne!("foo>1", "foo-1.0", Dewey);
        assert_pattern_ne!("foo>1", "foo-1.0alpha1", Dewey);
        assert_pattern_ne!("foo>1nb3", "foo-1.0nb2", Dewey);
        assert_pattern_ne!("foo>1<2", "foo-0.5", Dewey);
        assert_pattern_ne!("bar>=1", "foo-1.0", Dewey);
        assert_pattern_ne!("foo>=1", "foo", Dewey);
        /*
         * pkg_install quirks.
         */
        // XXX: this currently passes, pkg_match does not
        //assert_pattern_eq!("pkg>=0", "pkg", Dewey);
        assert_pattern_ne!("foo>1.1c2", "foo-1.1blah2", Dewey);
    }
    #[test]
    fn dewey_match_err() {
        use super::PatternError::Dewey;
        /* Must be no more than 1 of each direction operator. */
        assert_pattern_err!("foo>1<2<3", Dewey(_));
        /* Greater than must come before less than. */
        assert_pattern_err!("foo<2>3", Dewey(_));

        /*
         * Verify position of error.  To make things simple it always points
         * to the start of the pattern rather than any specific character, as
         * the original intent may not be obvious and the first operator may
         * be correct.
         */
        if let Err(Dewey(e)) = Pattern::new("<>") {
            assert_eq!(e.pos, 0);
        } else {
            panic!();
        }
        if let Err(Dewey(e)) = Pattern::new("foo>=1>2") {
            assert_eq!(e.pos, 3);
        } else {
            panic!();
        }
        if let Err(Dewey(e)) = Pattern::new("pkg>=1<2<4") {
            assert_eq!(e.pos, 8);
        } else {
            panic!();
        }

        /* Version component overflow (exceeds i64::MAX). */
        if let Err(Dewey(e)) = Pattern::new("pkg>=20251208143052123456") {
            assert_eq!(e.msg, "Version component overflow");
        } else {
            panic!();
        }
    }

    /*
     * Glob tests.  These are delegated to the glob crate.
     */
    #[test]
    fn glob_match_ok() {
        use super::PatternType::Glob;
        assert_pattern_eq!("foo-[0-9]*", "foo-1.0", Glob);
        assert_pattern_eq!("fo?-[0-9]*", "foo-1.0", Glob);
        assert_pattern_eq!("fo*-[0-9]*", "foo-1.0", Glob);
        assert_pattern_eq!("?oo-[0-9]*", "foo-1.0", Glob);
        assert_pattern_eq!("*oo-[0-9]*", "foo-1.0", Glob);
        assert_pattern_eq!("foo-[0-9]", "foo-1", Glob);
    }

    #[test]
    fn glob_match_notok() {
        use super::PatternType::Glob;
        assert_pattern_ne!("boo-[0-9]*", "foo-1.0", Glob);
        assert_pattern_ne!("bo?-[0-9]*", "foo-1.0", Glob);
        assert_pattern_ne!("bo*-[0-9]*", "foo-1.0", Glob);
        assert_pattern_ne!("foo-[2-9]*", "foo-1.0", Glob);
        assert_pattern_ne!("fo-[0-9]*", "foo-1.0", Glob);
        assert_pattern_ne!("bar-[0-9]*", "foo-1.0", Glob);
    }
    #[test]
    fn glob_match_err() {
        use super::PatternError::Glob;
        assert_pattern_err!("foo-[0-9", Glob(_));
        /* Apparently *** is an error in the glob crate. */
        assert_pattern_err!("foo-[0-9]***", Glob(_));
    }

    /*
     * Simple package matches.  Not as much to test, either string matches or
     * not.
     */
    #[test]
    fn simple_match() {
        use super::PatternType::Simple;
        assert_pattern_eq!("foo-1.0", "foo-1.0", Simple);
        assert_pattern_ne!("foo-1.1", "foo-1.0", Simple);
        assert_pattern_ne!("bar-1.0", "foo-1.0", Simple);
    }

    #[test]
    fn best_match_dewey() -> Result<(), PatternError> {
        let m = Pattern::new("pkg>1<3")?;
        assert_eq!(m.best_match("pkg-1.1", "pkg-3.0")?, Some("pkg-1.1"));
        assert_eq!(m.best_match("pkg-1.1", "pkg-1.1")?, Some("pkg-1.1"));
        assert_eq!(m.best_match("pkg-1.1", "pkg-2.0")?, Some("pkg-2.0"));
        assert_eq!(m.best_match("pkg-2.0", "pkg-1.1")?, Some("pkg-2.0"));
        assert_eq!(m.best_match("pkg", "pkg-2.0")?, Some("pkg-2.0"));
        assert_eq!(m.best_match("pkg-2.0", "pkg")?, Some("pkg-2.0"));
        assert_eq!(m.best_match("pkg-1", "pkg-3.0")?, None);
        assert_eq!(m.best_match("pkg", "pkg")?, None);
        Ok(())
    }

    #[test]
    fn best_match_alternate() -> Result<(), PatternError> {
        let m = Pattern::new("{foo,bar}-[0-9]*")?;
        assert_eq!(m.best_match("foo-1.1", "bar-1.0")?, Some("foo-1.1"));
        assert_eq!(m.best_match("foo-1.0", "bar-1.1")?, Some("bar-1.1"));
        // In the case of a tie pkg_order() returns the _smaller_ string,
        // which feels backwards, but we aim to preserve compatibility.
        assert_eq!(m.best_match("foo-1.0", "bar-1.0")?, Some("bar-1.0"));
        Ok(())
    }

    #[test]
    fn best_match_order() -> Result<(), PatternError> {
        let m = Pattern::new("mpg123{,-esound,-nas}>=0.59.18")?;
        let pkg1 = "mpg123-1";
        let pkg2 = "mpg123-esound-1";
        let pkg3 = "mpg123-nas-1";
        // pkg_install pkg_order returns the smaller string on tie.
        assert_eq!(m.best_match(pkg1, pkg2)?, Some(pkg1));
        assert_eq!(m.best_match(pkg2, pkg1)?, Some(pkg1));
        assert_eq!(m.best_match(pkg2, pkg3)?, Some(pkg2));
        assert_eq!(m.best_match(pkg3, pkg2)?, Some(pkg2));
        assert_eq!(m.best_match(pkg1, pkg3)?, Some(pkg1));
        assert_eq!(m.best_match(pkg3, pkg1)?, Some(pkg1));
        // pbulk pkg_order() returns the greater string on tie.
        assert_eq!(m.best_match_pbulk(pkg1, pkg2)?, Some(pkg2));
        assert_eq!(m.best_match_pbulk(pkg2, pkg1)?, Some(pkg2));
        assert_eq!(m.best_match_pbulk(pkg2, pkg3)?, Some(pkg3));
        assert_eq!(m.best_match_pbulk(pkg3, pkg2)?, Some(pkg3));
        assert_eq!(m.best_match_pbulk(pkg1, pkg3)?, Some(pkg3));
        assert_eq!(m.best_match_pbulk(pkg3, pkg1)?, Some(pkg3));
        Ok(())
    }

    #[test]
    fn best_match_overflow() -> Result<(), PatternError> {
        let m = Pattern::new("pkg-[0-9]*")?;
        // Timestamp with microseconds exceeds i64::MAX
        let overflow_ver = "pkg-20251208143052123456";
        // Both packages match the glob pattern
        assert!(m.matches("pkg-1.0"));
        assert!(m.matches(overflow_ver));
        // But best_match should fail when comparing versions
        assert!(matches!(
            m.best_match("pkg-1.0", overflow_ver),
            Err(PatternError::Dewey(_))
        ));
        Ok(())
    }

    #[test]
    fn display() {
        let p = Pattern::new("foo-[0-9]*").unwrap();
        assert_eq!(p.to_string(), "foo-[0-9]*");

        let p = Pattern::new("pkg>=1.0<2.0").unwrap();
        assert_eq!(format!("{p}"), "pkg>=1.0<2.0");
    }

    #[test]
    fn from_str() {
        use std::str::FromStr;

        let p = Pattern::from_str("foo-[0-9]*").unwrap();
        assert!(p.matches("foo-1.0"));

        let p: Pattern = "pkg>=1.0".parse().unwrap();
        assert!(p.matches("pkg-1.5"));

        assert!(Pattern::from_str("{unbalanced").is_err());
    }

    #[test]
    fn pattern_accessor() {
        let p = Pattern::new("foo-[0-9]*").unwrap();
        assert_eq!(p.pattern(), "foo-[0-9]*");

        let p = Pattern::new("{mysql,mariadb}-[0-9]*").unwrap();
        assert_eq!(p.pattern(), "{mysql,mariadb}-[0-9]*");
    }

    #[test]
    fn quick_pkg_match_edge_cases() {
        // Pattern starting with glob char - quick_pkg_match returns true early
        let p = Pattern::new("*-1.0").unwrap();
        assert!(p.matches("foo-1.0"));

        // Pattern starting with ? - quick_pkg_match returns true early
        let p = Pattern::new("?oo-[0-9]*").unwrap();
        assert!(p.matches("foo-1.0"));

        // Single char pattern
        let p = Pattern::new("f*").unwrap();
        assert!(p.matches("foo"));

        // First char mismatch - quick_pkg_match returns false at first char
        let p = Pattern::new("bar-[0-9]*").unwrap();
        assert!(!p.matches("foo-1.0"));

        // Second char mismatch - quick_pkg_match returns false at second char
        let p = Pattern::new("fa-[0-9]*").unwrap();
        assert!(!p.matches("fo-1.0"));

        // Both chars match but glob fails
        let p = Pattern::new("fo-[0-9]*").unwrap();
        assert!(!p.matches("foo-1.0"));
    }

    #[test]
    fn pattern_pkgbase() -> Result<(), PatternError> {
        // Glob patterns
        let p = Pattern::new("foo-[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("foo"));
        let p = Pattern::new("mpg123-nas-[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("mpg123-nas"));
        let p = Pattern::new("foo-1.[0-9]*")?;
        assert_eq!(p.pkgbase(), None);
        let p = Pattern::new("foo-bar*-1")?;
        assert_eq!(p.pkgbase(), None);
        let p = Pattern::new("*-1.0")?;
        assert_eq!(p.pkgbase(), None);
        let p = Pattern::new("fo?-[0-9]*")?;
        assert_eq!(p.pkgbase(), None);

        // Alternate patterns
        let p = Pattern::new("{foo,bar}-[0-9]*")?;
        assert_eq!(p.pkgbase(), None);

        // Dewey patterns
        let p = Pattern::new("foo>=1.0")?;
        assert_eq!(p.pkgbase(), Some("foo"));
        let p = Pattern::new("pkg-name>=2.0<3.0")?;
        assert_eq!(p.pkgbase(), Some("pkg-name"));

        // Simple patterns
        let p = Pattern::new("foo-1.0")?;
        assert_eq!(p.pkgbase(), Some("foo"));
        let p = Pattern::new("pkg-name-2.0nb1")?;
        assert_eq!(p.pkgbase(), Some("pkg-name"));

        Ok(())
    }
}
