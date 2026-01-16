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
 * Module supporting the package database.  WIP.
 */
use crate::metadata::{Entry, MetadataReader};
use std::fs;
use std::fs::ReadDir;
use std::io;
use std::path::{Path, PathBuf};

/**
 * Type of pkgdb.  Currently supported formats are `Files` for the legacy
 * directory of `+*` files, and `Database` for a sqlite3 backend.
 */
#[derive(Debug)]
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
pub struct Package {
    path: PathBuf,
    pkgbase: String,
    pkgname: String,
    pkgversion: String,
}

impl PkgDB {
    /**
     * Open an existing `PkgDB`.
     */
    pub fn open(p: &std::path::Path) -> Result<PkgDB, io::Error> {
        let mut db = PkgDB {
            dbtype: DBType::Files,
            path: PathBuf::new(),
            readdir: None,
        };

        /*
         * Nothing fancy for now, assume that what the user passed is valid,
         * we'll find out soon enough if it isn't.
         */
        if p.is_dir() {
            db.dbtype = DBType::Files;
            db.path = PathBuf::from(p);
            db.readdir = Some(fs::read_dir(&db.path).expect("fail"));
        } else if p.is_file() {
            db.dbtype = DBType::Database;
            db.path = PathBuf::from(p);
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Invalid pkgdb",
            ));
        }

        Ok(db)
    }

    /**
     * Ensure package directory is valid.  Only for `DBType::Files`.
     */
    fn is_valid_pkgdir(&self, pkgdir: &Path) -> bool {
        /*
         * Skip files such as pkg-vulnerabilities and pkgdb.byfile.db, we're
         * only interested in directories.
         */
        if pkgdir.is_file() {
            return false;
        }

        /*
         * These 3 metadata files are mandatory.
         */
        let reqd = vec![
            Entry::Comment.to_filename(),
            Entry::Contents.to_filename(),
            Entry::Desc.to_filename(),
        ];
        for file in reqd {
            if !pkgdir.join(file).exists() {
                return false;
            }
        }

        true
    }
}

impl Package {
    /**
     * Return a new empty `Package` container.
     */
    pub fn new() -> Package {
        Package {
            path: PathBuf::new(),
            pkgbase: String::new(),
            pkgname: String::new(),
            pkgversion: String::new(),
        }
    }

    /**
     * Package basename (no version information).
     */
    pub fn pkgbase(&self) -> &str {
        &self.pkgbase
    }

    /**
     * Full package name including version.
     */
    pub fn pkgname(&self) -> &str {
        &self.pkgname
    }

    /**
     * Package version.
     */
    pub fn pkgversion(&self) -> &str {
        &self.pkgversion
    }

    /**
     * Read a metadata file, returning its contents.
     */
    fn read_file(&self, entry: Entry) -> io::Result<String> {
        fs::read_to_string(self.path.join(entry.to_filename()))
    }

    /**
     * Package comment (`+COMMENT`).  Single line description.
     */
    pub fn comment(&self) -> io::Result<String> {
        self.read_file(Entry::Comment).map(|s| s.trim().to_string())
    }

    /**
     * Package contents (`+CONTENTS`).  The packing list.
     */
    pub fn contents(&self) -> io::Result<String> {
        self.read_file(Entry::Contents)
    }

    /**
     * Package description (`+DESC`).  Multi-line description.
     */
    pub fn desc(&self) -> io::Result<String> {
        self.read_file(Entry::Desc)
    }

    /**
     * Build information (`+BUILD_INFO`).
     */
    pub fn build_info(&self) -> Option<String> {
        self.read_file(Entry::BuildInfo).ok()
    }

    /**
     * Build version (`+BUILD_VERSION`).
     */
    pub fn build_version(&self) -> Option<String> {
        self.read_file(Entry::BuildVersion).ok()
    }

    /**
     * Deinstall script (`+DEINSTALL`).
     */
    pub fn deinstall(&self) -> Option<String> {
        self.read_file(Entry::DeInstall).ok()
    }

    /**
     * Display file (`+DISPLAY`).
     */
    pub fn display(&self) -> Option<String> {
        self.read_file(Entry::Display).ok()
    }

    /**
     * Install script (`+INSTALL`).
     */
    pub fn install(&self) -> Option<String> {
        self.read_file(Entry::Install).ok()
    }

    /**
     * Installed info (`+INSTALLED_INFO`).
     */
    pub fn installed_info(&self) -> Option<String> {
        self.read_file(Entry::InstalledInfo).ok()
    }

    /**
     * Mtree dirs (`+MTREE_DIRS`).
     */
    pub fn mtree_dirs(&self) -> Option<String> {
        self.read_file(Entry::MtreeDirs).ok()
    }

    /**
     * Preserve file (`+PRESERVE`).
     */
    pub fn preserve(&self) -> Option<String> {
        self.read_file(Entry::Preserve).ok()
    }

    /**
     * Required by (`+REQUIRED_BY`).
     */
    pub fn required_by(&self) -> Option<String> {
        self.read_file(Entry::RequiredBy).ok()
    }

    /**
     * Total size including dependencies (`+SIZE_ALL`).
     */
    pub fn size_all(&self) -> Option<String> {
        self.read_file(Entry::SizeAll).ok()
    }

    /**
     * Package size (`+SIZE_PKG`).
     */
    pub fn size_pkg(&self) -> Option<String> {
        self.read_file(Entry::SizePkg).ok()
    }
}

impl Default for Package {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataReader for Package {
    fn pkgname(&self) -> &str {
        &self.pkgname
    }

    fn comment(&self) -> io::Result<String> {
        self.read_file(Entry::Comment).map(|s| s.trim().to_string())
    }

    fn contents(&self) -> io::Result<String> {
        self.read_file(Entry::Contents)
    }

    fn desc(&self) -> io::Result<String> {
        self.read_file(Entry::Desc)
    }

    fn build_info(&self) -> Option<String> {
        self.read_file(Entry::BuildInfo).ok()
    }

    fn build_version(&self) -> Option<String> {
        self.read_file(Entry::BuildVersion).ok()
    }

    fn deinstall(&self) -> Option<String> {
        self.read_file(Entry::DeInstall).ok()
    }

    fn display(&self) -> Option<String> {
        self.read_file(Entry::Display).ok()
    }

    fn install(&self) -> Option<String> {
        self.read_file(Entry::Install).ok()
    }

    fn installed_info(&self) -> Option<String> {
        self.read_file(Entry::InstalledInfo).ok()
    }

    fn mtree_dirs(&self) -> Option<String> {
        self.read_file(Entry::MtreeDirs).ok()
    }

    fn preserve(&self) -> Option<String> {
        self.read_file(Entry::Preserve).ok()
    }

    fn required_by(&self) -> Option<String> {
        self.read_file(Entry::RequiredBy).ok()
    }

    fn size_all(&self) -> Option<String> {
        self.read_file(Entry::SizeAll).ok()
    }

    fn size_pkg(&self) -> Option<String> {
        self.read_file(Entry::SizePkg).ok()
    }
}

/**
 * An iterator over the entries of a package database, returning either a
 * valid `Package` handle, an ``io::Error`, or None.
 */
impl Iterator for PkgDB {
    type Item = io::Result<Package>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut package = Package::new();

        match self.dbtype {
            DBType::Files => loop {
                match self.readdir.as_mut().expect("Bad pkgdb read").next()? {
                    Ok(dir) => {
                        if !self.is_valid_pkgdir(&dir.path()) {
                            continue;
                        }
                        match dir.file_name().to_str() {
                            Some(p) => {
                                let v: Vec<&str> = p.rsplitn(2, '-').collect();
                                package.path = dir.path();
                                package.pkgname = p.to_string();
                                package.pkgbase = v[0].to_string();
                                package.pkgversion = v[1].to_string();
                                return Some(Ok(package));
                            }
                            _ => {
                                return Some(Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    "Could not parse package directory",
                                )));
                            }
                        };
                    }
                    _ => return None,
                };
            },
            DBType::Database => None,
        }
    }
}
