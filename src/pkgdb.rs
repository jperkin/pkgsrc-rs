/*
 * Copyright (c) 2019 Jonathan Perkin <jonathan@perkin.org.uk>
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

use crate::metadata::MetadataEntry;
use crate::summary::Summary;
use std::fs;
use std::fs::ReadDir;
use std::io;
use std::path::PathBuf;

/**
 * Type of pkgdb.  Currently supported formats are `Files` for the legacy
 * directory of `+*` files, and `Database` for a sqlite3 backend.
 */
#[derive(Debug)]
pub enum DBType {
    Files,
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
#[derive(Debug, Default)]
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
    fn is_valid_pkgdir(&self, pkgdir: &std::path::PathBuf) -> bool {
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
            MetadataEntry::Comment.to_filename(),
            MetadataEntry::Contents.to_filename(),
            MetadataEntry::Desc.to_filename(),
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
        let package: Package = Default::default();
        package
    }

    /**
     * Package basename (no version information).
     */
    pub fn pkgbase(&self) -> &String {
        &self.pkgbase
    }

    /**
     * Full package name including version.
     */
    pub fn pkgname(&self) -> &String {
        &self.pkgname
    }

    /**
     * Package version.
     */
    pub fn pkgversion(&self) -> &String {
        &self.pkgversion
    }

    /**
     * Read metadata for a package.  Return a string representation of the
     * complete metadata entry.
     *
     * XXX: Only supports Files for now.
     */
    pub fn read_metadata(
        &self,
        mentry: MetadataEntry,
    ) -> Result<String, io::Error> {
        let fname = self.path.as_path().join(mentry.to_filename());
        fs::read_to_string(fname)
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
                                )))
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
