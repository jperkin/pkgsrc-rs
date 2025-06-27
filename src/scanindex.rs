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

use crate::{Depend, PkgName, PkgPath};
use std::path::PathBuf;

#[cfg(feature = "serde")]
use {
    serde::de::value::StrDeserializer,
    serde::de::{self, Deserializer, Visitor},
    serde::Deserialize,
    std::collections::HashMap,
    std::fmt,
    std::io::{self, BufRead},
};

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
 * use std::fs::File;
 * use std::io::{self, BufRead, BufReader};
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
 * let index = ScanIndex::from_reader(reader).unwrap();
 *
 * // Should return 5 results due to MULTI_VERSION
 * assert_eq!(index.len(), 5);
 * assert_eq!(index[0].pkgname, PkgName::new("php56-mysql-5.6.40nb1"));
 * ```
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ScanIndex {
    /// Name of the package including the version number.
    pub pkgname: PkgName,
    /// Path to the package inside pkgsrc.  Should really be PKGPATH.
    pub pkg_location: Option<PkgPath>,
    /// All dependencies of the package in one line as DEPENDS matches.
    pub all_depends: Vec<Depend>,
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
    pub scan_depends: Vec<PathBuf>,
    /// Numeric build priority of the package.  If not set, a value of 100 is
    /// assumed.
    pub pbulk_weight: Option<String>,
    /// List of variables to be set when building this specific `PKGNAME` from
    /// a common `PKGPATH`.
    pub multi_version: Vec<String>,
    /// Calculated dependencies.
    pub depends: Vec<PkgName>,
}

/*
 * Simple KEY=VALUE parser.
 */
#[cfg(feature = "serde")]
struct KeyValue;

#[cfg(feature = "serde")]
impl Visitor<'_> for KeyValue {
    type Value = HashMap<String, String>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("A stream of the format KEY=VALUE")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let mut map = HashMap::new();
        for line in value.lines() {
            if let Some((key, value)) = line.split_once('=') {
                map.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
        Ok(map)
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for ScanIndex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map: HashMap<String, String> =
            deserializer.deserialize_str(KeyValue)?;

        /* A mandatory single-type value */
        macro_rules! var_reqd {
            ($type:expr, $key:expr) => {
                $type(map.get($key).ok_or(de::Error::missing_field($key))?)
            };
        }

        /* An optional single-type value */
        macro_rules! var_opt {
            ($type:expr, $key:expr) => {
                map.get($key).map($type)
            };
        }

        /* An optional single-type value where the type returns Result */
        macro_rules! var_opt_result {
            ($type:expr, $key:expr) => {
                map.get($key)
                    .map(|v| $type(v.as_str()))
                    .transpose()
                    .map_err(de::Error::custom)?
            };
        }

        /* A vec where the type always succeeds */
        macro_rules! var_vec {
            ($type:expr, $key:expr) => {
                map.get($key).map_or(vec![], |v| {
                    v.split_whitespace().map($type).collect()
                })
            };
        }

        /*
         * A vec where the type returns a Result.  It's fine if there is no
         * entry, but if there is then it must be parsed correctly.
         */
        macro_rules! var_vec_result {
            ($type:expr, $key:expr) => {
                map.get($key).map_or_else(
                    || Ok(vec![]),
                    |v| {
                        v.split_whitespace()
                            .map($type)
                            .map(|result| result.map_err(de::Error::custom))
                            .collect()
                    },
                )?
            };
        }

        let all_depends: Vec<Depend> =
            var_vec_result!(Depend::new, "ALL_DEPENDS");
        let pkgname = var_reqd!(PkgName::new, "PKGNAME");
        /* No idea why this isn't PKGPATH */
        let pkg_location = var_opt_result!(PkgPath::new, "PKG_LOCATION");
        let pkg_skip_reason = var_opt!(String::from, "PKG_SKIP_REASON");
        let pkg_fail_reason = var_opt!(String::from, "PKG_FAIL_REASON");
        let no_bin_on_ftp = var_opt!(String::from, "NO_BIN_ON_FTP");
        let restricted = var_opt!(String::from, "RESTRICTED");
        let categories = var_opt!(String::from, "CATEGORIES");
        let maintainer = var_opt!(String::from, "MAINTAINER");
        let use_destdir = var_opt!(String::from, "USE_DESTDIR");
        let bootstrap_pkg = var_opt!(String::from, "BOOTSTRAP_PKG");
        let usergroup_phase = var_opt!(String::from, "USERGROUP_PHASE");
        let scan_depends = var_vec!(PathBuf::from, "SCAN_DEPENDS");
        let pbulk_weight = var_opt!(String::from, "PBULK_WEIGHT");
        let multi_version = var_vec!(String::from, "MULTI_VERSION");

        /* DEPENDS is filled out by whatever parses this struct */
        let depends = vec![];

        Ok(ScanIndex {
            pkgname,
            pkg_location,
            all_depends,
            pkg_skip_reason,
            pkg_fail_reason,
            no_bin_on_ftp,
            restricted,
            categories,
            maintainer,
            use_destdir,
            bootstrap_pkg,
            usergroup_phase,
            scan_depends,
            pbulk_weight,
            multi_version,
            depends,
        })
    }
}

impl ScanIndex {
    /**
     * Convert a single pbulk-index-item to a [`ScanIndex`].
     */
    #[cfg(feature = "serde")]
    fn str_to_index(input: &str) -> io::Result<ScanIndex> {
        let index = StrDeserializer::<serde::de::value::Error>::new(input);
        let index = ScanIndex::deserialize(index).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse: {e}"),
            )
        })?;
        Ok(index)
    }

    /**
     * Return a [`Vec`] of new [`ScanIndex`] items from a reader.
     */
    #[cfg(feature = "serde")]
    pub fn from_reader<R: BufRead>(reader: R) -> io::Result<Vec<ScanIndex>> {
        let mut indexes = vec![];
        let mut buffer = String::new();

        for line in reader.lines() {
            let line = line?;
            /*
             * The output of pbulk-index should not include empty lines nor
             * any leading/trailing whitespace, but we do this to be kind and
             * also to simplify tests.
             */
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line.starts_with("PKGNAME=") && !buffer.is_empty() {
                indexes.push(Self::str_to_index(&buffer)?);
                buffer.clear();
            }
            buffer.push_str(line);
            buffer.push('\n');
        }
        if !buffer.is_empty() {
            indexes.push(Self::str_to_index(&buffer)?);
        }

        Ok(indexes)
    }
}
#[cfg(test)]
#[cfg(feature = "serde")]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::BufReader;

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
        let index = ScanIndex::from_reader(reader).unwrap();
        assert_eq!(index.len(), 40);
        assert_eq!(index[0].all_depends.len(), 11);
        assert_eq!(index[0].scan_depends.len(), 155);
        assert_eq!(index[0].multi_version.len(), 2);
    }

    #[test]
    fn duplicate_pkgname() {
        // We do not check for unique PKGNAME, two entries will be created.
        let input = "PKGNAME=foo\nPKGNAME=foo\n";
        let index = ScanIndex::from_reader(input.as_bytes()).unwrap();
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn no_input() {
        // No valid input should just result in an empty index.
        let input = "";
        let index = ScanIndex::from_reader(input.as_bytes()).unwrap();
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn empty_input() {
        // A single PKGNAME, even if invalid, will create an index
        // entry but it should be empty.
        let input = "PKGNAME=";
        let index = ScanIndex::from_reader(input.as_bytes()).unwrap();
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn input_error() {
        // If we see any valid field but no PKGNAME (required) then we should
        // generate an error.
        let input = "ALL_DEPENDS=";
        let index = ScanIndex::from_reader(input.as_bytes());
        assert!(index.is_err());

        // If ALL_DEPENDS is specified then it should be correct, i.e. here
        // we're testing that Depend::new errors are propagated.
        let input = "PKGNAME=\nALL_DEPENDS=hello\n";
        let index = ScanIndex::from_reader(input.as_bytes());
        assert!(index.is_err());
    }
}
