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

use crate::dewey;
use thiserror::Error;

#[derive(Clone, Debug, Default, Hash, PartialEq)]
enum PatternType {
    Alternate,
    Dewey,
    Glob,
    #[default]
    Simple,
}

/**
 * A pattern parsing error.
 */
#[derive(Debug, Error)]
pub enum PatternError {
    /// An alternate pattern was supplied with unbalanced braces.
    #[error("Unbalanced braces in pattern")]
    Alternate,
    /// Transparent [`dewey::DeweyError`]
    #[error(transparent)]
    Dewey(#[from] dewey::DeweyError),
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
 * // Unbalanced or incorrectly-ordered braces.
 * assert!(matches!(Pattern::new("{foo,bar}}>1.0"), Err(Alternate)));
 * assert!(matches!(Pattern::new("foo}b{ar>1.0"), Err(Alternate)));
 * ```
 *
 * [`glob`]: https://docs.rs/glob/latest/glob/
 */
#[derive(Clone, Debug, Default, Hash, PartialEq)]
pub struct Pattern {
    matchtype: PatternType,
    pattern: String,
    likely: bool,
    dewey: Option<dewey::Dewey>,
    glob: Option<glob::Pattern>,
}

impl Pattern {
    /**
     * Compile a pattern.  If the pattern is invalid in any way a
     * [`PatternError`] is returned.
     *
     * # Example
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
            return Ok(Pattern {
                matchtype,
                pattern: pattern.to_string(),
                ..Default::default()
            });
        }
        if pattern.contains('>') || pattern.contains('<') {
            let matchtype = PatternType::Dewey;
            let dewey = Some(dewey::Dewey::new(pattern)?);
            return Ok(Pattern {
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
            return Ok(Pattern {
                matchtype,
                pattern: pattern.to_string(),
                glob,
                ..Default::default()
            });
        }
        Ok(Pattern {
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
     * Return the original pattern string.
     */
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /**
     * Implement csh-style alternate matches.  Pattern::new() has already
     * verified that the pattern is valid and the braces are correctly balanced.
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
                let fmt = format!("{}{}{}", first, m, last);
                if let Ok(pat) = Pattern::new(&fmt) {
                    if pat.matches(pkg) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /**
     * pkg_install contains a quick_pkg_match() routine to quickly exit if
     * there is no possibility of a match.  As it gives a decent speed bump
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

    fn is_simple_char(c: char) -> bool {
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
            assert!(false);
        }
        if let Err(Dewey(e)) = Pattern::new("foo>=1>2") {
            assert_eq!(e.pos, 3);
        } else {
            assert!(false);
        }
        if let Err(Dewey(e)) = Pattern::new("pkg>=1<2<4") {
            assert_eq!(e.pos, 8);
        } else {
            assert!(false);
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
}
