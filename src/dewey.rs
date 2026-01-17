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

/*! Dewey decimal version comparison. */

use std::cmp::Ordering;
use thiserror::Error;

/**
 * A [`Dewey`] pattern parsing error.
 */
#[derive(Debug, Error)]
#[error("Pattern syntax error near position {pos}: {msg}")]
pub struct DeweyError {
    /// The approximate character index of where the error occurred.
    pub pos: usize,

    /// A message describing the error.
    pub msg: &'static str,
}

/*
 * Comparison operators for Dewey version matching.
 *
 * Note: pkg_install implements == and != operators but doesn't actually
 * support them (or document them), so we don't bother.
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) enum DeweyOp {
    LE,
    LT,
    GE,
    GT,
}

/*
 * DeweyVersion splits a version string into a vec of integers and a separate
 * PKGREVISION that can be compared against.
 *
 * This is a combined version of pkg_install dewey.c's mkversion() and
 * mkcomponent().
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct DeweyVersion {
    version: Vec<i64>,
    pkgrevision: i64,
}

impl DeweyVersion {
    /*
     * Create a new DeweyVersion from a string.  Returns DeweyError if a
     * version component overflows i64.
     */
    pub fn new(s: &str) -> Result<Self, DeweyError> {
        let mut version: Vec<i64> = vec![];
        let mut pkgrevision = 0;
        let mut idx = 0;

        /*
         * Incrementally loop through the pattern, looking for supported version
         * components and pushing them onto the vec.  To remain compatible with
         * pkg_install's dewey.c:mkcomponent() anything that is not matched is
         * ignored.
         */
        loop {
            if idx == s.len() {
                break;
            }

            /* idx should always be incremented by the correct char length. */
            let slice = &s[idx..];
            let c = slice.chars().next().unwrap();

            /*
             * Handle the most common cases first - digits and separators.
             */
            let numstr: String =
                slice.chars().take_while(char::is_ascii_digit).collect();
            if !numstr.is_empty() {
                let num = numstr.parse::<i64>().map_err(|_| DeweyError {
                    pos: idx,
                    msg: "Version component overflow",
                })?;
                version.push(num);
                idx += numstr.len();
                continue;
            }
            if c == '.' || c == '_' {
                version.push(0);
                idx += 1;
                continue;
            }

            /*
             * PKGREVISION denoted by nb<x>.  If <x> is missing then 0.
             */
            if slice.starts_with("nb") {
                idx += 2;
                let slice = &s[idx..];
                let nbstr: String =
                    slice.chars().take_while(char::is_ascii_digit).collect();
                pkgrevision = nbstr.parse::<i64>().unwrap_or(0);
                idx += nbstr.len();
                continue;
            }

            /*
             * Supported modifiers and their weightings so that they are ordered
             * correctly.
             */
            if slice.starts_with("alpha") {
                version.push(-3);
                idx += 5;
                continue;
            } else if slice.starts_with("beta") {
                version.push(-2);
                idx += 4;
                continue;
            } else if slice.starts_with("pre") {
                version.push(-1);
                idx += 3;
                continue;
            } else if slice.starts_with("rc") {
                version.push(-1);
                idx += 2;
                continue;
            } else if slice.starts_with("pl") {
                version.push(0);
                idx += 2;
                continue;
            }

            /*
             * Finally, encode any ASCII alphabetic characters as a 0 followed by
             * their ASCII code, otherwise completely ignore any non-ASCII
             * characters, making sure to correctly handle multibyte characters.
             *
             * Reuse "c" from above.
             */
            if c.is_ascii_alphabetic() {
                version.push(0);
                version.push(c as i64);
                idx += 1;
            } else {
                idx += c.len_utf8();
            }
        }

        Ok(Self {
            version,
            pkgrevision,
        })
    }
}

/*
 * DeweyMatch contains a single pattern to match against.
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct DeweyMatch {
    op: DeweyOp,
    version: DeweyVersion,
}

impl DeweyMatch {
    fn new(op: &DeweyOp, pattern: &str) -> Result<Self, DeweyError> {
        let version = DeweyVersion::new(pattern)?;
        Ok(Self {
            op: op.clone(),
            version,
        })
    }
}

/**
 * Package pattern matching for so-called "dewey" patterns.
 *
 * These are common across pkgsrc as a way to specify a range of versions for
 * a package.  Despite the name, these have nothing to do with the Dewey
 * decimal system.
 *
 * It is unlikely that anyone would want to use this directly.  The main
 * user-facing interface is [`Pattern`] which will handle any patterns
 * matching [`Dewey`] style automatically.  However, in case it proves at all
 * useful, it is made public.
 *
 * This fully supports the same modifiers and logic that [`pkg_install`] does,
 * according to the following rules:
 *
 *    Modifier(s) | Numeric value
 * ---------------|--------
 *       `alpha`  | `-3`
 *       `beta`   | `-2`
 *    `pre`, `rc` | `-1`
 * `pl`, `_`, `.` | `0`
 *    empty value | `0`
 *
 * # Examples
 *
 * ```
 * use pkgsrc::Dewey;
 *
 * // A version greater than or equal to 1.0 and less than 2.0 is required.
 * let m = Dewey::new("pkg>=1.0<2");
 *
 * // A common way to specify that any version is ok.
 * let m = Dewey::new("pkg>=0");
 *
 * // Any version as long as it is earlier than 7.
 * let m = Dewey::new("windows<7");
 * ```
 *
 * [`pkg_install`]:
 * https://github.com/NetBSD/pkgsrc/blob/trunk/pkgtools/pkg_install/files/lib/dewey.c
 * [`Pattern`]: crate::Pattern
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Dewey {
    pkgbase: String,
    matches: Vec<DeweyMatch>,
}

impl Dewey {
    /**
     * Compile a pattern.  If the pattern is invalid in any way a
     * [`DeweyError`] is returned.
     *
     * # Examples
     *
     * ```
     * use pkgsrc::Dewey;
     *
     * // A correctly specified range.
     * assert!(Dewey::new("pkg>=1.0<2").is_ok());
     *
     * // Incorrect order of operators.
     * assert!(Dewey::new("pkg<1>2").is_err());
     *
     * // Invalid use of incompatible operators.
     * assert!(Dewey::new("pkg>1>=2").is_err());
     * ```
     *
     * # Errors
     *
     * Returns [`DeweyError`] if the pattern is invalid.
     */
    pub fn new(pattern: &str) -> Result<Self, DeweyError> {
        /*
         * Search through the pattern looking for dewey match operators and
         * their indices.  Push a tuple containing the start of the pattern,
         * the start of the version part of the pattern, and the DeweyOp used
         * onto the matches vec for any found.
         */
        let mut deweyops: Vec<(usize, usize, DeweyOp)> = vec![];
        for (index, matched) in pattern.match_indices(&['>', '<']) {
            match (matched, pattern.get(index + 1..index + 2)) {
                (">", Some("=")) => {
                    deweyops.push((index, index + 2, DeweyOp::GE));
                }
                ("<", Some("=")) => {
                    deweyops.push((index, index + 2, DeweyOp::LE));
                }
                (">", _) => deweyops.push((index, index + 1, DeweyOp::GT)),
                ("<", _) => deweyops.push((index, index + 1, DeweyOp::LT)),
                _ => unreachable!(),
            }
        }

        /*
         * Verify that the pattern follows the rules:
         *
         * - Must be at least one operator but no more than two.
         * - If two operators are specified then the first must be GT/GE and
         *   the second LT/LE.
         * - Only ASCII characters are supported.
         *
         * For each valid pattern, push a new DeweyMatch onto the matches vec.
         */
        let mut matches: Vec<DeweyMatch> = vec![];
        match deweyops.len() {
            0 => {
                return Err(DeweyError {
                    pos: 0,
                    msg: "No dewey operators found",
                });
            }
            1 => {
                let p = &pattern[deweyops[0].1..];
                matches.push(DeweyMatch::new(&deweyops[0].2, p)?);
            }
            2 => {
                match (&deweyops[0].2, &deweyops[1].2) {
                    (DeweyOp::GT | DeweyOp::GE, DeweyOp::LT | DeweyOp::LE) => {}
                    _ => {
                        return Err(DeweyError {
                            pos: deweyops[0].0,
                            msg: "Unsupported operator order",
                        });
                    }
                }
                let p = &pattern[deweyops[0].1..deweyops[1].0];
                matches.push(DeweyMatch::new(&deweyops[0].2, p)?);
                let p = &pattern[deweyops[1].1..];
                matches.push(DeweyMatch::new(&deweyops[1].2, p)?);
            }
            _ => {
                return Err(DeweyError {
                    pos: deweyops[2].0,
                    msg: "Too many dewey operators found",
                });
            }
        }

        /*
         * At this point we know we have at least one valid match, extract the
         * pkgbase and return all matches.
         */
        let pkgbase = pattern[0..deweyops[0].0].to_string();
        Ok(Self { pkgbase, matches })
    }

    /**
     * Return whether a given [`str`] matches the compiled pattern.  `pkg`
     * must be a fully-specified `PKGNAME`.
     *
     * # Examples
     *
     * ```
     * use pkgsrc::Dewey;
     *
     * let m = Dewey::new("pkg>=1.0<2").unwrap();
     * assert_eq!(m.matches("pkg-1.0rc1"), false);
     * assert_eq!(m.matches("pkg-1.0"), true);
     * assert_eq!(m.matches("pkg-2.0rc1"), true);
     * assert_eq!(m.matches("pkg-2.0"), false);
     * ```
     */
    #[must_use]
    pub fn matches(&self, pkg: &str) -> bool {
        let Some((base, version)) = pkg.rsplit_once('-') else {
            return false;
        };
        if base != self.pkgbase {
            return false;
        }
        let Ok(pkgver) = DeweyVersion::new(version) else {
            return false;
        };
        for m in &self.matches {
            if !dewey_cmp(&pkgver, &m.op, &m.version) {
                return false;
            }
        }
        true
    }

    /**
     * Return the `PKGBASE` name from this pattern.
     */
    #[must_use]
    pub fn pkgbase(&self) -> &str {
        &self.pkgbase
    }
}

/*
 * Compare two i64s using the specified operator.
 */
const fn dewey_test(lhs: i64, op: &DeweyOp, rhs: i64) -> bool {
    match op {
        DeweyOp::GE => lhs >= rhs,
        DeweyOp::GT => lhs > rhs,
        DeweyOp::LE => lhs <= rhs,
        DeweyOp::LT => lhs < rhs,
    }
}

/*
 * Compare two DeweyVersions using the specified operator.  This iterates
 * through both vecs, skipping entries that are identical, and comparing any
 * that differ.  If the vecs differ in length, perform the remaining
 * comparisons against zero.
 *
 * If both versions are identical, the PKGREVISION is compared as the final
 * result.
 */
pub(crate) fn dewey_cmp(
    lhs: &DeweyVersion,
    op: &DeweyOp,
    rhs: &DeweyVersion,
) -> bool {
    let llen = lhs.version.len();
    let rlen = rhs.version.len();
    for i in 0..std::cmp::min(llen, rlen) {
        if lhs.version[i] != rhs.version[i] {
            return dewey_test(lhs.version[i], op, rhs.version[i]);
        }
    }
    match llen.cmp(&rlen) {
        Ordering::Less => {
            for i in llen..rlen {
                if rhs.version[i] != 0 {
                    return dewey_test(0, op, rhs.version[i]);
                }
            }
        }
        Ordering::Greater => {
            for i in rlen..llen {
                if lhs.version[i] != 0 {
                    return dewey_test(lhs.version[i], op, 0);
                }
            }
            return dewey_test(lhs.pkgrevision, op, rhs.pkgrevision);
        }
        Ordering::Equal => {}
    }
    dewey_test(lhs.pkgrevision, op, rhs.pkgrevision)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dewey_version_empty() -> Result<(), DeweyError> {
        let dv = DeweyVersion::new("")?;
        assert_eq!(dv.version, Vec::<i64>::new());
        assert_eq!(dv.pkgrevision, 0);
        Ok(())
    }

    #[test]
    fn dewey_no_operators() {
        let err = Dewey::new("pkg");
        assert!(err.is_err());
        let err = err.unwrap_err();
        assert_eq!(err.pos, 0);
        assert_eq!(err.msg, "No dewey operators found");
    }

    /*
     * Any non-ASCII characters are just skipped.
     */
    #[test]
    fn dewey_version_utf8() -> Result<(), DeweyError> {
        let dv = DeweyVersion::new("é")?;
        assert_eq!(dv.version, Vec::<i64>::new());
        assert_eq!(dv.pkgrevision, 0);
        Ok(())
    }

    #[test]
    fn dewey_version_modifiers() -> Result<(), DeweyError> {
        let dv = DeweyVersion::new("1.0alpha1beta2rc3pl4_5nb17")?;
        assert_eq!(dv.version, vec![1, 0, 0, -3, 1, -2, 2, -1, 3, 0, 4, 0, 5]);
        assert_eq!(dv.pkgrevision, 17);
        // chars replaced with [0, <char code>], - ignored.
        let dv = DeweyVersion::new("ojnknb30_-")?;
        assert_eq!(dv.version, vec![0, 111, 0, 106, 0, 110, 0, 107, 0]);
        assert_eq!(dv.pkgrevision, 30);
        // Ensure "pre" is parsed correctly.
        let m = Dewey::new("spandsp>=0.0.6pre18")?;
        assert!(m.matches("spandsp-0.0.6nb5"));
        assert!(m.matches("spandsp-0.0.6pre19"));
        assert!(m.matches("spandsp-0.0.6rc18"));
        assert!(!m.matches("spandsp-0.0.6rc17"));
        Ok(())
    }

    #[test]
    fn dewey_version_empty_pkgrevision() -> Result<(), DeweyError> {
        let dv = DeweyVersion::new("100nb")?;
        assert_eq!(dv.version, vec![100]);
        assert_eq!(dv.pkgrevision, 0);
        Ok(())
    }

    /*
     * If no version is specified at all it behaves as if it were 0.
     */
    #[test]
    fn dewey_match_no_version() -> Result<(), DeweyError> {
        let m = Dewey::new("pkg>")?;
        assert!(!m.matches("pkg"));
        assert!(!m.matches("pkg-"));
        assert!(!m.matches("pkg-0"));
        assert!(m.matches("pkg-0nb1"));

        let m = Dewey::new("pkg>=")?;
        assert!(!m.matches("pkg"));
        assert!(m.matches("pkg-"));
        Ok(())
    }

    #[test]
    fn dewey_match_range() -> Result<(), DeweyError> {
        let m = Dewey::new("pkg>1.0alpha3nb2<2.0beta4nb7")?;
        assert!(m.matches("pkg-1.1"));
        assert!(!m.matches("pkg-1.0alpha3nb2"));
        assert!(m.matches("pkg-1.0alpha3nb3"));
        assert!(m.matches("pkg-2.0alpha3nb3"));
        assert!(m.matches("pkg-2.0beta3nb8"));
        assert!(!m.matches("pkg-2.0beta5nb6"));
        assert!(!m.matches("pkg-2.0beta4nb7"));
        assert!(!m.matches("pkg-2.0"));
        assert!(!m.matches("pkg-2.0nb1"));
        assert!(!m.matches("pkg-2.0nb8"));
        Ok(())
    }

    /*
     * Ensure that comparisons between versions of differing lengths are
     * calculated correctly.
     */
    #[test]
    fn dewey_match_length() -> Result<(), DeweyError> {
        let m = Dewey::new("pkg>1.0.0.0alphanb1")?;
        assert!(m.matches("pkg-1"));
        assert!(m.matches("pkg-1.0"));
        assert!(m.matches("pkg-1.0.0"));
        assert!(m.matches("pkg-1.0.0."));
        assert!(m.matches("pkg-1.0.0.0"));
        assert!(m.matches("pkg-1.0.0.0alpha1"));
        assert!(m.matches("pkg-1.0.0.0alpha1nb0"));
        assert!(m.matches("pkg-1.0.0.0alphanb2"));
        assert!(m.matches("pkg-1.0.0.0."));
        assert!(m.matches("pkg-1.0.0.0_"));
        assert!(m.matches("pkg-1.0.0.0beta"));
        assert!(m.matches("pkg-1.0.0.0rc"));
        assert!(m.matches("pkg-1.0.0.0nb1"));
        assert!(!m.matches("pkg-1.0.0.0alphanb1"));
        assert!(!m.matches("pkg-1.0.0.0alpha"));
        assert!(!m.matches("pkg-1.0.0.beta"));
        assert!(!m.matches("pkg-1.0.0alpha"));
        assert!(m.matches("pkg-1.0.1"));
        assert!(!m.matches("pkg-1.0alpha"));
        Ok(())
    }

    /*
     * Version numbers are currently constrained to i64.
     */
    #[test]
    fn dewey_pattern_overflow() {
        let err = Dewey::new("pkg>=0.20251208143052000000");
        assert!(err.is_err());
        let err = err.unwrap_err();
        assert_eq!(err.msg, "Version component overflow");
    }

    #[test]
    fn dewey_version_overflow() {
        let err = DeweyVersion::new("20251208143052000000");
        assert!(err.is_err());
        let err = err.unwrap_err();
        assert_eq!(err.pos, 0);
        assert_eq!(err.msg, "Version component overflow");
    }

    #[test]
    fn dewey_version_overflow_position() {
        let err = DeweyVersion::new("1.20251208143052000000");
        assert!(err.is_err());
        let err = err.unwrap_err();
        assert_eq!(err.pos, 2);
        assert_eq!(err.msg, "Version component overflow");
    }

    #[test]
    fn dewey_matches_version_overflow() -> Result<(), DeweyError> {
        let m = Dewey::new("pkg>=1.0")?;
        assert!(!m.matches("pkg-20251208143052000000"));
        Ok(())
    }

    #[test]
    fn dewey_matches_no_hyphen() -> Result<(), DeweyError> {
        let m = Dewey::new("pkg>=1.0")?;
        assert!(!m.matches("pkg1.0"));
        Ok(())
    }

    #[test]
    fn dewey_pkgbase() -> Result<(), DeweyError> {
        let m = Dewey::new("my-package>=1.0")?;
        assert_eq!(m.pkgbase(), "my-package");
        assert!(!m.matches("other-package-1.0"));
        Ok(())
    }

    #[test]
    fn dewey_lt_operator() -> Result<(), DeweyError> {
        let m = Dewey::new("pkg<2.0")?;
        assert!(m.matches("pkg-1.0"));
        assert!(m.matches("pkg-1.9"));
        assert!(!m.matches("pkg-2.0"));
        assert!(!m.matches("pkg-3.0"));
        Ok(())
    }

    #[test]
    fn dewey_le_operator() -> Result<(), DeweyError> {
        let m = Dewey::new("pkg<=2.0")?;
        assert!(m.matches("pkg-1.0"));
        assert!(m.matches("pkg-2.0"));
        assert!(!m.matches("pkg-2.1"));
        assert!(!m.matches("pkg-3.0"));
        Ok(())
    }
}
