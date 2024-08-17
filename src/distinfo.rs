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

/*!
 * pkgsrc `distinfo` file parsing and processing.
 *
 * Most packages have a `distinfo` file that describes all of the source
 * distribution files (known in pkgsrc nomenclature as `distfiles`) used by the
 * package, as well as any pkgsrc patches that are applied to the unpacked
 * source code.
 *
 * Each `distinfo` file primarily contains checksums for each file, to ensure
 * integrity of both downloaded distfiles as well as the applied patches.  For
 * additional integrity, distfiles usually contain two hashes from different
 * digest algorithms.  They also usually include the size of the file.  Patch
 * files usually just have a single hash.
 *
 * The first line is usually a `$NetBSD$` RCS Id, and the second line is
 * usually blank.  Thus an example `distinfo` file and how to parse it looks
 * something like this:
 *
 * ```
 * use pkgsrc::distinfo::Distinfo;
 * use std::ffi::OsString;
 *
 * let input = r#"
 *     $NetBSD: distinfo,v 1.80 2024/05/27 23:27:10 riastradh Exp $
 *
 *     BLAKE2s (pkgin-23.8.1.tar.gz) = eb0f008ba9801a3c0a35de3e2b2503edd554c3cb17235b347bb8274a18794eb7
 *     SHA512 (pkgin-23.8.1.tar.gz) = 2561d9e4b28a9a77c3c798612ec489dd67dd9a93c61344937095b0683fa89d8432a9ab8e600d0e2995d954888ac2e75a407bab08aa1e8198e375c99d2999f233
 *     Size (pkgin-23.8.1.tar.gz) = 267029 bytes
 *     SHA1 (patch-configure.ac) = 53f56351fb602d9fdce2c1ed266d65919a369086
 *     "#;
 * let distinfo = Distinfo::from_bytes(&input.as_bytes());
 * assert_eq!(distinfo.rcsid(), Some(&OsString::from("$NetBSD: distinfo,v 1.80 2024/05/27 23:27:10 riastradh Exp $")));
 * ```
 *
 * As `distinfo` files can contain usernames and filenames that are not UTF-8
 * clean (for example ISO-8859), `from_bytes()` is the method used to parse
 * input, and the rcsid and filename portions are parsed as [`OsString`].
 * The remaining sections must be UTF-8 clean and are regular [`String`]s.
 *
 * [`OsString`]: https://doc.rust-lang.org/std/ffi/struct.OsString.html
 * [`String`]: https://doc.rust-lang.org/std/string/struct.String.html
 */

use crate::digest::Digest;
use indexmap::IndexMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::str::FromStr;

/**
 * [`ChecksumEntry`] contains the [`Digest`] type and the [`String`] hash the
 * digest algorithm calculated for an associated [`FileEntry`].
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChecksumEntry {
    /**
     * The [`Digest`] type used for this entry.
     */
    pub digest: Digest,
    /**
     * A [`String`] result of the digest hash applied to the associated file.
     */
    pub hash: String,
}

/**
 * [`FileEntry`] contains the information stored about each unique file listed
 * in the distinfo file.
 */
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileEntry {
    /**
     * The filename stored as a [`PathBuf`].  This should not contain any
     * directory portion.
     */
    pub filename: PathBuf,
    /**
     * File size.  This field is not currently used for patch files, as they
     * are distributed alongside the distinfo file and are not downloaded
     * separately, thus a single hash check is sufficient.
     */
    pub size: u64,
    /**
     * List of checksums, one [`ChecksumEntry`] entry per Digest type.  These
     * are in order of appearance in the `distinfo` file.
     */
    pub checksums: Vec<ChecksumEntry>,
}

/**
 * Parse a single `distinfo` line into a valid line type.  This is an
 * intermediate format, as it doesn't serve any useful function to the user,
 * but is helpful for internally constructing an eventual [`Distinfo`].
 */
#[derive(Debug, Eq, PartialEq)]
enum LineEntry {
    RcsId(OsString),
    Size(PathBuf, u64),
    Checksum(Digest, PathBuf, String),
    None,
}

/**
 * [`Distinfo`] contains the contents of a `distinfo` file.
 *
 * The primary interface for populating a [`Distinfo`] is using the
 * [`from_bytes`] function.
 *
 * There is no error handling.  Any input that is unrecognised or not in the
 * correct format is simply ignored.
 *
 * [`from_bytes`]: Distinfo::from_bytes
 */
#[derive(Debug)]
pub struct Distinfo {
    rcsid: Option<OsString>,
    files: Vec<FileEntry>,
    patches: Vec<FileEntry>,
}

impl Distinfo {
    /**
     * Return an [`Option`] containing either a valid `$NetBSD: ...` RCS Id
     * line, or None if one was not found.
     */
    pub fn rcsid(&self) -> Option<&OsString> {
        match &self.rcsid {
            Some(s) => Some(s),
            None => None,
        }
    }
    /**
     * Return a [`Vec`] of references to distfile entries, if any.
     */
    pub fn files(&self) -> Vec<&FileEntry> {
        self.files.iter().collect()
    }
    /**
     * Return a [`Vec`] of references to patchfile entries, if any.
     */
    pub fn patches(&self) -> Vec<&FileEntry> {
        self.patches.iter().collect()
    }
    /**
     * Read a [`Vec`] of [`u8`] bytes and parse for [`Distinfo`] entries.  If
     * nothing is found then an empty [`Distinfo`] is returned.
     */
    pub fn from_bytes(bytes: &[u8]) -> Distinfo {
        let mut distinfo = Distinfo {
            rcsid: None,
            files: vec![],
            patches: vec![],
        };
        let mut files: IndexMap<PathBuf, FileEntry> = IndexMap::new();
        let mut patches: IndexMap<PathBuf, FileEntry> = IndexMap::new();
        for line in bytes.split(|c| *c == b'\n') {
            match LineEntry::from_bytes(line) {
                /*
                 * We shouldn't encounter multiple RcsId entries, but if we do
                 * then last match wins.
                 */
                LineEntry::RcsId(s) => distinfo.rcsid = Some(s),
                LineEntry::Size(p, v) => {
                    match is_patchfile(&p) {
                        true => update_size(&mut patches, &p, v),
                        false => update_size(&mut files, &p, v),
                    };
                }
                LineEntry::Checksum(d, p, s) => {
                    match is_patchfile(&p) {
                        true => update_checksum(&mut patches, &p, d, s),
                        false => update_checksum(&mut files, &p, d, s),
                    };
                }
                LineEntry::None => {}
            }
        }
        for (_, v) in files {
            distinfo.files.push(v);
        }
        for (_, v) in patches {
            distinfo.patches.push(v);
        }

        distinfo
    }
    /**
     * Convert [`Distinfo`] into a byte representation suitable for writing to
     * a `distinfo` file.  The contents will be ordered as expected.
     */
    pub fn as_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        if let Some(s) = self.rcsid() {
            bytes.extend_from_slice(s.as_bytes());
        } else {
            bytes.extend_from_slice("$NetBSD$".as_bytes());
        }
        bytes.extend_from_slice("\n\n".as_bytes());

        for q in &self.files {
            for c in &q.checksums {
                bytes.extend_from_slice(
                    format!(
                        "{} ({}) = {}\n",
                        c.digest,
                        q.filename.display(),
                        c.hash
                    )
                    .as_bytes(),
                );
            }
            bytes.extend_from_slice(
                format!("Size ({}) = {} bytes\n", q.filename.display(), q.size)
                    .as_bytes(),
            );
        }

        for q in &self.patches {
            for c in &q.checksums {
                bytes.extend_from_slice(
                    format!(
                        "{} ({}) = {}\n",
                        c.digest,
                        q.filename.display(),
                        c.hash
                    )
                    .as_bytes(),
                );
            }
        }

        bytes
    }
}

fn update_checksum(
    hash: &mut IndexMap<PathBuf, FileEntry>,
    p: &Path,
    d: Digest,
    c: String,
) {
    match hash.get_mut(p) {
        Some(h) => h.checksums.push(ChecksumEntry { digest: d, hash: c }),
        None => {
            let v: Vec<ChecksumEntry> =
                vec![ChecksumEntry { digest: d, hash: c }];
            hash.insert(
                p.to_path_buf(),
                FileEntry {
                    filename: p.to_path_buf(),
                    checksums: v,
                    ..Default::default()
                },
            );
        }
    };
}

fn update_size(hash: &mut IndexMap<PathBuf, FileEntry>, p: &Path, v: u64) {
    match hash.get_mut(p) {
        Some(h) => h.size = v,
        None => {
            hash.insert(
                p.to_path_buf(),
                FileEntry {
                    filename: p.to_path_buf(),
                    size: v,
                    ..Default::default()
                },
            );
        }
    };
}

impl LineEntry {
    fn from_bytes(bytes: &[u8]) -> LineEntry {
        /*
         * Despite expecting a single line, handle embedded newlines anyway
         * to simplify things.  First valid (i.e. not None) match wins.
         */
        for line in bytes.split(|c| *c == b'\n') {
            let mut start = 0;
            /*
             * Skip leading whitespace.  Technically this isn't supported, but
             * be liberal in what you accept...
             */
            for ch in line.iter() {
                if !(*ch as char).is_whitespace() {
                    break;
                }
                start += 1;
            }

            let line = &line[start..];

            /* Skip comments and empty lines */
            if line.starts_with(b"#") || line.is_empty() {
                continue;
            }

            /*
             * Match NetBSD RCS Id.  Only match an expanded "$NetBSD: ..."
             * string, there's no point matching an unexpanded "$NetBSD$".
             */
            if line.starts_with(b"$NetBSD: ") {
                return LineEntry::RcsId(OsString::from_vec((*line).to_vec()));
            }

            /*
             * The remaining types are matched the same, even though they in
             * format, because the important parts are in the same place:
             *
             *   DIGEST (FILENAME) = HASH
             *   Size (FILENAME) = BYTES bytes
             *
             * We just ignore the trailing "bytes" of "Size" lines.
             *
             * If we see anything we don't like then LineEntry::None is
             * immediately returned.
             */
            let mut field = 0;
            let mut action = String::new();
            let mut path = PathBuf::new();
            let mut value = String::new();
            for s in line.split(|c| (*c as char).is_whitespace()) {
                /* Skip extra whitespace */
                if s.is_empty() {
                    continue;
                }
                if field == 0 {
                    action = match String::from_utf8(s.to_vec()) {
                        Ok(s) => s,
                        Err(_) => return LineEntry::None,
                    };
                }
                /* Record path from "(filename)" */
                if field == 1 {
                    if s[0] == b'(' && s[s.len() - 1] == b')' {
                        path.push(OsStr::from_bytes(&s[1..s.len() - 1]));
                    } else {
                        return LineEntry::None;
                    }
                }
                /* Record size or hash */
                if field == 3 {
                    value = match String::from_utf8(s.to_vec()) {
                        Ok(s) => s,
                        Err(_) => return LineEntry::None,
                    }
                }
                field += 1;
            }
            /*
             * Valid actions are "Size", or a valid Digest type.  Anything
             * else is unmatched.
             */
            if action == "Size" {
                match u64::from_str(&value) {
                    Ok(n) => return LineEntry::Size(path, n),
                    Err(_) => return LineEntry::None,
                };
            } else {
                match Digest::from_str(&action) {
                    Ok(d) => return LineEntry::Checksum(d, path, value),
                    Err(_) => return LineEntry::None,
                }
            }
        }
        LineEntry::None
    }
}

/*
 * Verify that a supplied path is a valid patch file.  Returns a String
 * containing the patch filename if so, otherwise None.
 */
fn is_patchfile(p: &Path) -> bool {
    let s = p.to_string_lossy();
    /*
     * Skip local patches or temporary patch files created by e.g. mkpatches.
     */
    if s.starts_with("patch-local-")
        || s.ends_with(".orig")
        || s.ends_with(".rej")
        || s.ends_with("~")
    {
        return false;
    }
    /*
     * Match valid patch filenames.
     */
    if s.starts_with("patch-")
        || (s.starts_with("emul-") && s.contains("-patch-"))
    {
        /*
         * This is really janky, but we need to skip distfiles for devel/patch
         * itself, e.g. "patch-2.7.6.tar.xz".
         */
        if !s.contains(".tar.") {
            return true;
        }
    }

    /*
     * Anything else is invalid.
     */
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /*
     * Test RcsId parsing, with and without additional whitespace and
     * comments.
     */
    #[test]
    fn test_line_rcsid() {
        let rcsid = "$NetBSD: distinfo,v 1.1 1970/01/01 01:01:01 ken Exp $";
        let expected = LineEntry::RcsId(rcsid.into());

        let entry = LineEntry::from_bytes(rcsid.as_bytes());
        assert_eq!(entry, expected);

        let entry = LineEntry::from_bytes(format!("   {rcsid}").as_bytes());
        assert_eq!(entry, expected);

        let entry = LineEntry::from_bytes(format!("\n\n {rcsid}").as_bytes());
        assert_eq!(entry, expected);

        let entry = LineEntry::from_bytes(format!(" {rcsid}\n\n").as_bytes());
        assert_eq!(entry, expected);

        /* Commented entry should return None */
        let entry = LineEntry::from_bytes(format!("#{rcsid}").as_bytes());
        assert_eq!(entry, LineEntry::None);
    }

    #[test]
    fn test_line_size() {
        /*
         * Regular entry
         */
        let i = "Size    (foo-1.2.3.tar.gz)    =    321     bytes";
        let o = LineEntry::from_bytes(i.as_bytes());
        assert_eq!(o, LineEntry::Size(PathBuf::from("foo-1.2.3.tar.gz"), 321));

        /*
         * Entry with extra whitespace is accepted, but in reality is likely
         * to be rejected by other tools.
         */
        let i = "Size    (foo-1.2.3.tar.gz)    =    321     bytes";
        let o = LineEntry::from_bytes(i.as_bytes());
        assert_eq!(o, LineEntry::Size(PathBuf::from("foo-1.2.3.tar.gz"), 321));

        /*
         * Invalid as it's missing "bytes", but accepted anyway.
         */
        let i = "Size (foo-1.2.3.tar.gz) = 123";
        let o = LineEntry::from_bytes(i.as_bytes());
        assert_eq!(o, LineEntry::Size(PathBuf::from("foo-1.2.3.tar.gz"), 123));

        /*
         * Check for u64 overflow
         */
        let i = "Size (a.tar.gz) = 18446744073709551615";
        let o = LineEntry::from_bytes(i.as_bytes());
        assert_eq!(
            o,
            LineEntry::Size(PathBuf::from("a.tar.gz"), 18446744073709551615)
        );
        let i = "Size (a.tar.gz) = 18446744073709551616";
        let o = LineEntry::from_bytes(i.as_bytes());
        assert_eq!(o, LineEntry::None);
    }

    #[test]
    fn test_line_digest() {
        let i = "BLAKE2s (pkgin-23.8.1.tar.gz) = ojnk";
        let o = LineEntry::from_bytes(i.as_bytes());
        assert_eq!(
            o,
            LineEntry::Checksum(
                Digest::BLAKE2s,
                PathBuf::from("pkgin-23.8.1.tar.gz"),
                "ojnk".to_string()
            )
        );
    }

    #[test]
    fn test_line_none() {
        let o = LineEntry::from_bytes(format!("").as_bytes());
        assert_eq!(o, LineEntry::None);
        let o = LineEntry::from_bytes(format!("\n  \n\n").as_bytes());
        assert_eq!(o, LineEntry::None);
        let o = LineEntry::from_bytes(format!("#  \n\n").as_bytes());
        assert_eq!(o, LineEntry::None);
    }

    #[test]
    fn test_distinfo() {
        let i = r#"
            $NetBSD: distinfo,v 1.80 2024/05/27 23:27:10 riastradh Exp $

            BLAKE2s (pkgin-23.8.1.tar.gz) = eb0f008ba9801a3c0a35de3e2b2503edd554c3cb17235b347bb8274a18794eb7
            SHA512 (pkgin-23.8.1.tar.gz) = 2561d9e4b28a9a77c3c798612ec489dd67dd9a93c61344937095b0683fa89d8432a9ab8e600d0e2995d954888ac2e75a407bab08aa1e8198e375c99d2999f233
            Size (pkgin-23.8.1.tar.gz) = 267029 bytes
            SHA1 (patch-configure.ac) = 53f56351fb602d9fdce2c1ed266d65919a369086
        "#;
        let di = Distinfo::from_bytes(i.as_bytes());
        assert_eq!(
            di.rcsid(),
            Some(&OsString::from(
                "$NetBSD: distinfo,v 1.80 2024/05/27 23:27:10 riastradh Exp $"
            ))
        );
    }
}
