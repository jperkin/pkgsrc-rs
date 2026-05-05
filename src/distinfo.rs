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
 * use std::ffi::OsStr;
 *
 * let input = r#"
 *     $NetBSD: distinfo,v 1.80 2024/05/27 23:27:10 riastradh Exp $
 *
 *     BLAKE2s (pkgin-23.8.1.tar.gz) = eb0f008ba9801a3c0a35de3e2b2503edd554c3cb17235b347bb8274a18794eb7
 *     SHA512 (pkgin-23.8.1.tar.gz) = 2561d9e4b28a9a77c3c798612ec489dd67dd9a93c61344937095b0683fa89d8432a9ab8e600d0e2995d954888ac2e75a407bab08aa1e8198e375c99d2999f233
 *     Size (pkgin-23.8.1.tar.gz) = 267029 bytes
 *     SHA1 (patch-configure.ac) = 53f56351fb602d9fdce2c1ed266d65919a369086
 *     "#;
 * let distinfo = Distinfo::from_bytes(input.as_bytes());
 * assert_eq!(distinfo.rcsid(), Some(OsStr::new("$NetBSD: distinfo,v 1.80 2024/05/27 23:27:10 riastradh Exp $")));
 * ```
 *
 * As `distinfo` files can contain usernames and filenames that are not UTF-8
 * clean (for example ISO-8859), `from_bytes()` is the method used to parse
 * input, and the rcsid and filename portions are parsed as [`OsString`].  The
 * remaining sections must be UTF-8 clean and are regular [`String`]s.
 */

use crate::digest::{Digest, DigestError};
use indexmap::IndexMap;
use indexmap::map::Values;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;

/**
 * [`Checksum`] contains the [`Digest`] type and the [`String`] hash the digest
 * algorithm calculated for an associated [`Entry`].
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

impl EntryType {
    /**
     * Classify a path based on its file name component.  Judges by basename
     * alone and is appropriate for filesystem paths handed to
     * [`Distinfo::calculate_checksum`].
     */
    pub fn classify<P: AsRef<Path>>(path: P) -> EntryType {
        if Self::is_patch_filename(&path) {
            EntryType::Patchfile
        } else {
            EntryType::Distfile
        }
    }

    /**
     * Returns true if the path is a valid patch filename for inclusion in a
     * distinfo.  Returns false for backup/temporary files (.orig, .rej, ~),
     * local patches (patch-local-*), and files that don't match the patch
     * naming pattern.
     *
     * Must handle quirks such as "patch-2.7.6.tar.xz" which is a distfile not
     * a patch.  This is obviously not exhaustive.
     */
    pub fn is_patch_filename<P: AsRef<Path>>(path: P) -> bool {
        let Some(p) = path.as_ref().file_name() else {
            return false;
        };
        let s = p.to_string_lossy();
        if s.starts_with("patch-local-")
            || s.ends_with(".orig")
            || s.ends_with(".rej")
            || s.ends_with("~")
        {
            return false;
        }
        if s.contains(".tar.") {
            return false;
        }
        s.starts_with("patch-")
            || (s.starts_with("emul-") && s.contains("-patch-"))
    }
}

/*
 * Classify a path as it appears in a `distinfo` file.  pkgsrc records
 * patches by basename only; anything with a directory component is a
 * distfile distributed under a `DIST_SUBDIR` (for example
 * `mush/patch-7.2.6-alpha-1` is an upstream archive, not a pkgsrc patch).
 */
fn classify_stored(path: &Path) -> EntryType {
    if path.parent().is_some_and(|p| !p.as_os_str().is_empty()) {
        EntryType::Distfile
    } else {
        EntryType::classify(path)
    }
}

/**
 * [`Entry`] contains the information stored about each unique file listed in
 * the distinfo file.
 */
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Entry {
    filename: PathBuf,
    filepath: PathBuf,
    size: Option<u64>,
    checksums: Vec<Checksum>,
    filetype: EntryType,
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
        let filetype = classify_stored(filename.as_ref());
        Entry {
            filename: filename.as_ref().to_path_buf(),
            filepath: filepath.as_ref().to_path_buf(),
            checksums,
            size,
            filetype,
        }
    }

    /**
     * Path relative to a certain directory (usually `DISTDIR`) where this
     * entry is stored.  This may contain a directory portion, for example if
     * the package uses DIST_SUBDIR.  This is the string that will be stored
     * in the resulting `distinfo` file.
     */
    pub fn filename(&self) -> &Path {
        &self.filename
    }

    /**
     * Full path to filename.  This is not used in the `distinfo` file but is
     * stored here for processing purposes.
     */
    pub fn filepath(&self) -> &Path {
        &self.filepath
    }

    /**
     * File size.  This field is not currently used for patch files, as they
     * are distributed alongside the distinfo file and are not downloaded
     * separately, thus a single hash check is sufficient.
     */
    pub fn size(&self) -> Option<u64> {
        self.size
    }

    /**
     * List of checksums, one [`Checksum`] entry per Digest type.  These are in
     * order of appearance in the `distinfo` file.
     */
    pub fn checksums(&self) -> &[Checksum] {
        &self.checksums
    }

    /**
     * Whether this entry is a distfile or a patchfile.
     */
    pub fn filetype(&self) -> EntryType {
        self.filetype
    }

    /**
     * Pass the full path to a file to check as a [`PathBuf`] and verify that
     * it matches the size stored in the [`Distinfo`].
     *
     * Returns the size if [`Ok`], otherwise return an [`DistinfoError`].
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
     * Pass the full path to a file to check as a [`PathBuf`] and verify that
     * it matches a specific [`Digest`] checksum stored in the [`Distinfo`].
     *
     * Return the [`Digest`] if [`Ok`], otherwise return an [`DistinfoError`].
     *
     * To verify all stored checksums use [`verify_checksums`].
     *
     * [`verify_checksums`]: Entry::verify_checksums
     */
    pub fn verify_checksum<P: AsRef<Path>>(
        &self,
        path: P,
        digest: Digest,
    ) -> Result<Digest, DistinfoError> {
        for c in &self.checksums {
            if digest != c.digest {
                continue;
            }
            let mut f = File::open(path.as_ref())?;
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
     * it matches all of the checksums stored in the [`Distinfo`].  The file
     * is opened and read exactly once regardless of how many algorithms are
     * recorded.  The returned vector contains one inner [`Result`] per
     * [`Checksum`] in order; the outer [`Result`] reports failures that
     * prevent verification from starting, such as the file being unreadable.
     */
    pub fn verify_checksums<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Vec<Result<Digest, DistinfoError>>, DistinfoError> {
        let path = path.as_ref();
        if self.checksums.is_empty() {
            return Ok(Vec::new());
        }
        let digests: Vec<Digest> =
            self.checksums.iter().map(|c| c.digest).collect();
        let mut file = File::open(path)?;
        let actual = match self.filetype {
            EntryType::Distfile => {
                Digest::multi_hash_file(&mut file, &digests)?
            }
            EntryType::Patchfile => {
                Digest::multi_hash_patch(&mut file, &digests)?
            }
        };
        Ok(self
            .checksums
            .iter()
            .zip(actual)
            .map(|(c, hash)| {
                if hash == c.hash {
                    Ok(c.digest)
                } else {
                    Err(DistinfoError::Checksum(
                        self.filename.clone(),
                        c.digest,
                        c.hash.clone(),
                        hash,
                    ))
                }
            })
            .collect())
    }

    /**
     * Write this entry's `distinfo` lines to `writer`.  Filenames are written
     * verbatim as bytes so that non-UTF-8 names round-trip correctly.
     */
    pub fn write_to<W: Write>(&self, mut writer: W) -> io::Result<()> {
        let name = self.filename.as_os_str().as_bytes();
        for c in &self.checksums {
            write!(writer, "{} (", c.digest)?;
            writer.write_all(name)?;
            writeln!(writer, ") = {}", c.hash)?;
        }
        if let Some(size) = self.size {
            write!(writer, "Size (")?;
            writer.write_all(name)?;
            writeln!(writer, ") = {size} bytes")?;
        }
        Ok(())
    }

    /**
     * Convenience wrapper around [`Entry::write_to`] that returns an owned
     * byte vector.
     */
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let _ = self.write_to(&mut out);
        out
    }
}

/**
 * [`Distinfo`] contains the contents of a `distinfo` file.
 *
 * The primary interface for populating a [`Distinfo`] from an existing
 * `distinfo` file is using the [`from_bytes`] function.  There is no error
 * handling.  Any input that is unrecognised or not in the correct format is
 * simply ignored.
 *
 * To create a new `distinfo` file, use [`new`] and populate it with
 * [`insert`].
 *
 * [`from_bytes`]: Distinfo::from_bytes
 * [`new`]: Distinfo::new
 * [`insert`]: Distinfo::insert
 */
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    #[error("File not found: {0}")]
    NotFound(PathBuf),
    /// Checksum mismatch, expected vs actual.
    #[error("Checksum {1} mismatch for {0}: expected {2}, actual {3}")]
    Checksum(PathBuf, Digest, String, String),
    /// No checksum found for the requested Digest.
    #[error("Missing {1} checksum entry for {0}")]
    MissingChecksum(PathBuf, Digest),
    /// Size mismatch, expected vs actual.
    #[error("Size mismatch for {0}: expected {1}, actual {2}")]
    Size(PathBuf, u64, u64),
    /// No size found for the requested entry.
    #[error("Missing size entry for {0}")]
    MissingSize(PathBuf),
}

impl Distinfo {
    /**
     * Return a new empty [`Distinfo`].
     */
    pub fn new() -> Self {
        Self::default()
    }

    /**
     * Return an [`Option`] containing either a valid `$NetBSD: ...` RCS Id
     * line, or None if one was not found.
     */
    pub fn rcsid(&self) -> Option<&OsStr> {
        self.rcsid.as_deref()
    }

    /**
     * Set the rcsid value, returning the previous value if any.
     */
    pub fn set_rcsid(
        &mut self,
        rcsid: impl Into<OsString>,
    ) -> Option<OsString> {
        self.rcsid.replace(rcsid.into())
    }

    /**
     * Return a matching distfile [`Entry`] if found, otherwise [`None`].
     */
    pub fn distfile<P: AsRef<Path>>(&self, name: P) -> Option<&Entry> {
        self.distfiles.get(name.as_ref())
    }

    /**
     * Return a matching patchfile [`Entry`] if found, otherwise [`None`].
     */
    pub fn patchfile<P: AsRef<Path>>(&self, name: P) -> Option<&Entry> {
        self.patchfiles.get(name.as_ref())
    }

    /**
     * Return an iterator of distfile entries in insertion order.
     */
    pub fn distfiles(&self) -> Values<'_, PathBuf, Entry> {
        self.distfiles.values()
    }

    /**
     * Return an iterator of patchfile entries in insertion order.
     */
    pub fn patchfiles(&self) -> Values<'_, PathBuf, Entry> {
        self.patchfiles.values()
    }

    /**
     * Find an [`Entry`] in the current [`Distinfo`] given a [`Path`].
     * [`Distinfo`] distfile entries may include a directory component
     * (`DIST_SUBDIR`) and patches are conventionally stored under a
     * `patches/` prefix on disk, so applications can't simply look up by
     * filename.
     *
     * This function iterates over the [`Path`] in reverse, adding any
     * leading components until an entry is found in either namespace, or
     * returns [`None`].
     */
    pub fn find<P: AsRef<Path>>(&self, path: P) -> Option<&Entry> {
        let path = path.as_ref();
        let mut suffix = PathBuf::new();
        for component in path.iter().rev() {
            suffix = Path::new(component).join(&suffix);
            if let Some(entry) = self
                .distfiles
                .get(&suffix)
                .or_else(|| self.patchfiles.get(&suffix))
            {
                return Some(entry);
            }
        }
        None
    }

    /**
     * Insert a populated [`Entry`] into the [`Distinfo`].  Returns the
     * previous entry stored under the same filename, if any.
     */
    pub fn insert(&mut self, entry: Entry) -> Option<Entry> {
        let map = match entry.filetype {
            EntryType::Distfile => &mut self.distfiles,
            EntryType::Patchfile => &mut self.patchfiles,
        };
        map.insert(entry.filename.clone(), entry)
    }

    /**
     * Calculate size of a [`Path`].
     */
    pub fn calculate_size<P: AsRef<Path>>(path: P) -> io::Result<u64> {
        Ok(File::open(path)?.metadata()?.len())
    }

    /**
     * Calculate [`Digest`] hash for a [`Path`].  The hash will differ
     * depending on the [`EntryType`] of the supplied path.
     */
    pub fn calculate_checksum<P: AsRef<Path>>(
        path: P,
        digest: Digest,
    ) -> Result<String, DistinfoError> {
        let path = path.as_ref();
        let mut file = File::open(path)?;
        let hash = match EntryType::classify(path) {
            EntryType::Distfile => digest.hash_file(&mut file)?,
            EntryType::Patchfile => digest.hash_patch(&mut file)?,
        };
        Ok(hash)
    }

    /**
     * Pass the full path to a file to check as a [`PathBuf`] and verify that
     * it matches the size stored in the [`Distinfo`].
     *
     * Returns the size if [`Ok`], otherwise return an [`DistinfoError`].
     */
    pub fn verify_size<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<u64, DistinfoError> {
        let path = path.as_ref();
        self.find(path)
            .ok_or_else(|| DistinfoError::NotFound(path.into()))?
            .verify_size(path)
    }

    /**
     * Pass the full path to a file to check as a [`PathBuf`] and verify that
     * it matches a specific [`Digest`] checksum stored in the [`Distinfo`].
     *
     * Return the [`Digest`] if [`Ok`], otherwise return an [`DistinfoError`].
     *
     * To verify all stored checksums use [`verify_checksums`].
     *
     * [`verify_checksums`]: Distinfo::verify_checksums
     */
    pub fn verify_checksum<P: AsRef<Path>>(
        &self,
        path: P,
        digest: Digest,
    ) -> Result<Digest, DistinfoError> {
        let path = path.as_ref();
        self.find(path)
            .ok_or_else(|| DistinfoError::NotFound(path.into()))?
            .verify_checksum(path, digest)
    }

    /**
     * Pass the full path to a file to check as a [`PathBuf`] and verify that
     * it matches all of the checksums stored in the [`Distinfo`].  See
     * [`Entry::verify_checksums`] for the return shape.
     */
    pub fn verify_checksums<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<Vec<Result<Digest, DistinfoError>>, DistinfoError> {
        let path = path.as_ref();
        self.find(path)
            .ok_or_else(|| DistinfoError::NotFound(path.into()))?
            .verify_checksums(path)
    }

    /**
     * Read a [`Vec`] of [`u8`] bytes and parse for [`Distinfo`] entries.  If
     * nothing is found then an empty [`Distinfo`] is returned.
     */
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut distinfo = Self::new();
        for line in bytes.split(|c| *c == b'\n') {
            match Line::from_bytes(line) {
                /*
                 * We shouldn't encounter multiple RcsId entries, but if we do
                 * then last match wins.
                 */
                Line::RcsId(s) => distinfo.rcsid = Some(s),
                Line::Size { path, size } => distinfo.upsert_size(path, size),
                Line::Checksum { digest, path, hash } => {
                    distinfo.upsert_checksum(path, digest, hash);
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
    pub fn write_to<W: Write>(&self, mut writer: W) -> io::Result<()> {
        match self.rcsid() {
            Some(s) => writer.write_all(s.as_bytes())?,
            None => writer.write_all(b"$NetBSD$")?,
        }
        writer.write_all(b"\n\n")?;
        for entry in self.distfiles.values().chain(self.patchfiles.values()) {
            entry.write_to(&mut writer)?;
        }
        Ok(())
    }

    /**
     * Convenience wrapper around [`Distinfo::write_to`] that returns an
     * owned byte vector.
     */
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let _ = self.write_to(&mut out);
        out
    }

    /*
     * Internal functions to update or insert entries in the current
     * [`Distinfo`], given a [`Path`] and its value data.
     */
    fn upsert_size(&mut self, path: PathBuf, size: u64) {
        let filetype = classify_stored(&path);
        let map = match filetype {
            EntryType::Distfile => &mut self.distfiles,
            EntryType::Patchfile => &mut self.patchfiles,
        };
        map.entry(path.clone())
            .and_modify(|e| e.size = Some(size))
            .or_insert_with(|| Entry {
                filename: path,
                size: Some(size),
                filetype,
                ..Default::default()
            });
    }

    fn upsert_checksum(&mut self, path: PathBuf, digest: Digest, hash: String) {
        let filetype = classify_stored(&path);
        let map = match filetype {
            EntryType::Distfile => &mut self.distfiles,
            EntryType::Patchfile => &mut self.patchfiles,
        };
        let checksum = Checksum { digest, hash };
        map.entry(path.clone())
            .and_modify(|e| e.checksums.push(checksum.clone()))
            .or_insert_with(|| Entry {
                filename: path,
                checksums: vec![checksum],
                filetype,
                ..Default::default()
            });
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
    Size {
        path: PathBuf,
        size: u64,
    },
    Checksum {
        digest: Digest,
        path: PathBuf,
        hash: String,
    },
    None,
}

impl Line {
    fn from_bytes(mut line: &[u8]) -> Line {
        /*
         * Skip leading whitespace.  Technically this isn't supported, but
         * be liberal in what you accept...
         */
        while let Some((&c, rest)) = line.split_first() {
            if (c as char).is_whitespace() {
                line = rest;
            } else {
                break;
            }
        }

        /* Skip comments and empty lines */
        if line.is_empty() || line.starts_with(b"#") {
            return Line::None;
        }

        /*
         * Match NetBSD RCS Id.  Only match an expanded "$NetBSD: ..."
         * string, there's no point matching an unexpanded "$NetBSD$".
         */
        if line.starts_with(b"$NetBSD: ") {
            return Line::RcsId(OsString::from_vec(line.to_vec()));
        }

        /*
         * The remaining types are matched the same, because the important
         * parts are in the same place:
         *
         *   DIGEST (FILENAME) = HASH
         *   Size (FILENAME) = BYTES bytes
         *
         * We just ignore the trailing "bytes" of "Size" lines.  If we see
         * anything we don't like then Line::None is immediately returned.
         */
        let mut fields = line
            .split(|b| (*b as char).is_whitespace())
            .filter(|s| !s.is_empty());

        let action =
            match fields.next().and_then(|s| std::str::from_utf8(s).ok()) {
                Some(s) => s,
                None => return Line::None,
            };
        let path_field = match fields.next() {
            Some(s) => s,
            None => return Line::None,
        };
        if fields.next().is_none() {
            return Line::None;
        }
        let value =
            match fields.next().and_then(|s| std::str::from_utf8(s).ok()) {
                Some(s) => s,
                None => return Line::None,
            };

        let path = match path_field
            .strip_prefix(b"(")
            .and_then(|s| s.strip_suffix(b")"))
        {
            Some(inner) => PathBuf::from(OsStr::from_bytes(inner)),
            None => return Line::None,
        };

        /*
         * Valid actions are "Size", or a valid Digest type.  Anything
         * else is unmatched.
         */
        if action == "Size" {
            match u64::from_str(value) {
                Ok(size) => Line::Size { path, size },
                Err(_) => Line::None,
            }
        } else {
            match Digest::from_str(action) {
                Ok(digest) => Line::Checksum {
                    digest,
                    path,
                    hash: value.to_string(),
                },
                Err(_) => Line::None,
            }
        }
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
        assert_eq!(
            o,
            Line::Size {
                path: PathBuf::from("foo-1.2.3.tar.gz"),
                size: 321,
            },
        );

        /*
         * Invalid as it's missing "bytes", but accepted anyway.
         */
        let i = "Size (foo-1.2.3.tar.gz) = 123";
        let o = Line::from_bytes(i.as_bytes());
        assert_eq!(
            o,
            Line::Size {
                path: PathBuf::from("foo-1.2.3.tar.gz"),
                size: 123,
            },
        );

        /*
         * Check for u64 overflow
         */
        let i = "Size (a.tar.gz) = 18446744073709551615";
        let o = Line::from_bytes(i.as_bytes());
        assert_eq!(
            o,
            Line::Size {
                path: PathBuf::from("a.tar.gz"),
                size: 18446744073709551615,
            },
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
            Line::Checksum {
                digest: Digest::BLAKE2s,
                path: PathBuf::from("pkgin-23.8.1.tar.gz"),
                hash: "ojnk".to_string(),
            },
        );
    }

    #[test]
    fn test_line_none() {
        let o = Line::from_bytes(String::new().as_bytes());
        assert_eq!(o, Line::None);
        let o = Line::from_bytes("\n  \n\n".to_string().as_bytes());
        assert_eq!(o, Line::None);
        let o = Line::from_bytes("#  \n\n".to_string().as_bytes());
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
            Some(OsStr::new(
                "$NetBSD: distinfo,v 1.80 2024/05/27 23:27:10 riastradh Exp $"
            )),
        );
        assert!(di.distfile("pkgin-23.8.1.tar.gz").is_some());
        assert!(di.patchfile("patch-configure.ac").is_some());
        assert!(di.distfile("foo-23.8.1.tar.gz").is_none());
        assert!(di.patchfile("patch-Makefile").is_none());
    }

    #[test]
    fn test_construct() {
        let mut di = Distinfo::new();

        let entry = Entry::new(
            "foo.tar.gz",
            "/distfiles/foo.tar.gz",
            vec![
                Checksum::new(Digest::BLAKE2s, String::new()),
                Checksum::new(Digest::SHA512, String::new()),
            ],
            None,
        );

        /* First insert returns None (no previous entry). */
        assert!(di.insert(entry.clone()).is_none());

        /* Second insert returns the previous entry. */
        assert_eq!(di.insert(entry.clone()), Some(entry));

        let mut distfiles = di.distfiles();
        let first = distfiles.next().expect("at least one distfile");
        assert_eq!(first.filetype(), EntryType::Distfile);
        assert!(distfiles.next().is_none());

        di.insert(Entry::new(
            "patch-Makefile",
            "patches/patch-Makefile",
            vec![Checksum::new(Digest::SHA1, String::new())],
            None,
        ));

        let mut patchfiles = di.patchfiles();
        let p = patchfiles.next().expect("at least one patchfile");
        assert_eq!(p.filetype(), EntryType::Patchfile);
    }

    #[test]
    fn test_is_patch_filename() {
        assert!(EntryType::is_patch_filename("patch-Makefile"));
        assert!(EntryType::is_patch_filename("patch-configure.ac"));
        assert!(EntryType::is_patch_filename("emul-linux-x86-patch-foo"));

        assert!(!EntryType::is_patch_filename("patch-local-foo"));
        assert!(!EntryType::is_patch_filename("patch-Makefile.orig"));
        assert!(!EntryType::is_patch_filename("patch-Makefile.rej"));
        assert!(!EntryType::is_patch_filename("patch-Makefile~"));

        assert!(!EntryType::is_patch_filename("foo-1.0.tar.gz"));
        assert!(!EntryType::is_patch_filename("patch-2.7.6.tar.xz"));
        assert!(!EntryType::is_patch_filename("emul-foo.tar.gz"));
    }

    #[test]
    fn test_set_rcsid() {
        let mut di = Distinfo::new();
        assert_eq!(di.rcsid(), None);

        assert_eq!(di.set_rcsid("$NetBSD$"), None);
        assert_eq!(di.rcsid(), Some(OsStr::new("$NetBSD$")));

        let prev = di.set_rcsid(OsString::from("$NetBSD: test $"));
        assert_eq!(prev, Some(OsString::from("$NetBSD$")));
        assert_eq!(di.rcsid(), Some(OsStr::new("$NetBSD: test $")));
    }

    #[test]
    fn test_entry_to_bytes() {
        let entry = Entry::new(
            "foo-1.0.tar.gz",
            "/distfiles/foo-1.0.tar.gz",
            vec![
                Checksum::new(Digest::BLAKE2s, "abc123".to_string()),
                Checksum::new(Digest::SHA512, "def456".to_string()),
            ],
            Some(12345),
        );
        let s = String::from_utf8(entry.to_bytes()).expect("valid utf8");
        assert!(s.contains("BLAKE2s (foo-1.0.tar.gz) = abc123\n"));
        assert!(s.contains("SHA512 (foo-1.0.tar.gz) = def456\n"));
        assert!(s.contains("Size (foo-1.0.tar.gz) = 12345 bytes\n"));
    }

    #[test]
    fn test_entry_to_bytes_no_size() {
        let entry = Entry::new(
            "patch-Makefile",
            "patches/patch-Makefile",
            vec![Checksum::new(Digest::SHA1, "abc123".to_string())],
            None,
        );
        let s = String::from_utf8(entry.to_bytes()).expect("valid utf8");
        assert!(s.contains("SHA1 (patch-Makefile) = abc123\n"));
        assert!(!s.contains("Size"));
    }

    #[test]
    fn test_distinfo_to_bytes() {
        let input = concat!(
            "$NetBSD: distinfo,v 1.1 2024/01/01 00:00:00 user Exp $\n",
            "\n",
            "BLAKE2s (foo-1.0.tar.gz) = abc123\n",
            "SHA512 (foo-1.0.tar.gz) = def456\n",
            "Size (foo-1.0.tar.gz) = 99999 bytes\n",
            "SHA1 (patch-Makefile) = fedcba\n",
        );
        let di = Distinfo::from_bytes(input.as_bytes());
        let s = String::from_utf8(di.to_bytes()).expect("valid utf8");
        assert!(s.starts_with("$NetBSD: distinfo,v 1.1"));
        assert!(s.contains("BLAKE2s (foo-1.0.tar.gz) = abc123\n"));
        assert!(s.contains("SHA512 (foo-1.0.tar.gz) = def456\n"));
        assert!(s.contains("Size (foo-1.0.tar.gz) = 99999 bytes\n"));
        assert!(s.contains("SHA1 (patch-Makefile) = fedcba\n"));
    }

    #[test]
    fn test_line_malformed() {
        /* Missing parentheses around filename */
        let o = Line::from_bytes(b"SHA1 foo = abc123");
        assert_eq!(o, Line::None);

        /* Non-UTF8 action field */
        let o = Line::from_bytes(b"\xff\xfe (foo) = abc");
        assert_eq!(o, Line::None);

        /* Non-UTF8 value field */
        let o = Line::from_bytes(b"SHA1 (foo) = \xff\xfe");
        assert_eq!(o, Line::None);

        /* Unknown digest type */
        let o = Line::from_bytes(b"BOGUS (foo) = abc");
        assert_eq!(o, Line::None);
    }

    #[test]
    fn test_calculate_size() -> Result<(), DistinfoError> {
        let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        file.push("tests/data/digest.txt");
        let size = Distinfo::calculate_size(&file)?;
        assert_eq!(size, 158);
        Ok(())
    }

    #[test]
    fn test_calculate_checksum() -> Result<(), DistinfoError> {
        let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        file.push("tests/data/digest.txt");
        let hash = Distinfo::calculate_checksum(&file, Digest::BLAKE2s)?;
        assert_eq!(
            hash,
            "555e56e8177159b7d7fe96d5068dcf5335b554b917c8daaa4c893ec4f04b5303"
        );

        let mut patch = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        patch.push("tests/data/patch-Makefile");
        let hash = Distinfo::calculate_checksum(&patch, Digest::SHA1)?;
        assert_eq!(hash, "ab5ce8a374d3aca7948eecabc35386d8195e3fbf");
        Ok(())
    }

    #[test]
    fn test_distinfo_to_bytes_no_rcsid() {
        let mut di = Distinfo::new();
        di.insert(Entry::new(
            "foo-1.0.tar.gz",
            "/distfiles/foo-1.0.tar.gz",
            vec![Checksum::new(Digest::SHA1, "abc".to_string())],
            None,
        ));
        let s = String::from_utf8(di.to_bytes()).expect("valid utf8");
        assert!(s.starts_with("$NetBSD$\n\n"));
        assert!(s.contains("SHA1 (foo-1.0.tar.gz) = abc\n"));
    }

    #[test]
    fn test_classify_stored_dist_subdir() {
        /*
         * Anything stored with a directory component is a distfile under
         * DIST_SUBDIR, regardless of how the basename looks.  This is the
         * stricter rule applied during parsing and Entry::new.
         */
        assert_eq!(
            classify_stored(Path::new("mush/patch-7.2.6-alpha-1")),
            EntryType::Distfile,
        );
        assert_eq!(
            classify_stored(Path::new("foo/patch-Makefile")),
            EntryType::Distfile,
        );
        assert_eq!(
            classify_stored(Path::new("patch-Makefile")),
            EntryType::Patchfile,
        );
    }

    #[test]
    fn test_find_with_patches_prefix() {
        let mut di = Distinfo::new();
        di.insert(Entry::new(
            "patch-aa",
            "patches/patch-aa",
            vec![Checksum::new(Digest::SHA1, "abc".to_string())],
            None,
        ));
        /*
         * Callers commonly hand a full filesystem path; find() must walk up
         * components to match the basename-only patch entry.
         */
        let found = di.find("patches/patch-aa").expect("entry resolves");
        assert_eq!(found.filetype(), EntryType::Patchfile);
    }

    #[test]
    fn test_subdir_patchlike_distfile() {
        let input = concat!(
            "$NetBSD$\n\n",
            "BLAKE2s (mush/patch-7.2.6-alpha-1) = abc\n",
            "Size (mush/patch-7.2.6-alpha-1) = 42 bytes\n",
        );
        let di = Distinfo::from_bytes(input.as_bytes());
        let entry = di
            .distfile("mush/patch-7.2.6-alpha-1")
            .expect("classified as distfile");
        assert_eq!(entry.filetype(), EntryType::Distfile);
        assert_eq!(entry.size(), Some(42));
        /* Round-trip preserves the Size line. */
        assert_eq!(di.to_bytes(), input.as_bytes());
    }
}
