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
 * input, and the rcsid and filename portions are parsed as [`OsString`].  The
 * remaining sections must be UTF-8 clean and are regular [`String`]s.
 */

use crate::digest::{Digest, DigestError};
use indexmap::IndexMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;

/**
 * [`Checksum`] contains the [`Digest`] type and the [`String`] hash the digest
 * algorithm calculated for an associated [`Entry`].
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Checksum {
    /**
     * The [`Digest`] type used for this entry.
     */
    pub digest: Digest,
    /**
     * A [`String`] result of the digest hash applied to the associated file.
     */
    pub hash: String,
}

impl Checksum {
    /**
     * Create a new empty [`Checksum`] entry using the specified [`Digest`].
     */
    pub fn new(digest: Digest, hash: String) -> Checksum {
        Checksum { digest, hash }
    }
}

/**
 * Type of this [`Entry`], either [`Distfile`] (the default) or [`Patchfile`].
 *
 * [`Distfile`]: EntryType::Distfile
 * [`Patchfile`]: EntryType::Patchfile
 */
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub enum EntryType {
    /**
     * A source distribution file.
     */
    #[default]
    Distfile,
    /**
     * A pkgsrc patch file.
     */
    Patchfile,
}

impl<P: AsRef<Path>> From<P> for EntryType {
    /*
     * Determine whether a supplied path is a distfile or patchfile.  Unless
     * absolutely sure this is a valid patchfile, default to distfile.
     */
    fn from(path: P) -> Self {
        let Some(p) = path.as_ref().file_name() else {
            return EntryType::Distfile;
        };
        let s = p.to_string_lossy();
        /*
         * Skip local patches or temporary patch files created by e.g. mkpatches.
         */
        if s.starts_with("patch-local-")
            || s.ends_with(".orig")
            || s.ends_with(".rej")
            || s.ends_with("~")
        {
            return EntryType::Distfile;
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
                return EntryType::Patchfile;
            }
        }

        EntryType::Distfile
    }
}

/**
 * [`Entry`] contains the information stored about each unique file listed in
 * the distinfo file.
 */
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Entry {
    /**
     * Path relative to a certain directory (usually `DISTDIR`) where this
     * entry is stored.  This may contain a directory portion, for example if
     * the package uses DIST_SUBDIR.  This is the string that will be stored
     * in the resulting `distinfo` file.
     */
    pub filename: PathBuf,
    /**
     * Full path to filename.  This is not used in the `distinfo` file but is
     * stored here for processing purposes.
     */
    pub filepath: PathBuf,
    /**
     * File size.  This field is not currently used for patch files, as they
     * are distributed alongside the distinfo file and are not downloaded
     * separately, thus a single hash check is sufficient.
     */
    pub size: Option<u64>,
    /**
     * List of checksums, one [`Checksum`] entry per Digest type.  These are in
     * order of appearance in the `distinfo` file.
     */
    pub checksums: Vec<Checksum>,
    /**
     * Whether this entry is a distfile or a patchfile.
     */
    pub filetype: EntryType,
}

impl Entry {
    /**
     * Create a new [`Entry`].
     */
    pub fn new<P1, P2>(
        filename: P1,
        filepath: P2,
        checksums: Vec<Checksum>,
        size: Option<u64>,
    ) -> Entry
    where
        P1: AsRef<Path>,
        P2: AsRef<Path>,
    {
        let filetype = EntryType::from(filename.as_ref());
        Entry {
            filename: filename.as_ref().to_path_buf(),
            filepath: filepath.as_ref().to_path_buf(),
            checksums,
            size,
            filetype,
        }
    }
    /**
     * Pass the full path to a file to check as a [`PathBuf`] and verify that
     * it matches the size stored in the [`Distinfo`].
     *
     * Returns the size if [`Ok`], otherwise return a [`DistinfoError`].
     */
    pub fn verify_size<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<u64, DistinfoError> {
        if let Some(size) = self.size {
            let f = File::open(path)?;
            let fsize = f.metadata()?.len();
            if fsize != size {
                return Err(DistinfoError::Size(
                    self.filename.clone(),
                    size,
                    fsize,
                ));
            } else {
                return Ok(size);
            }
        }
        Err(DistinfoError::MissingSize(path.as_ref().to_path_buf()))
    }

    /**
     * Internal function to check a specific hash.
     */
    fn verify_checksum_internal<P: AsRef<Path>>(
        &self,
        path: P,
        digest: Digest,
    ) -> Result<Digest, DistinfoError> {
        for c in &self.checksums {
            if digest != c.digest {
                continue;
            }
            let mut f = File::open(path)?;
            let hash = match self.filetype {
                EntryType::Distfile => c.digest.hash_file(&mut f)?,
                EntryType::Patchfile => c.digest.hash_patch(&mut f)?,
            };
            if hash != c.hash {
                return Err(DistinfoError::Checksum(
                    self.filename.clone(),
                    c.digest,
                    c.hash.clone(),
                    hash,
                ));
            } else {
                return Ok(digest);
            }
        }
        Err(DistinfoError::MissingChecksum(
            path.as_ref().to_path_buf(),
            digest,
        ))
    }

    /**
     * Pass the full path to a file to check as a [`PathBuf`] and verify that
     * it matches a specific [`Digest`] checksum stored in the [`Distinfo`].
     *
     * Return the [`Digest`] if [`Ok`], otherwise return a [`DistinfoError`].
     *
     * To verify all stored checksums use use [`verify_checksums`].
     *
     * [`verify_checksums`]: Distinfo::verify_checksums
     */
    pub fn verify_checksum<P: AsRef<Path>>(
        &self,
        path: P,
        digest: Digest,
    ) -> Result<Digest, DistinfoError> {
        self.verify_checksum_internal(path, digest)
    }

    /**
     * Pass the full path to a file to check as a [`PathBuf`] and verify that
     * it matches all of the checksums stored in the [`Distinfo`].  Returns a
     * [`Vec`] of [`Result`]s containing the [`Digest`] if [`Ok`], otherwise
     * return a [`DistinfoError`].
     */
    pub fn verify_checksums<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Vec<Result<Digest, DistinfoError>> {
        let mut results = vec![];
        for c in &self.checksums {
            results
                .push(self.verify_checksum_internal(path.as_ref(), c.digest));
        }
        results
    }

    /**
     * Convert [`Entry`] into a byte representation suitable for writing to
     * a `distinfo` file.  The contents will be ordered as expected.
     */
    pub fn as_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for c in &self.checksums {
            bytes.extend_from_slice(
                format!(
                    "{} ({}) = {}\n",
                    c.digest,
                    self.filename.display(),
                    c.hash
                )
                .as_bytes(),
            );
        }
        if let Some(size) = self.size {
            bytes.extend_from_slice(
                format!(
                    "Size ({}) = {} bytes\n",
                    self.filename.display(),
                    size
                )
                .as_bytes(),
            );
        }
        bytes
    }
}

/**
 * Parse a single `distinfo` line into a valid line type.  This is an
 * intermediate format, as it doesn't serve any useful function to the user,
 * but is helpful for internally constructing an eventual [`Distinfo`].
 */
#[derive(Debug, Eq, PartialEq)]
enum Line {
    RcsId(OsString),
    Size(PathBuf, u64),
    Checksum(Digest, PathBuf, String),
    None,
}

/**
 * [`Distinfo`] contains the contents of a `distinfo` file.
 *
 * The primary interface for populating a [`Distinfo`] from an existing
 * `distinfo` file is using the [`from_bytes`] function.  There is no error
 * handling.  Any input that is unrecognised or not in the correct format is
 * simply ignored.
 *
 * To create a new `distinfo` file, use [`new`] and set the fields manually.
 *
 * [`from_bytes`]: Distinfo::from_bytes
 * [`new`]: Distinfo::new
 */
#[derive(Clone, Debug, Default)]
pub struct Distinfo {
    /**
     * An optional `$NetBSD: ... $` RCS Id.  As the username portion may
     * contain e.g. ISO-8859 characters it is stored as an [`OsString`].
     */
    rcsid: Option<OsString>,
    /**
     * An [`IndexMap`] of [`Entry`] entries for all source distfiles used by
     * the package, keyed by [`PathBuf`].  These should store both checksums
     * and size information.
     */
    distfiles: IndexMap<PathBuf, Entry>,
    /**
     * An [`IndexMap`] of [`Entry`] entries for any pkgsrc patches applied to
     * the extracted source code, keyed by [`PathBuf`].  These currently do
     * not contain size information.
     */
    patchfiles: IndexMap<PathBuf, Entry>,
}

/**
 * Possible errors returned by various [`Distinfo`] operations.
 */
#[derive(Debug, Error)]
pub enum DistinfoError {
    /// Transparent [`io::Error`] error.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Transparent [`Digest`] error.
    #[error(transparent)]
    Digest(#[from] DigestError),
    /// File was not found as a valid entry in the current [`Distinfo`] struct.
    #[error("File not found")]
    NotFound,
    /// Checksum mismatch, expected vs actual.
    #[error("Checksum {1} mismatch for {0}: expected {2}, actual {3}")]
    Checksum(PathBuf, Digest, String, String),
    /// No checksum found for the requested Digest
    #[error("Missing {1} checksum entry for {0}")]
    MissingChecksum(PathBuf, Digest),
    /// Size mismatch, expected vs actual.
    #[error("Size mismatch for {0}: expected {1}, actual {2}")]
    Size(PathBuf, u64, u64),
    /// No checksum found for the requested Digest
    #[error("Missing size entry for {0}")]
    MissingSize(PathBuf),
}

impl Distinfo {
    /**
     * Return a new empty [`Distinfo`].
     */
    pub fn new() -> Distinfo {
        let di: Distinfo = Default::default();
        di
    }

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
     * Set the rcsid value.
     */
    pub fn set_rcsid(&mut self, rcsid: &OsString) {
        self.rcsid = Some(rcsid.clone());
    }
    /**
     * Return a matching distfile [`Entry`] if found, otherwise [`None`].
     */
    pub fn get_distfile<P: AsRef<Path>>(&self, path: P) -> Option<&Entry> {
        self.distfiles.get(path.as_ref())
    }

    /**
     * Return a matching patchfile [`Entry`] if found, otherwise [`None`].
     */
    pub fn get_patchfile<P: AsRef<Path>>(&self, path: P) -> Option<&Entry> {
        self.patchfiles.get(path.as_ref())
    }

    /**
     * Return a [`Vec`] of references to distfile entries, if any.
     */
    pub fn distfiles(&self) -> Vec<&Entry> {
        self.distfiles.values().collect()
    }

    /**
     * Return a [`Vec`] of references to patchfile entries, if any.
     */
    pub fn patchfiles(&self) -> Vec<&Entry> {
        self.patchfiles.values().collect()
    }

    /**
     * Calculate size of a [`PathBuf`].
     */
    pub fn calculate_size<P: AsRef<Path>>(
        path: P,
    ) -> Result<u64, DistinfoError> {
        let file = File::open(path)?;
        Ok(file.metadata()?.len())
    }

    /**
     * Calculate [`Digest`] hash for a [`Path`].  The hash will differ depending on the
     * [`EntryType`] of the supplied path.
     */
    pub fn calculate_checksum<P: AsRef<Path>>(
        path: P,
        digest: Digest,
    ) -> Result<String, DistinfoError> {
        let filetype = EntryType::from(path.as_ref());
        let mut f = File::open(path)?;
        match filetype {
            EntryType::Distfile => Ok(digest.hash_file(&mut f)?),
            EntryType::Patchfile => Ok(digest.hash_patch(&mut f)?),
        }
    }

    /**
     * Insert a populated [`Entry`] into the [`Distinfo`].
     */
    pub fn insert(&mut self, entry: Entry) -> bool {
        let map = match entry.filetype {
            EntryType::Distfile => &mut self.distfiles,
            EntryType::Patchfile => &mut self.patchfiles,
        };
        map.insert(entry.filename.clone(), entry).is_none()
    }

    /**
     * Find an [`Entry`] in the current [`Distinfo`] given a [`Path`].
     * [`Distinfo`] distfile entries may include a directory component
     * (`DIST_SUBDIR`) so applications can't simply look up by filename.
     *
     * This function iterates over the [`Path`] in reverse, adding any leading
     * components until an entry is found, or returns [`NotFound`].
     */
    pub fn find_entry<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<&Entry, DistinfoError> {
        let filetype = EntryType::from(path.as_ref());
        let mut file = PathBuf::new();
        for component in path.as_ref().iter().rev() {
            if file.parent().is_none() {
                file = PathBuf::from(component);
            } else {
                file = PathBuf::from(component).join(file);
            }
            match filetype {
                EntryType::Distfile => {
                    if let Some(entry) = self.get_distfile(&file) {
                        return Ok(entry);
                    }
                }
                EntryType::Patchfile => {
                    if let Some(entry) = self.get_patchfile(&file) {
                        return Ok(entry);
                    }
                }
            }
        }
        Err(DistinfoError::NotFound)
    }

    /**
     * Internal functions to update or insert entries in the current
     * [`Distinfo`], given a [`Path`] and its value data.
     */
    fn update_size<P: AsRef<Path>>(&mut self, path: P, size: u64) {
        let filetype = EntryType::from(path.as_ref());
        let map = match filetype {
            EntryType::Distfile => &mut self.distfiles,
            EntryType::Patchfile => &mut self.patchfiles,
        };
        match map.get_mut(path.as_ref()) {
            Some(entry) => entry.size = Some(size),
            None => {
                map.insert(
                    path.as_ref().to_path_buf(),
                    Entry {
                        filename: path.as_ref().to_path_buf(),
                        size: Some(size),
                        filetype,
                        ..Default::default()
                    },
                );
            }
        };
    }
    fn update_checksum<P: AsRef<Path>>(
        &mut self,
        path: P,
        digest: Digest,
        hash: String,
    ) {
        let filetype = EntryType::from(path.as_ref());
        let map = match filetype {
            EntryType::Distfile => &mut self.distfiles,
            EntryType::Patchfile => &mut self.patchfiles,
        };
        match map.get_mut(path.as_ref()) {
            Some(entry) => entry.checksums.push(Checksum { digest, hash }),
            None => {
                let v: Vec<Checksum> = vec![Checksum { digest, hash }];
                map.insert(
                    path.as_ref().to_path_buf(),
                    Entry {
                        filename: path.as_ref().to_path_buf(),
                        checksums: v,
                        filetype,
                        ..Default::default()
                    },
                );
            }
        };
    }

    /**
     * Pass the full path to a file to check as a [`PathBuf`] and verify that
     * it matches the size stored in the [`Distinfo`].
     *
     * Returns the size if [`Ok`], otherwise return a [`DistinfoError`].
     */
    pub fn verify_size<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<u64, DistinfoError> {
        let entry = self.find_entry(path.as_ref())?;
        entry.verify_size(path)
    }

    /**
     * Pass the full path to a file to check as a [`PathBuf`] and verify that
     * it matches a specific [`Digest`] checksum stored in the [`Distinfo`].
     *
     * Return the [`Digest`] if [`Ok`], otherwise return a [`DistinfoError`].
     *
     * To verify all stored checksums use use [`verify_checksums`].
     *
     * [`verify_checksums`]: Distinfo::verify_checksums
     */
    pub fn verify_checksum<P: AsRef<Path>>(
        &self,
        path: P,
        digest: Digest,
    ) -> Result<Digest, DistinfoError> {
        let entry = self.find_entry(path.as_ref())?;
        entry.verify_checksum_internal(path, digest)
    }

    /**
     * Pass the full path to a file to check as a [`PathBuf`] and verify that
     * it matches all of the checksums stored in the [`Distinfo`].  Returns a
     * [`Vec`] of [`Result`]s containing the [`Digest`] if [`Ok`], otherwise
     * return a [`DistinfoError`].
     */
    pub fn verify_checksums<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Vec<Result<Digest, DistinfoError>> {
        let entry = match self.find_entry(path.as_ref()) {
            Ok(entry) => entry,
            Err(e) => return vec![Err(e)],
        };
        let mut results = vec![];
        for c in &entry.checksums {
            results
                .push(entry.verify_checksum_internal(path.as_ref(), c.digest));
        }
        results
    }

    /**
     * Read a [`Vec`] of [`u8`] bytes and parse for [`Distinfo`] entries.  If
     * nothing is found then an empty [`Distinfo`] is returned.
     */
    pub fn from_bytes(bytes: &[u8]) -> Distinfo {
        let mut distinfo = Distinfo::new();
        for line in bytes.split(|c| *c == b'\n') {
            match Line::from_bytes(line) {
                /*
                 * We shouldn't encounter multiple RcsId entries, but if we do
                 * then last match wins.
                 */
                Line::RcsId(s) => distinfo.rcsid = Some(s),
                Line::Size(p, v) => {
                    distinfo.update_size(&p, v);
                }
                Line::Checksum(d, p, s) => {
                    distinfo.update_checksum(&p, d, s);
                }
                Line::None => {}
            }
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

        for q in self.distfiles.values() {
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
            if let Some(size) = q.size {
                bytes.extend_from_slice(
                    format!(
                        "Size ({}) = {} bytes\n",
                        q.filename.display(),
                        size
                    )
                    .as_bytes(),
                );
            }
        }

        for q in self.patchfiles.values() {
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

impl Line {
    fn from_bytes(bytes: &[u8]) -> Line {
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
                return Line::RcsId(OsString::from_vec((*line).to_vec()));
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
             * If we see anything we don't like then Line::None is
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
                        Err(_) => return Line::None,
                    };
                }
                /* Record path from "(filename)" */
                if field == 1 {
                    if s[0] == b'(' && s[s.len() - 1] == b')' {
                        path.push(OsStr::from_bytes(&s[1..s.len() - 1]));
                    } else {
                        return Line::None;
                    }
                }
                /* Record size or hash */
                if field == 3 {
                    value = match String::from_utf8(s.to_vec()) {
                        Ok(s) => s,
                        Err(_) => return Line::None,
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
                    Ok(n) => return Line::Size(path, n),
                    Err(_) => return Line::None,
                };
            } else {
                match Digest::from_str(&action) {
                    Ok(d) => return Line::Checksum(d, path, value),
                    Err(_) => return Line::None,
                }
            }
        }
        Line::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /*
     * Test RcsId parsing, with and without additional whitespace and comments.
     */
    #[test]
    fn test_line_rcsid() {
        let rcsid = "$NetBSD: distinfo,v 1.1 1970/01/01 01:01:01 ken Exp $";
        let exp = Line::RcsId(rcsid.into());

        assert_eq!(Line::from_bytes(rcsid.as_bytes()), exp);
        assert_eq!(Line::from_bytes(format!("     {rcsid}").as_bytes()), exp);
        assert_eq!(Line::from_bytes(format!("\n\n {rcsid}").as_bytes()), exp);
        assert_eq!(Line::from_bytes(format!(" {rcsid}\n\n").as_bytes()), exp);

        /* Commented entry should return None */
        let entry = Line::from_bytes(format!("#{rcsid}").as_bytes());
        assert_eq!(entry, Line::None);
    }

    #[test]
    fn test_line_size() {
        /*
         * Regular entry
         */
        let i = "Size    (foo-1.2.3.tar.gz)    =    321     bytes";
        let o = Line::from_bytes(i.as_bytes());
        assert_eq!(o, Line::Size(PathBuf::from("foo-1.2.3.tar.gz"), 321));

        /*
         * Entry with extra whitespace is accepted, but in reality is likely
         * to be rejected by other tools.
         */
        let i = "Size    (foo-1.2.3.tar.gz)    =    321     bytes";
        let o = Line::from_bytes(i.as_bytes());
        assert_eq!(o, Line::Size(PathBuf::from("foo-1.2.3.tar.gz"), 321));

        /*
         * Invalid as it's missing "bytes", but accepted anyway.
         */
        let i = "Size (foo-1.2.3.tar.gz) = 123";
        let o = Line::from_bytes(i.as_bytes());
        assert_eq!(o, Line::Size(PathBuf::from("foo-1.2.3.tar.gz"), 123));

        /*
         * Check for u64 overflow
         */
        let i = "Size (a.tar.gz) = 18446744073709551615";
        let o = Line::from_bytes(i.as_bytes());
        assert_eq!(
            o,
            Line::Size(PathBuf::from("a.tar.gz"), 18446744073709551615)
        );
        let i = "Size (a.tar.gz) = 18446744073709551616";
        let o = Line::from_bytes(i.as_bytes());
        assert_eq!(o, Line::None);
    }

    #[test]
    fn test_line_digest() {
        let i = "BLAKE2s (pkgin-23.8.1.tar.gz) = ojnk";
        let o = Line::from_bytes(i.as_bytes());
        assert_eq!(
            o,
            Line::Checksum(
                Digest::BLAKE2s,
                PathBuf::from("pkgin-23.8.1.tar.gz"),
                "ojnk".to_string()
            )
        );
    }

    #[test]
    fn test_line_none() {
        let o = Line::from_bytes(format!("").as_bytes());
        assert_eq!(o, Line::None);
        let o = Line::from_bytes(format!("\n  \n\n").as_bytes());
        assert_eq!(o, Line::None);
        let o = Line::from_bytes(format!("#  \n\n").as_bytes());
        assert_eq!(o, Line::None);
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
        let f = di.get_distfile("pkgin-23.8.1.tar.gz");
        assert!(matches!(f, Some(_)));
        let p = di.get_patchfile("patch-configure.ac");
        assert!(matches!(p, Some(_)));
        assert_eq!(None, di.get_distfile("foo-23.8.1.tar.gz"));
        assert_eq!(None, di.get_patchfile("patch-Makefile"));
    }

    #[test]
    fn test_construct() {
        let mut di = Distinfo::new();

        let mut distsums: Vec<Checksum> = Vec::new();
        distsums.push(Checksum::new(Digest::BLAKE2s, String::new()));
        distsums.push(Checksum::new(Digest::SHA512, String::new()));

        let entry =
            Entry::new("foo.tar.gz", "/distfiles/foo.tar.gz", distsums, None);

        /* First insert is created, returns true */
        assert_eq!(di.insert(entry.clone()), true);

        /* Second insert is an update, returns false */
        assert_eq!(di.insert(entry.clone()), false);

        assert_eq!(di.distfiles()[0].filetype, EntryType::Distfile);
        assert_eq!(di.distfiles().len(), 1);

        let mut patchsums: Vec<Checksum> = Vec::new();
        patchsums.push(Checksum::new(Digest::SHA1, String::new()));

        di.insert(Entry::new(
            "patch-Makefile",
            "patches/patch-Makefile",
            patchsums,
            None,
        ));

        assert_eq!(di.patchfiles().len(), 1);
        assert_eq!(di.patchfiles()[0].filetype, EntryType::Patchfile);
    }
}
