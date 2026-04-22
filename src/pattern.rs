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
 * Package pattern matching with globs and version constraints.
 *
 * Pattern matching is fundamental to pkgsrc's dependency system. When a package
 * declares a dependency like `DEPENDS+=mktool-[0-9]*:../../pkgtools/mktool`, the
 * pattern `mktool-[0-9]*` specifies which versions of `mktool` satisfy the
 * dependency.
 *
 * This module supports all pattern types used across pkgsrc:
 *
 * # Pattern Types
 *
 * ## Glob Patterns
 *
 * The most common pattern type, using standard UNIX glob syntax:
 *
 * - `*` matches any sequence of characters
 * - `?` matches any single character
 * - `[...]` matches any character in the set
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * // Match any version of mktool
 * let p = Pattern::new("mktool-[0-9]*")?;
 * assert!(p.matches("mktool-1.4.2"));
 * assert!(p.matches("mktool-2.0"));
 * assert!(!p.matches("mktool-abc"));  // doesn't start with digit
 * # Ok::<(), pkgsrc::PatternError>(())
 * ```
 *
 * ## Dewey Patterns
 *
 * Version range patterns using comparison operators (`>`, `>=`, `<`, `<=`):
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * // Require librsvg 2.12.x through 2.40.x
 * let p = Pattern::new("librsvg>=2.12<2.41")?;
 * assert!(!p.matches("librsvg-2.11"));
 * assert!(p.matches("librsvg-2.40.21"));
 * assert!(!p.matches("librsvg-2.41"));
 * # Ok::<(), pkgsrc::PatternError>(())
 * ```
 *
 * ## Alternate Patterns
 *
 * csh-style brace expansion for matching multiple package names:
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * // Accept any MySQL-compatible database
 * let p = Pattern::new("{mysql,mariadb,percona}-client-[0-9]*")?;
 * assert!(p.matches("mysql-client-8.0.36"));
 * assert!(p.matches("mariadb-client-11.4.3"));
 * assert!(!p.matches("postgresql-client-16.4"));
 * # Ok::<(), pkgsrc::PatternError>(())
 * ```
 *
 * ## Simple Patterns
 *
 * Exact string matches (rarely used):
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * let p = Pattern::new("specific-pkg-1.0")?;
 * assert!(p.matches("specific-pkg-1.0"));
 * assert!(!p.matches("specific-pkg-1.1"));
 * # Ok::<(), pkgsrc::PatternError>(())
 * ```
 *
 * # Best Match Selection
 *
 * When multiple packages match a pattern, use [`Pattern::best_match`] to select
 * the highest version:
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * let p = Pattern::new("pkg-[0-9]*")?;
 * let mut best = None;
 * best = p.best_match(best, "pkg-1.0")?;
 * best = p.best_match(best, "pkg-2.0")?;
 * assert_eq!(best, Some("pkg-2.0"));
 * # Ok::<(), pkgsrc::PatternError>(())
 * ```
 */

use crate::PkgName;
use crate::dewey::{Dewey, DeweyError, DeweyOp, DeweyVersion, dewey_cmp};
use hashbrown::HashMap;
use hashbrown::hash_map::EntryRef;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/**
 * Characters that indicate the start of a glob pattern.
 */
const GLOB_START: [char; 3] = ['*', '?', '['];

/**
 * Characters that indicate the start of a dewey version constraint.
 */
const DEWEY_START: [char; 2] = ['>', '<'];

#[cfg(feature = "serde")]
use serde_with::{DeserializeFromStr, SerializeDisplay};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum PatternType {
    Alternate(Vec<Pattern>),
    Dewey(Dewey),
    Glob(glob::Pattern),
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
 * let m = Pattern::new("mutt-[0-9]*")?;
 * assert_eq!(m.matches("mutt-2.2.13"), true);
 * assert_eq!(m.matches("mutt-vid-1.1"), false);
 * assert_eq!(m.matches("pine-1.0"), false);
 * # Ok::<(), pkgsrc::PatternError>(())
 * ```
 *
 * Next most popular are so-called "dewey" matches.  These are used to test
 * for a specific range of versions.
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * let m = Pattern::new("librsvg>=2.12<2.41")?;
 * assert_eq!(m.matches("librsvg-2.11"), false);
 * assert_eq!(m.matches("librsvg-2.12alpha"), false);
 * assert_eq!(m.matches("librsvg-2.13"), true);
 * assert_eq!(m.matches("librsvg-2.41"), false);
 * # Ok::<(), pkgsrc::PatternError>(())
 * ```
 *
 * Alternate matches are csh-style `{foo,bar}` either/or matches, matching any
 * of the expanded strings.
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * let m = Pattern::new("{mysql,mariadb,percona}-[0-9]*")?;
 * assert_eq!(m.matches("mysql-8.0.36"), true);
 * assert_eq!(m.matches("mariadb-11.4.3"), true);
 * assert_eq!(m.matches("postgresql-16.4"), false);
 * # Ok::<(), pkgsrc::PatternError>(())
 * ```
 *
 * Finally plain, exact string matches can be used, though these are very
 * rare and never recommended.
 *
 * ```
 * use pkgsrc::Pattern;
 *
 * let m = Pattern::new("foobar-1.0")?;
 * assert_eq!(m.matches("foobar-1.0"), true);
 * assert_eq!(m.matches("foobar-1.1"), false);
 * # Ok::<(), pkgsrc::PatternError>(())
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
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(SerializeDisplay, DeserializeFromStr))]
pub struct Pattern {
    matchtype: PatternType,
    pattern: String,
    likely: bool,
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
        if let Some(brace) = pattern.find(['{', '}']) {
            if pattern.as_bytes()[brace] == b'}' {
                return Err(PatternError::Alternate);
            }
            /*
             * Verify that braces are correctly balanced.
             */
            let mut depth = 0usize;
            for ch in pattern.chars() {
                if ch == '{' {
                    depth += 1;
                } else if ch == '}' {
                    if depth == 0 {
                        return Err(PatternError::Alternate);
                    }
                    depth -= 1;
                }
            }
            if depth != 0 {
                return Err(PatternError::Alternate);
            }
            /*
             * Expand the outermost brace group and pre-compile each
             * alternative.  Recursive Pattern::new calls handle any
             * remaining brace groups in the expanded patterns.
             */
            let Some(i) = pattern.rfind('{') else {
                return Err(PatternError::Alternate);
            };
            let (first, rest) = pattern.split_at(i);
            let Some(n) = rest.find('}') else {
                return Err(PatternError::Alternate);
            };
            let (group, last) = rest.split_at(n + 1);
            let alts = &group[1..group.len() - 1];

            let mut expanded = Vec::new();
            for m in alts.split(',') {
                let s = format!("{first}{m}{last}");
                expanded.push(Pattern::new(&s)?);
            }
            return Ok(Self {
                matchtype: PatternType::Alternate(expanded),
                pattern: pattern.to_string(),
                likely: false,
            });
        }
        if pattern.contains(DEWEY_START) {
            return Ok(Self {
                matchtype: PatternType::Dewey(Dewey::new(pattern)?),
                pattern: pattern.to_string(),
                likely: false,
            });
        }
        if pattern.contains(GLOB_START) {
            return Ok(Self {
                matchtype: PatternType::Glob(glob::Pattern::new(pattern)?),
                pattern: pattern.to_string(),
                likely: false,
            });
        }
        Ok(Self {
            matchtype: PatternType::Simple,
            pattern: pattern.to_string(),
            likely: false,
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
     * let pkgmatch = Pattern::new("librsvg>=2.12<2.41")?;
     * assert_eq!(pkgmatch.matches("librsvg"), false);
     * assert_eq!(pkgmatch.matches("librsvg-2.11"), false);
     * assert_eq!(pkgmatch.matches("librsvg-2.13"), true);
     * assert_eq!(pkgmatch.matches("librsvg-2.41"), false);
     * # Ok::<(), pkgsrc::PatternError>(())
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
            PatternType::Alternate(ref patterns) => {
                patterns.iter().any(|p| p.matches(pkg))
            }
            PatternType::Dewey(ref dewey) => dewey.matches(pkg),
            PatternType::Glob(ref glob) => glob.matches(pkg),
            PatternType::Simple => self.pattern == pkg,
        }
    }

    /**
     * Accumulate the best matching package from a sequence of candidates.
     *
     * Given the current best (or [`None`] if no match yet) and a new
     * candidate, return the updated best.  Only `candidate` is tested
     * against the pattern; `current` is assumed to already match.
     *
     * When versions compare equal, the lexicographically smaller string
     * is returned, to match pkg_install's `pkg_order()`.
     *
     * # Errors
     *
     * Returns [`PatternError::Dewey`] if parsing a package version fails.
     */
    pub fn best_match<'a>(
        &self,
        current: Option<&'a str>,
        candidate: &'a str,
    ) -> Result<Option<&'a str>, PatternError> {
        self.best_match_cmp(current, candidate, std::cmp::Ordering::Less)
    }

    /**
     * Identical to [`Pattern::best_match`] except when versions compare
     * equal, the lexicographically greater string is returned to match
     * pbulk's `pkg_order()`.
     *
     * # Errors
     *
     * Returns [`PatternError::Dewey`] if parsing a package version fails.
     */
    pub fn best_match_pbulk<'a>(
        &self,
        current: Option<&'a str>,
        candidate: &'a str,
    ) -> Result<Option<&'a str>, PatternError> {
        self.best_match_cmp(current, candidate, std::cmp::Ordering::Greater)
    }

    fn best_match_cmp<'a>(
        &self,
        current: Option<&'a str>,
        candidate: &'a str,
        tiebreak: std::cmp::Ordering,
    ) -> Result<Option<&'a str>, PatternError> {
        if !self.matches(candidate) {
            return Ok(current);
        }
        let Some(current) = current else {
            return Ok(Some(candidate));
        };
        let d1 = DeweyVersion::new(PkgName::new(current).pkgversion())?;
        let d2 = DeweyVersion::new(PkgName::new(candidate).pkgversion())?;
        if dewey_cmp(&d1, &DeweyOp::GT, &d2) {
            Ok(Some(current))
        } else if dewey_cmp(&d1, &DeweyOp::LT, &d2) {
            Ok(Some(candidate))
        } else if current.cmp(candidate) == tiebreak {
            Ok(Some(current))
        } else {
            Ok(Some(candidate))
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
            PatternType::Dewey(ref dewey) => Some(dewey.pkgbase()),
            PatternType::Simple => {
                self.pattern.rsplit_once('-').map(|(b, _)| b)
            }
            PatternType::Glob(_) => {
                let end =
                    self.pattern.find(GLOB_START).unwrap_or(self.pattern.len());
                let prefix = &self.pattern[..end];
                prefix.strip_suffix('-').or_else(|| {
                    let (base, ver) = prefix.rsplit_once('-')?;
                    (!ver.is_empty()
                        && ver.chars().all(|c| c.is_ascii_digit() || c == '.'))
                    .then_some(base)
                })
            }
            PatternType::Alternate(_) => None,
        }
    }

    /**
     * `pkg_install` contains a `quick_pkg_match()` routine to quickly exit if
     * there is no possibility of a match. As it gives a decent speed bump
     * when matching across thousands of packages we include a similar routine.
     */
    fn quick_pkg_match(pattern: &str, pkg: &str) -> bool {
        let mut p1 = pattern.chars();
        let mut p2 = pkg.chars();

        let p = p1.next();
        if !p.is_some_and(Self::is_simple_char) {
            return true;
        }
        if p != p2.next() {
            return false;
        }

        let p = p1.next();
        if !p.is_some_and(Self::is_simple_char) {
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

/**
 * A cache of compiled [`Pattern`] objects for efficient reuse.
 *
 * When resolving dependencies across a large package set, the same
 * pattern strings appear repeatedly.  `PatternCache` ensures each
 * unique pattern string is compiled only once.
 *
 * # Example
 *
 * ```
 * use pkgsrc::PatternCache;
 *
 * let mut cache = PatternCache::new();
 * let p = cache.compile("foo-[0-9]*")?;
 * assert!(p.matches("foo-1.0"));
 * let p = cache.compile("foo-[0-9]*")?;
 * assert!(p.matches("foo-2.0"));
 * # Ok::<(), pkgsrc::PatternError>(())
 * ```
 */
#[derive(Debug)]
pub struct PatternCache {
    cache: HashMap<String, Pattern>,
}

impl PatternCache {
    /**
     * Create an empty cache.
     */
    #[must_use]
    pub fn new() -> Self {
        PatternCache {
            cache: HashMap::new(),
        }
    }

    /**
     * Create an empty cache with capacity for the given number of
     * unique patterns.
     */
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        PatternCache {
            cache: HashMap::with_capacity(capacity),
        }
    }

    /**
     * Compile a pattern string, returning a cached reference if the
     * same string was previously compiled.
     *
     * # Errors
     *
     * Returns [`PatternError`] if the pattern is invalid and has not
     * been previously compiled.
     */
    pub fn compile(&mut self, pattern: &str) -> Result<&Pattern, PatternError> {
        match self.cache.entry_ref(pattern) {
            EntryRef::Occupied(e) => Ok(e.into_mut()),
            EntryRef::Vacant(e) => {
                let p = Pattern::new(pattern)?;
                Ok(e.insert(p))
            }
        }
    }

    /**
     * Return the number of cached patterns.
     */
    #[must_use]
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /**
     * Return true if the cache is empty.
     */
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

impl Default for PatternCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_pattern {
        ($pattern:expr, $pkg:expr, $variant:pat, $result:expr) => {
            let p = Pattern::new($pattern)?;
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
    fn alternate_match_ok() -> Result<(), PatternError> {
        use super::PatternType::Alternate;
        assert_pattern_eq!(
            "a-{b,c}-{d{e,f},g}-h>=1",
            "a-b-de-h-2",
            Alternate(_)
        );
        assert_pattern_eq!(
            "a-{b,c}-{d{e,f},g}-h>=1",
            "a-b-de-h-2",
            Alternate(_)
        );
        assert_pattern_eq!(
            "a-{b,c}-{d{e,f},g}-h>=1",
            "a-b-df-h-2",
            Alternate(_)
        );
        assert_pattern_eq!(
            "a-{b,c}-{d{e,f},g}-h>=1",
            "a-b-g-h-2",
            Alternate(_)
        );
        assert_pattern_eq!(
            "a-{b,c}-{d{e,f},g}-h>=1",
            "a-c-de-h-2",
            Alternate(_)
        );
        assert_pattern_eq!(
            "a-{b,c}-{d{e,f},g}-h>=1",
            "a-c-df-h-2",
            Alternate(_)
        );
        assert_pattern_eq!(
            "a-{b,c}-{d{e,f},g}-h>=1",
            "a-c-g-h-2",
            Alternate(_)
        );
        assert_pattern_eq!("foo*{a,b}-[0-9]*", "fooxa-1", Alternate(_));
        Ok(())
    }
    #[test]
    fn alternate_match_notok() -> Result<(), PatternError> {
        use super::PatternType::Alternate;
        assert_pattern_ne!(
            "a-{b,c}-{d{e,f},g}-h>=1",
            "a-a-g-h-2",
            Alternate(_)
        );
        assert_pattern_ne!(
            "a-{b,c}-{d{e,f},g}-h>=1",
            "a-b-d-h-2",
            Alternate(_)
        );
        assert_pattern_ne!("abc{d,e}-[0-9]*", "abz-1.0", Alternate(_));
        Ok(())
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
    fn dewey_match_ok() -> Result<(), PatternError> {
        use super::PatternType::Dewey;
        assert_pattern_eq!("foo>1", "foo-1.1", Dewey(_));
        assert_pattern_eq!("foo>1", "foo-1.0pl1", Dewey(_));
        assert_pattern_eq!("foo<1", "foo-1.0alpha1", Dewey(_));
        assert_pattern_eq!("foo>=1", "foo-1.0", Dewey(_));
        assert_pattern_eq!("foo<2", "foo-1.0", Dewey(_));
        assert_pattern_eq!("foo>=1", "foo-1.0", Dewey(_));
        assert_pattern_eq!("foo>=1<2", "foo-1.0", Dewey(_));
        assert_pattern_eq!("foo>1<2", "foo-1.0nb2", Dewey(_));
        assert_pattern_eq!("foo>1.1.1<2", "foo-1.22b2", Dewey(_));
        //
        assert_pattern_eq!("librsvg>=2.12", "librsvg-2.13", Dewey(_));
        assert_pattern_eq!("librsvg<2.39", "librsvg-2.13", Dewey(_));
        assert_pattern_eq!("librsvg<2.40", "librsvg-2.13", Dewey(_));
        assert_pattern_eq!("librsvg<2.43", "librsvg-2.13", Dewey(_));
        assert_pattern_eq!("librsvg<2.41", "librsvg-2.13", Dewey(_));
        assert_pattern_eq!("librsvg>=2.12<2.41", "librsvg-2.13", Dewey(_));
        /*
         * pkg_install quirks.
         */
        assert_pattern_eq!("pkg>=0", "pkg-", Dewey(_));
        assert_pattern_eq!("foo>1.1", "foo-1.1blah2", Dewey(_));
        assert_pattern_eq!("foo>1.1a2", "foo-1.1blah2", Dewey(_));
        Ok(())
    }
    #[test]
    fn dewey_match_notok() -> Result<(), PatternError> {
        use super::PatternType::Dewey;
        assert_pattern_ne!("foo>1alpha<2beta", "foo-2.5", Dewey(_));
        assert_pattern_ne!("foo>1", "foo-0.5", Dewey(_));
        assert_pattern_ne!("foo>1", "foo-1.0", Dewey(_));
        assert_pattern_ne!("foo>1", "foo-1.0alpha1", Dewey(_));
        assert_pattern_ne!("foo>1nb3", "foo-1.0nb2", Dewey(_));
        assert_pattern_ne!("foo>1<2", "foo-0.5", Dewey(_));
        assert_pattern_ne!("bar>=1", "foo-1.0", Dewey(_));
        assert_pattern_ne!("foo>=1", "foo", Dewey(_));
        /*
         * pkg_install quirks.
         */
        // XXX: this currently passes, pkg_match does not
        //assert_pattern_eq!("pkg>=0", "pkg", Dewey(_));
        assert_pattern_ne!("foo>1.1c2", "foo-1.1blah2", Dewey(_));
        Ok(())
    }
    #[test]
    fn dewey_match_err() -> std::result::Result<(), &'static str> {
        use super::PatternError::Dewey;

        fn dewey_err(
            r: Result<Pattern, PatternError>,
        ) -> std::result::Result<DeweyError, &'static str> {
            match r {
                Err(Dewey(e)) => Ok(e),
                _ => Err("expected Dewey error"),
            }
        }

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
        let e = dewey_err(Pattern::new("<>"))?;
        assert_eq!(e.pos, 0);

        let e = dewey_err(Pattern::new("foo>=1>2"))?;
        assert_eq!(e.pos, 3);

        let e = dewey_err(Pattern::new("pkg>=1<2<4"))?;
        assert_eq!(e.pos, 8);

        /* Version component overflow (exceeds i64::MAX). */
        let e = dewey_err(Pattern::new("pkg>=20251208143052123456"))?;
        assert_eq!(e.msg, "Version component overflow");
        Ok(())
    }

    /*
     * Glob tests.  These are delegated to the glob crate.
     */
    #[test]
    fn glob_match_ok() -> Result<(), PatternError> {
        use super::PatternType::Glob;
        assert_pattern_eq!("foo-[0-9]*", "foo-1.0", Glob(_));
        assert_pattern_eq!("fo?-[0-9]*", "foo-1.0", Glob(_));
        assert_pattern_eq!("fo*-[0-9]*", "foo-1.0", Glob(_));
        assert_pattern_eq!("?oo-[0-9]*", "foo-1.0", Glob(_));
        assert_pattern_eq!("*oo-[0-9]*", "foo-1.0", Glob(_));
        assert_pattern_eq!("foo-[0-9]", "foo-1", Glob(_));
        Ok(())
    }

    #[test]
    fn glob_match_notok() -> Result<(), PatternError> {
        use super::PatternType::Glob;
        assert_pattern_ne!("boo-[0-9]*", "foo-1.0", Glob(_));
        assert_pattern_ne!("bo?-[0-9]*", "foo-1.0", Glob(_));
        assert_pattern_ne!("bo*-[0-9]*", "foo-1.0", Glob(_));
        assert_pattern_ne!("foo-[2-9]*", "foo-1.0", Glob(_));
        assert_pattern_ne!("fo-[0-9]*", "foo-1.0", Glob(_));
        assert_pattern_ne!("bar-[0-9]*", "foo-1.0", Glob(_));
        Ok(())
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
    fn simple_match() -> Result<(), PatternError> {
        use super::PatternType::Simple;
        assert_pattern_eq!("foo-1.0", "foo-1.0", Simple);
        assert_pattern_ne!("foo-1.1", "foo-1.0", Simple);
        assert_pattern_ne!("bar-1.0", "foo-1.0", Simple);
        Ok(())
    }

    #[test]
    fn best_match_dewey() -> Result<(), PatternError> {
        let m = Pattern::new("pkg>1<3")?;
        // Non-matching candidates are ignored
        assert_eq!(m.best_match(None, "pkg-0.5")?, None);
        assert_eq!(m.best_match(None, "pkg-3.0")?, None);
        // First match becomes the best
        assert_eq!(m.best_match(None, "pkg-1.1")?, Some("pkg-1.1"));
        // Higher version wins
        assert_eq!(m.best_match(Some("pkg-1.1"), "pkg-2.0")?, Some("pkg-2.0"));
        assert_eq!(m.best_match(Some("pkg-2.0"), "pkg-1.1")?, Some("pkg-2.0"));
        // Non-matching candidate preserves current best
        assert_eq!(m.best_match(Some("pkg-2.0"), "pkg-3.0")?, Some("pkg-2.0"));
        Ok(())
    }

    #[test]
    fn best_match_alternate() -> Result<(), PatternError> {
        let m = Pattern::new("{foo,bar}-[0-9]*")?;
        assert_eq!(m.best_match(Some("bar-1.0"), "foo-1.1")?, Some("foo-1.1"));
        assert_eq!(m.best_match(Some("foo-1.0"), "bar-1.1")?, Some("bar-1.1"));
        // In the case of a tie pkg_order() returns the _smaller_ string,
        // which feels backwards, but we aim to preserve compatibility.
        assert_eq!(m.best_match(Some("foo-1.0"), "bar-1.0")?, Some("bar-1.0"));
        Ok(())
    }

    #[test]
    fn best_match_order() -> Result<(), PatternError> {
        let m = Pattern::new("mpg123{,-esound,-nas}>=0.59.18")?;
        let pkg1 = "mpg123-1";
        let pkg2 = "mpg123-esound-1";
        let pkg3 = "mpg123-nas-1";
        // pkg_install pkg_order returns the smaller string on tie.
        assert_eq!(m.best_match(Some(pkg1), pkg2)?, Some(pkg1));
        assert_eq!(m.best_match(Some(pkg2), pkg1)?, Some(pkg1));
        assert_eq!(m.best_match(Some(pkg2), pkg3)?, Some(pkg2));
        assert_eq!(m.best_match(Some(pkg3), pkg2)?, Some(pkg2));
        assert_eq!(m.best_match(Some(pkg1), pkg3)?, Some(pkg1));
        assert_eq!(m.best_match(Some(pkg3), pkg1)?, Some(pkg1));
        // pbulk pkg_order() returns the greater string on tie.
        assert_eq!(m.best_match_pbulk(Some(pkg1), pkg2)?, Some(pkg2));
        assert_eq!(m.best_match_pbulk(Some(pkg2), pkg1)?, Some(pkg2));
        assert_eq!(m.best_match_pbulk(Some(pkg2), pkg3)?, Some(pkg3));
        assert_eq!(m.best_match_pbulk(Some(pkg3), pkg2)?, Some(pkg3));
        assert_eq!(m.best_match_pbulk(Some(pkg1), pkg3)?, Some(pkg3));
        assert_eq!(m.best_match_pbulk(Some(pkg3), pkg1)?, Some(pkg3));
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
            m.best_match(Some("pkg-1.0"), overflow_ver),
            Err(PatternError::Dewey(_))
        ));
        Ok(())
    }

    #[test]
    fn display() -> Result<(), PatternError> {
        let p = Pattern::new("foo-[0-9]*")?;
        assert_eq!(p.to_string(), "foo-[0-9]*");

        let p = Pattern::new("pkg>=1.0<2.0")?;
        assert_eq!(format!("{p}"), "pkg>=1.0<2.0");
        Ok(())
    }

    #[test]
    fn from_str() -> Result<(), PatternError> {
        use std::str::FromStr;

        let p = Pattern::from_str("foo-[0-9]*")?;
        assert!(p.matches("foo-1.0"));

        let p: Pattern = "pkg>=1.0".parse()?;
        assert!(p.matches("pkg-1.5"));

        assert!(Pattern::from_str("{unbalanced").is_err());

        let p: Pattern = "foo-[0-9]*".try_into()?;
        assert!(p.matches("foo-1.0"));
        Ok(())
    }

    #[test]
    fn pattern_accessor() -> Result<(), PatternError> {
        let p = Pattern::new("foo-[0-9]*")?;
        assert_eq!(p.pattern(), "foo-[0-9]*");

        let p = Pattern::new("{mysql,mariadb}-[0-9]*")?;
        assert_eq!(p.pattern(), "{mysql,mariadb}-[0-9]*");
        Ok(())
    }

    #[test]
    fn quick_pkg_match_edge_cases() -> Result<(), PatternError> {
        // Pattern starting with glob char - quick_pkg_match returns true early
        let p = Pattern::new("*-1.0")?;
        assert!(p.matches("foo-1.0"));

        // Pattern starting with ? - quick_pkg_match returns true early
        let p = Pattern::new("?oo-[0-9]*")?;
        assert!(p.matches("foo-1.0"));

        // Single char pattern
        let p = Pattern::new("f*")?;
        assert!(p.matches("foo"));

        // First char mismatch - quick_pkg_match returns false at first char
        let p = Pattern::new("bar-[0-9]*")?;
        assert!(!p.matches("foo-1.0"));

        // Second char mismatch - quick_pkg_match returns false at second char
        let p = Pattern::new("fa-[0-9]*")?;
        assert!(!p.matches("fo-1.0"));

        // Both chars match but glob fails
        let p = Pattern::new("fo-[0-9]*")?;
        assert!(!p.matches("foo-1.0"));
        Ok(())
    }

    #[test]
    fn pattern_pkgbase() -> Result<(), PatternError> {
        // Glob patterns
        let p = Pattern::new("foo-[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("foo"));
        let p = Pattern::new("mpg123-nas-[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("mpg123-nas"));
        let p = Pattern::new("foo-1.[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("foo"));
        let p = Pattern::new("foo-bar*-1")?;
        assert_eq!(p.pkgbase(), None);
        let p = Pattern::new("*-1.0")?;
        assert_eq!(p.pkgbase(), None);
        let p = Pattern::new("fo?-[0-9]*")?;
        assert_eq!(p.pkgbase(), None);

        /*
         * Version-pinning globs: the text between the last '-' and the
         * first glob char consists only of digits and dots, so it is
         * treated as a version prefix and the base is extracted.
         */
        let p = Pattern::new("boost-headers-1.90.*")?;
        assert_eq!(p.pkgbase(), Some("boost-headers"));
        let p = Pattern::new("foo-1.[0-9]")?;
        assert_eq!(p.pkgbase(), Some("foo"));
        let p = Pattern::new("foo-1.*")?;
        assert_eq!(p.pkgbase(), Some("foo"));
        let p = Pattern::new("foo-10*")?;
        assert_eq!(p.pkgbase(), Some("foo"));
        let p = Pattern::new("lib2to3-3.1[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("lib2to3"));
        let p = Pattern::new("R-4.*")?;
        assert_eq!(p.pkgbase(), Some("R"));
        let p = Pattern::new("p5-IO-1.2[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("p5-IO"));

        /*
         * Non-version text after the last '-': contains letters or
         * other non-version characters, so the fallback does not apply
         * and pkgbase returns None to force a full scan.
         */
        let p = Pattern::new("foo-bar*")?;
        assert_eq!(p.pkgbase(), None);
        let p = Pattern::new("foo-abc[0-9]*")?;
        assert_eq!(p.pkgbase(), None);
        let p = Pattern::new("foo-2bar*")?;
        assert_eq!(p.pkgbase(), None);
        let p = Pattern::new("foo-1alpha*")?;
        assert_eq!(p.pkgbase(), None);
        let p = Pattern::new("foo-1.0rc*")?;
        assert_eq!(p.pkgbase(), None);

        /*
         * Packages with digits in name components.  These all use the
         * standard "-[0-9]*" form so strip_suffix('-') handles them,
         * but the fallback must also be safe if a version-pinning glob
         * were ever used (e.g. "font-adobe-100dpi-1.*").
         */
        let p = Pattern::new("font-adobe-100dpi-[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("font-adobe-100dpi"));
        let p = Pattern::new("font-adobe-100dpi-1.*")?;
        assert_eq!(p.pkgbase(), Some("font-adobe-100dpi"));
        let p = Pattern::new("fuse-ntfs-3g-[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("fuse-ntfs-3g"));
        let p = Pattern::new("tex-pst-3dplot-[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("tex-pst-3dplot"));
        let p = Pattern::new("tex-2up-[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("tex-2up"));
        let p = Pattern::new("nerd-fonts-3270-[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("nerd-fonts-3270"));
        let p = Pattern::new("u-boot-rpi3-32-[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("u-boot-rpi3-32"));

        /*
         * Real version-pinning patterns from pkgsrc.
         */
        let p = Pattern::new("boost-libs-1.90.*")?;
        assert_eq!(p.pkgbase(), Some("boost-libs"));
        let p = Pattern::new("SDL-1.2.[0-9]*")?;
        assert_eq!(p.pkgbase(), Some("SDL"));
        let p = Pattern::new("mongodb-3*")?;
        assert_eq!(p.pkgbase(), Some("mongodb"));
        let p = Pattern::new("python27-2.7.*")?;
        assert_eq!(p.pkgbase(), Some("python27"));
        let p = Pattern::new("go14-1.4*")?;
        assert_eq!(p.pkgbase(), Some("go14"));
        let p = Pattern::new("go110-1.10.*")?;
        assert_eq!(p.pkgbase(), Some("go110"));
        let p = Pattern::new("mariadb-client-10.11.*")?;
        assert_eq!(p.pkgbase(), Some("mariadb-client"));
        let p = Pattern::new("gcc10-aux-10.*")?;
        assert_eq!(p.pkgbase(), Some("gcc10-aux"));
        let p = Pattern::new("binutils-2.22*")?;
        assert_eq!(p.pkgbase(), Some("binutils"));
        let p = Pattern::new("gtar-base-1.*")?;
        assert_eq!(p.pkgbase(), Some("gtar-base"));

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
