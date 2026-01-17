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

/*! Parse `make pbulk-index` output into package records. */

use crate::kv::Kv;
use crate::{Depend, PkgName, PkgPath};
use std::fmt;
use std::io::{self, BufRead};
use std::path::PathBuf;
use std::str::FromStr;

/**
 * Parse the output of `make pbulk-index` into individual records.
 *
 * See [pbulk-index.mk] and [pbulk-build(1)].
 *
 * While the majority of these fields will always be set even if left empty,
 * they are wrapped in [`Option`] to simplify tests as well as handle cases in
 * the future should they be removed from the default output.
 *
 * [pbulk-index.mk]: https://github.com/NetBSD/pkgsrc/blob/trunk/mk/pbulk/pbulk-index.mk
 * [pbulk-build(1)]: https://github.com/NetBSD/pkgsrc/blob/trunk/pkgtools/pbulk/files/pbulk/pbuild/pbulk-build.1
 *
 * # Example
 *
 * ```no_run
 * use pkgsrc::{PkgName, ScanIndex};
 * use std::io::BufReader;
 * use std::process::{Command, Stdio};
 *
 * let cmd = Command::new("make")
 *     .current_dir("/usr/pkgsrc/databases/php-mysql")
 *     .arg("pbulk-index")
 *     .stdout(Stdio::piped())
 *     .spawn()
 *     .expect("Unable to execute make");
 * let stdout = cmd.stdout.unwrap();
 * let reader = BufReader::new(stdout);
 * let index: Vec<_> = ScanIndex::from_reader(reader)
 *     .collect::<Result<_, _>>()
 *     .unwrap();
 *
 * // Should return 5 results due to MULTI_VERSION
 * assert_eq!(index.len(), 5);
 * assert_eq!(index[0].pkgname, PkgName::new("php56-mysql-5.6.40nb1"));
 * ```
 */
#[derive(Clone, Debug, PartialEq, Eq, Kv)]
pub struct ScanIndex {
    /// Name of the package including the version number.
    pub pkgname: PkgName,
    /// Path to the package inside pkgsrc.  Should really be PKGPATH.
    pub pkg_location: Option<PkgPath>,
    /// All dependencies of the package as DEPENDS matches.
    pub all_depends: Option<Vec<Depend>>,
    /// A string containing the reason if the package should be skipped.
    pub pkg_skip_reason: Option<String>,
    /// A string containing the reason if the package failed or is broken.
    pub pkg_fail_reason: Option<String>,
    /// A string containing the reason why its binary package may not be
    /// uploaded.
    pub no_bin_on_ftp: Option<String>,
    /// A string containing the reason why its binary package may not be
    /// distributed.
    pub restricted: Option<String>,
    /// Categories to which the package belongs.
    pub categories: Option<String>,
    /// Maintainer of the package.
    pub maintainer: Option<String>,
    /// `DESTDIR` method this package supports (almost always `user-destdir`).
    pub use_destdir: Option<String>,
    /// If this package is used during pkgsrc bootstrap.
    pub bootstrap_pkg: Option<String>,
    /// The phase of the build process during which the user and/or group
    /// needed by this package need to be available.
    pub usergroup_phase: Option<String>,
    /// List of files read during the dependency scanning step.
    pub scan_depends: Option<Vec<PathBuf>>,
    /// Numeric build priority of the package. If not set, a value of 100 is
    /// assumed.
    pub pbulk_weight: Option<String>,
    /// List of variables to be set when building this specific `PKGNAME` from
    /// a common `PKGPATH`.
    pub multi_version: Option<Vec<String>>,
}

impl FromStr for ScanIndex {
    type Err = crate::kv::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for ScanIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "PKGNAME={}", self.pkgname)?;
        if let Some(ref v) = self.pkg_location {
            writeln!(f, "PKG_LOCATION={v}")?;
        }
        write!(f, "ALL_DEPENDS=")?;
        if let Some(ref deps) = self.all_depends {
            for (i, d) in deps.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{d}")?;
            }
        }
        writeln!(f)?;
        writeln!(f, "PKG_SKIP_REASON={}", opt_str(&self.pkg_skip_reason))?;
        writeln!(f, "PKG_FAIL_REASON={}", opt_str(&self.pkg_fail_reason))?;
        writeln!(f, "NO_BIN_ON_FTP={}", opt_str(&self.no_bin_on_ftp))?;
        writeln!(f, "RESTRICTED={}", opt_str(&self.restricted))?;
        writeln!(f, "CATEGORIES={}", opt_str(&self.categories))?;
        writeln!(f, "MAINTAINER={}", opt_str(&self.maintainer))?;
        writeln!(f, "USE_DESTDIR={}", opt_str(&self.use_destdir))?;
        writeln!(f, "BOOTSTRAP_PKG={}", opt_str(&self.bootstrap_pkg))?;
        writeln!(f, "USERGROUP_PHASE={}", opt_str(&self.usergroup_phase))?;
        write!(f, "SCAN_DEPENDS=")?;
        if let Some(ref paths) = self.scan_depends {
            for (i, p) in paths.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", p.display())?;
            }
        }
        writeln!(f)?;
        if let Some(ref v) = self.pbulk_weight {
            writeln!(f, "PBULK_WEIGHT={v}")?;
        }
        if let Some(ref vars) = self.multi_version {
            if !vars.is_empty() {
                write!(f, "MULTI_VERSION=")?;
                for v in vars {
                    write!(f, " {v}")?;
                }
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

fn opt_str(o: &Option<String>) -> &str {
    o.as_deref().unwrap_or("")
}

impl ScanIndex {
    /**
     * Create an iterator that parses [`ScanIndex`] entries from a reader.
     *
     * Records are delimited by lines starting with `PKGNAME=`.
     */
    pub fn from_reader<R: BufRead>(reader: R) -> ScanIndexIter<R> {
        ScanIndexIter {
            lines: reader.lines(),
            buffer: String::new(),
            done: false,
        }
    }
}

/**
 * Iterator that parses [`ScanIndex`] entries from a [`BufRead`] source.
 *
 * Created by [`ScanIndex::from_reader`].
 */
pub struct ScanIndexIter<R> {
    lines: io::Lines<R>,
    buffer: String,
    done: bool,
}

impl<R: BufRead> Iterator for ScanIndexIter<R> {
    type Item = io::Result<ScanIndex>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        loop {
            match self.lines.next() {
                Some(Ok(line)) => {
                    if line.starts_with("PKGNAME=") && !self.buffer.is_empty() {
                        let record = std::mem::take(&mut self.buffer);
                        self.buffer.push_str(&line);
                        self.buffer.push('\n');
                        return Some(parse_record(&record));
                    }
                    self.buffer.push_str(&line);
                    self.buffer.push('\n');
                }
                Some(Err(e)) => return Some(Err(e)),
                None => {
                    self.done = true;
                    if self.buffer.is_empty() {
                        return None;
                    }
                    return Some(parse_record(&std::mem::take(
                        &mut self.buffer,
                    )));
                }
            }
        }
    }
}

fn parse_record(s: &str) -> io::Result<ScanIndex> {
    s.parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::BufReader;
    use std::path::PathBuf;

    /*
     * Real-world test input generated using 'bmake pbulk-index' inside
     * databases/py-mysqlclient using pbulkmulti patches, so that there are
     * a total of 40 packages built from a single PKGPATH.
     */
    #[test]
    fn multi_input() {
        let mut scanfile = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        scanfile.push("tests/data/scanindex/pbulk-index.txt");
        let file = File::open(&scanfile).unwrap();
        let reader = BufReader::new(file);
        let index: Vec<_> = ScanIndex::from_reader(reader)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(index.len(), 40);
        assert_eq!(index[0].all_depends.as_ref().unwrap().len(), 11);
        assert_eq!(index[0].scan_depends.as_ref().unwrap().len(), 155);
        assert_eq!(index[0].multi_version.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn duplicate_pkgname() {
        // We do not check for unique PKGNAME, two entries will be created.
        let input = "PKGNAME=foo\nPKGNAME=foo\n";
        let index: Vec<_> = ScanIndex::from_reader(input.as_bytes())
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn no_input() {
        // No valid input should just result in an empty index.
        let input = "";
        let index: Vec<_> = ScanIndex::from_reader(input.as_bytes())
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn empty_input() {
        // A single PKGNAME, even if invalid, will create an index
        // entry but it should be empty.
        let input = "PKGNAME=";
        let index: Vec<_> = ScanIndex::from_reader(input.as_bytes())
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].pkgname.pkgname(), "");
    }

    #[test]
    fn input_error() {
        // If we see any valid field but no PKGNAME (required) then we should
        // generate an error.
        let input = "ALL_DEPENDS=";
        let result: Result<Vec<_>, _> =
            ScanIndex::from_reader(input.as_bytes()).collect();
        assert!(result.is_err());

        // If ALL_DEPENDS is specified then it should be correct, i.e. here
        // we're testing that Depend::new errors are propagated.
        let input = "PKGNAME=\nALL_DEPENDS=hello\n";
        let result: Result<Vec<_>, _> =
            ScanIndex::from_reader(input.as_bytes()).collect();
        assert!(result.is_err());
    }

    #[test]
    fn from_str() {
        use std::str::FromStr;

        let input = "PKGNAME=test-1.0\nMAINTAINER=test@example.com\n";
        let index = ScanIndex::from_str(input).unwrap();
        assert_eq!(index.pkgname.pkgname(), "test-1.0");
        assert_eq!(index.maintainer.as_deref(), Some("test@example.com"));
    }

    #[test]
    fn error_unknown_variable() {
        use crate::kv;
        use std::str::FromStr;

        let input = "PKGNAME=test-1.0\nUNKNOWN=value\n";
        let err = ScanIndex::from_str(input).unwrap_err();
        match err {
            kv::Error::UnknownVariable { variable, span } => {
                assert_eq!(variable, "UNKNOWN");
                assert_eq!(span.offset, 17);
                assert_eq!(span.len, 7);
                assert_eq!(
                    &input[span.offset..span.offset + span.len],
                    "UNKNOWN"
                );
            }
            _ => panic!("expected UnknownVariable error, got {err:?}"),
        }
    }

    #[test]
    fn error_invalid_depend() {
        use crate::kv;
        use std::str::FromStr;

        // "invalid" is not a valid Depend (missing ":" separator)
        let input = "PKGNAME=test-1.0\nALL_DEPENDS=invalid\n";
        let err = ScanIndex::from_str(input).unwrap_err();
        match err {
            kv::Error::Parse { message, span } => {
                assert!(message.contains("Invalid DEPENDS"));
                assert_eq!(span.offset, 29);
                assert_eq!(span.len, 7);
                assert_eq!(
                    &input[span.offset..span.offset + span.len],
                    "invalid"
                );
            }
            _ => panic!("expected Parse error, got {err:?}"),
        }
    }

    #[test]
    fn error_invalid_pkgpath() {
        use crate::kv;
        use std::str::FromStr;

        // "bad" is not a valid PkgPath (missing category/package structure)
        let input = "PKGNAME=test-1.0\nPKG_LOCATION=bad\n";
        let err = ScanIndex::from_str(input).unwrap_err();
        match err {
            kv::Error::Parse { message, span } => {
                assert!(message.contains("Invalid path"));
                assert_eq!(span.offset, 30);
                assert_eq!(span.len, 3);
                assert_eq!(&input[span.offset..span.offset + span.len], "bad");
            }
            _ => panic!("expected Parse error, got {err:?}"),
        }
    }

    #[test]
    fn error_missing_pkgname() {
        use crate::kv;
        use std::str::FromStr;

        let input = "MAINTAINER=test@example.com\n";
        let err = ScanIndex::from_str(input).unwrap_err();
        match err {
            kv::Error::Incomplete(field) => {
                assert_eq!(field, "PKGNAME");
            }
            _ => panic!("expected Incomplete error, got {err:?}"),
        }
    }

    #[test]
    fn error_bad_line_format() {
        use crate::kv;
        use std::str::FromStr;

        let input = "PKGNAME=test-1.0\nbadline\n";
        let err = ScanIndex::from_str(input).unwrap_err();
        match err {
            kv::Error::ParseLine(span) => {
                assert_eq!(span.offset, 17);
                assert_eq!(span.len, 7);
                assert_eq!(
                    &input[span.offset..span.offset + span.len],
                    "badline"
                );
            }
            _ => panic!("expected ParseLine error, got {err:?}"),
        }
    }

    #[test]
    fn error_span_accessor() {
        use std::str::FromStr;

        let input = "PKGNAME=test-1.0\nUNKNOWN=value\n";
        let err = ScanIndex::from_str(input).unwrap_err();
        let span = err.span().expect("should have span");
        assert_eq!(&input[span.offset..span.offset + span.len], "UNKNOWN");
    }

    #[test]
    fn display_roundtrip() {
        let mut scanfile = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        scanfile.push("tests/data/scanindex/pbulk-index.txt");
        let file = File::open(&scanfile).unwrap();
        let reader = BufReader::new(file);
        let original: Vec<_> = ScanIndex::from_reader(reader)
            .collect::<Result<_, _>>()
            .unwrap();

        let output: String = original.iter().map(|s| s.to_string()).collect();
        let reparsed: Vec<_> = ScanIndex::from_reader(output.as_bytes())
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(original, reparsed);
    }
}
