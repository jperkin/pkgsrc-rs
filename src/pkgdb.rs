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
 *
 * pkgdb.rs - handle the package database
 */

/*!
 * Package database access.
 *
 * This module provides read access to the pkgsrc package database,
 * allowing iteration over installed packages and access to their metadata.
 *
 * # Example
 *
 * ```no_run
 * use pkgsrc::metadata::FileRead;
 * use pkgsrc::pkgdb::PkgDB;
 * use std::io;
 * use std::path::Path;
 *
 * fn main() -> io::Result<()> {
 *     let db = PkgDB::open(Path::new("/var/db/pkg"))?;
 *     for result in db {
 *         let pkg = result?;
 *         println!("{}: {}", pkg.pkgname(), pkg.comment()?);
 *     }
 *     Ok(())
 * }
 * ```
 */
use crate::metadata::{Entry, FileRead};
use std::fs;
use std::fs::ReadDir;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;

/**
 * Errors that can occur when working with the package database.
 */
#[derive(Debug, Error)]
pub enum PkgDBError {
    /**
     * An I/O error occurred.
     */
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /**
     * The specified path is not a valid package database.
     */
    #[error("Invalid package database: {0}")]
    InvalidDatabase(PathBuf),

    /**
     * The package name could not be parsed.
     */
    #[error("Invalid package name: {0}")]
    InvalidPackageName(String),

    /**
     * The package database iterator was not properly initialized.
     */
    #[error("Package database not properly initialized")]
    UninitializedDatabase,
}

/**
 * Type of pkgdb.  Currently supported formats are `Files` for the legacy
 * directory of `+*` files, and `Database` for a sqlite3 backend.
 */
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DBType {
    /**
     * Standard pkg_install pkgdb using files.
     */
    Files,
    /**
     * Future work to support sqlite3 backend.  Currently unimplemented.
     */
    Database,
}

/**
 * A handle to an opened package database.
 */
#[derive(Debug)]
pub struct PkgDB {
    dbtype: DBType,
    path: PathBuf,
    readdir: Option<ReadDir>,
}

/**
 * An installed package in a PkgDB.
 */
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledPackage {
    path: PathBuf,
    pkgbase: String,
    pkgname: String,
    pkgversion: String,
}

impl PkgDB {
    /**
     * Open an existing `PkgDB`.
     */
    pub fn open(path: &Path) -> Result<PkgDB, io::Error> {
        if path.is_dir() {
            let readdir = fs::read_dir(path)?;
            Ok(PkgDB {
                dbtype: DBType::Files,
                path: path.to_path_buf(),
                readdir: Some(readdir),
            })
        } else if path.is_file() {
            Ok(PkgDB {
                dbtype: DBType::Database,
                path: path.to_path_buf(),
                readdir: None,
            })
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "Invalid pkgdb"))
        }
    }

    /**
     * Return the path to this package database.
     */
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /**
     * Return the type of this package database.
     */
    #[must_use]
    pub fn dbtype(&self) -> DBType {
        self.dbtype
    }

    /**
     * Check if a directory is a valid package directory.
     *
     * A valid package directory must be a directory containing the three
     * required metadata files: `+COMMENT`, `+CONTENTS`, and `+DESC`.
     */
    fn is_valid_pkgdir(&self, pkgdir: &Path) -> bool {
        if !pkgdir.is_dir() {
            return false;
        }
        pkgdir.join(Entry::Comment.to_filename()).exists()
            && pkgdir.join(Entry::Contents.to_filename()).exists()
            && pkgdir.join(Entry::Desc.to_filename()).exists()
    }
}

impl InstalledPackage {
    /**
     * Return a new empty `InstalledPackage` container.
     */
    #[must_use]
    pub fn new() -> InstalledPackage {
        InstalledPackage {
            path: PathBuf::new(),
            pkgbase: String::new(),
            pkgname: String::new(),
            pkgversion: String::new(),
        }
    }

    /**
     * Package basename (no version information).
     */
    #[must_use]
    pub fn pkgbase(&self) -> &str {
        &self.pkgbase
    }

    /**
     * Full package name including version.
     */
    #[must_use]
    pub fn pkgname(&self) -> &str {
        &self.pkgname
    }

    /**
     * Package version.
     */
    #[must_use]
    pub fn pkgversion(&self) -> &str {
        &self.pkgversion
    }

    /**
     * Return the file system path to this package's metadata directory.
     */
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /**
     * Read an optional metadata file.
     *
     * Returns `Ok(Some(content))` if file exists and was read,
     * `Ok(None)` if file does not exist, or `Err(e)` for other I/O errors.
     */
    fn read_file(&self, entry: Entry) -> io::Result<Option<String>> {
        match fs::read_to_string(self.path.join(entry.to_filename())) {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

impl Default for InstalledPackage {
    fn default() -> Self {
        Self::new()
    }
}

impl FileRead for InstalledPackage {
    fn pkgname(&self) -> &str {
        &self.pkgname
    }

    fn comment(&self) -> io::Result<String> {
        fs::read_to_string(self.path.join(Entry::Comment.to_filename()))
            .map(|s| s.trim().to_string())
    }

    fn contents(&self) -> io::Result<String> {
        fs::read_to_string(self.path.join(Entry::Contents.to_filename()))
    }

    fn desc(&self) -> io::Result<String> {
        fs::read_to_string(self.path.join(Entry::Desc.to_filename()))
    }

    fn build_info(&self) -> io::Result<Option<String>> {
        self.read_file(Entry::BuildInfo)
    }

    fn build_version(&self) -> io::Result<Option<String>> {
        self.read_file(Entry::BuildVersion)
    }

    fn deinstall(&self) -> io::Result<Option<String>> {
        self.read_file(Entry::DeInstall)
    }

    fn display(&self) -> io::Result<Option<String>> {
        self.read_file(Entry::Display)
    }

    fn install(&self) -> io::Result<Option<String>> {
        self.read_file(Entry::Install)
    }

    fn installed_info(&self) -> io::Result<Option<String>> {
        self.read_file(Entry::InstalledInfo)
    }

    fn mtree_dirs(&self) -> io::Result<Option<String>> {
        self.read_file(Entry::MtreeDirs)
    }

    fn preserve(&self) -> io::Result<Option<String>> {
        self.read_file(Entry::Preserve)
    }

    fn required_by(&self) -> io::Result<Option<String>> {
        self.read_file(Entry::RequiredBy)
    }

    fn size_all(&self) -> io::Result<Option<String>> {
        self.read_file(Entry::SizeAll)
    }

    fn size_pkg(&self) -> io::Result<Option<String>> {
        self.read_file(Entry::SizePkg)
    }
}

/**
 * An iterator over the entries of a package database, returning either a
 * valid `InstalledPackage` handle, an `io::Error`, or None.
 */
impl Iterator for PkgDB {
    type Item = io::Result<InstalledPackage>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.dbtype {
            DBType::Files => loop {
                let readdir = self.readdir.as_mut()?;
                let entry = match readdir.next()? {
                    Ok(entry) => entry,
                    Err(e) => return Some(Err(e)),
                };

                let path = entry.path();
                if !self.is_valid_pkgdir(&path) {
                    continue;
                }

                let filename = entry.file_name();
                let dirname = match filename.to_str() {
                    Some(name) => name,
                    None => {
                        return Some(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Could not parse package directory name",
                        )));
                    }
                };

                let (pkgbase, pkgversion) = match dirname.rsplit_once('-') {
                    Some((base, version)) => (base, version),
                    None => {
                        return Some(Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("Invalid package name: {}", dirname),
                        )));
                    }
                };

                return Some(Ok(InstalledPackage {
                    path,
                    pkgname: dirname.to_string(),
                    pkgbase: pkgbase.to_string(),
                    pkgversion: pkgversion.to_string(),
                }));
            },
            DBType::Database => None,
        }
    }
}
