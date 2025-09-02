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

use std::cmp::Ordering;
use std::error::Error;
use std::fmt;

/**
 * A [`Dewey`] pattern parsing error.
 */
#[derive(Debug)]
pub struct DeweyError {
    /// The approximate character index of where the error occurred.
    pub pos: usize,

    /// A message describing the error.
    pub msg: &'static str,
}

impl Error for DeweyError {
    fn description(&self) -> &str {
        self.msg
    }
}

impl fmt::Display for DeweyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Pattern syntax error near position {}: {}",
            self.pos, self.msg
        )
    }
}

/*
 * pkg_install implements "==" (DEWEY_EQ) and "!=" (DEWEY_NE) but doesn't
 * actually support them (or document them), so we don't bother.
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum DeweyOp {
    LE,
    LT,
    GE,
    GT,
}

/**
 * [`DeweyVersion`] splits a version string into a [`Vec`] of integers and a
 * separate `PKGREVISION` that can be compared against.
 *
 * This is a combined version of `pkg_install` dewey.c's `mkversion()` and
 * `mkcomponent()`.
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DeweyVersion {
    version: Vec<i64>,
    pkgrevision: i64,
}

impl DeweyVersion {
    /**
     * Create a new [`DeweyVersion`] from a string.
     */
    pub fn new(s: &str) -> Self {
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
            let slice = &s[idx..s.len()];
            let c = slice.chars().next().unwrap();

            /*
             * Handle the most common cases first - digits and separators.
             */
            let numstr: String =
                slice.chars().take_while(char::is_ascii_digit).collect();
            if !numstr.is_empty() {
                version.push(numstr.parse::<i64>().unwrap());
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
                let slice = &s[idx..s.len()];
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

        DeweyVersion {
            version,
            pkgrevision,
        }
    }
}

/**
 * [`DeweyMatch`] contains a single pattern to match against.
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DeweyMatch {
    /// Which logical operation to apply
    op: DeweyOp,
    /// A vec of version numbers to compare against.
    version: DeweyVersion,
}

impl DeweyMatch {
    fn new(op: &DeweyOp, pattern: &str) -> Result<Self, DeweyError> {
        let version = DeweyVersion::new(pattern);
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
pub struct Dewey {
    pkgname: String,
    matches: Vec<DeweyMatch>,
}

impl Dewey {
    /**
     * Compile a pattern.  If the pattern is invalid in any way a
     * [`DeweyError`] is returned.
     *
     * # Example
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
                /* Cannot happen, appeases the compiler. */
                (&_, _) => todo!(),
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
                let p = &pattern[deweyops[0].1..pattern.len()];
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
                let p = &pattern[deweyops[1].1..pattern.len()];
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
         * pkgname and return all matches.
         */
        let pkgname = pattern[0..deweyops[0].0].to_string();
        Ok(Self { pkgname, matches })
    }

    /**
     * Return whether a given [`str`] matches the compiled pattern.  `pkg`
     * must be a fully-specified `PKGNAME`.
     *
     * # Example
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
        let v: Vec<&str> = pkg.rsplitn(2, '-').collect();
        if v.len() != 2 {
            return false;
        }
        if v[1] != self.pkgname {
            return false;
        }
        let pkgver = DeweyVersion::new(v[0]);
        for m in &self.matches {
            if !dewey_cmp(&pkgver, &m.op, &m.version) {
                return false;
            }
        }
        true
    }
}
/**
 * Compare two [`i64`]s using the specified operator.
 */
const fn dewey_test(lhs: i64, op: &DeweyOp, rhs: i64) -> bool {
    match op {
        DeweyOp::GE => lhs >= rhs,
        DeweyOp::GT => lhs > rhs,
        DeweyOp::LE => lhs <= rhs,
        DeweyOp::LT => lhs < rhs,
    }
}

/**
 * Compare two [`DeweyVersion`]s using the specified operator.  This iterates
 * through both vecs, skipping entries that are identical, and comparing any
 * that differ.  If the vecs differ in length, perform the remaining
 * comparisons against zero.
 *
 * If both versions are identical, the PKGREVISION is compared as the final
 * result.
 */
pub fn dewey_cmp(lhs: &DeweyVersion, op: &DeweyOp, rhs: &DeweyVersion) -> bool {
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
                if 0 != rhs.version[i] {
                    return dewey_test(0, op, rhs.version[i]);
                }
            }
        }
        Ordering::Greater => {
            for i in rlen..llen {
                if 0 != lhs.version[i] {
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
    fn dewey_version_empty() {
        let dv = DeweyVersion::new("");
        assert_eq!(dv.version, Vec::<i64>::new());
        assert_eq!(dv.pkgrevision, 0);
    }

    /*
     * Any non-ASCII characters are just skipped.
     */
    #[test]
    fn dewey_version_utf8() {
        let dv = DeweyVersion::new("é");
        assert_eq!(dv.version, Vec::<i64>::new());
        assert_eq!(dv.pkgrevision, 0);
    }

    #[test]
    fn dewey_version_modifiers() {
        let dv = DeweyVersion::new("1.0alpha1beta2rc3pl4_5nb17");
        assert_eq!(dv.version, vec![1, 0, 0, -3, 1, -2, 2, -1, 3, 0, 4, 0, 5]);
        assert_eq!(dv.pkgrevision, 17);
        // chars replaced with [0, <char code>], - ignored.
        let dv = DeweyVersion::new("ojnknb30_-");
        assert_eq!(dv.version, vec![0, 111, 0, 106, 0, 110, 0, 107, 0]);
        assert_eq!(dv.pkgrevision, 30);
    }

    #[test]
    fn dewey_version_empty_pkgrevision() {
        let dv = DeweyVersion::new("100nb");
        assert_eq!(dv.version, vec![100]);
        assert_eq!(dv.pkgrevision, 0);
    }

    /*
     * If no version is specified at all it behaves as if it were 0.
     */
    #[test]
    fn dewey_match_no_version() {
        let m = Dewey::new("pkg>").unwrap();
        assert!(!m.matches("pkg"));
        assert!(!m.matches("pkg-"));
        assert!(!m.matches("pkg-0"));
        assert!(m.matches("pkg-0nb1"));

        let m = Dewey::new("pkg>=").unwrap();
        assert!(!m.matches("pkg"));
        assert!(m.matches("pkg-"));
    }

    #[test]
    fn dewey_match_range() {
        let m = Dewey::new("pkg>1.0alpha3nb2<2.0beta4nb7").unwrap();
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
    }

    /*
     * Ensure that comparisons between versions of differing lengths are
     * calculated correctly.
     */
    #[test]
    fn dewey_match_length() {
        let m = Dewey::new("pkg>1.0.0.0alphanb1").unwrap();
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
    }
}
