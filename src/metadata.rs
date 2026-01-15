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
 * metadata.rs - parse package metadata from "+*" files
 */

/**
 * Parse metadata contained in `+*` files in a package archive.
 *
 * ## Examples
 *
 * ```no_run
 * use flate2::read::GzDecoder;
 * use pkgsrc::{Metadata,MetadataEntry};
 * use std::fs::File;
 * use std::io::Read;
 * use tar::Archive;
 *
 * fn main() -> Result<(), std::io::Error> {
 *     let pkg = File::open("package-1.0.tgz")?;
 *     let mut archive = Archive::new(GzDecoder::new(pkg));
 *     let mut metadata = Metadata::new();
 *
 *     for file in archive.entries()? {
 *         let mut file = file?;
 *         let fname = String::from(file.header().path()?.to_str().unwrap());
 *         let mut s = String::new();
 *
 *         if let Some(entry) = MetadataEntry::from_filename(fname.as_str()) {
 *             file.read_to_string(&mut s)?;
 *             if let Err(e) = metadata.read_metadata(entry, &s) {
 *                 panic!("Bad metadata: {}", e);
 *             }
 *         }
 *     }
 *
 *     if let Err(e) = metadata.is_valid() {
 *         panic!("Bad metadata: {}", e);
 *     }
 *
 *     println!("Information for package-1.0");
 *     println!("Comment: {}", metadata.comment());
 *     println!("Files:");
 *     for line in metadata.contents().lines() {
 *         if !line.starts_with('@') && !line.starts_with('+') {
 *             println!("{}", line);
 *         }
 *     }
 *
 *     Ok(())
 * }
 * ```
 */
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Metadata {
    build_info: Option<Vec<String>>,
    build_version: Option<Vec<String>>,
    comment: String,
    contents: String,
    deinstall: Option<String>,
    desc: String,
    display: Option<String>,
    install: Option<String>,
    installed_info: Option<Vec<String>>,
    mtree_dirs: Option<Vec<String>>,
    preserve: Option<Vec<String>>,
    required_by: Option<Vec<String>>,
    size_all: Option<i64>,
    size_pkg: Option<i64>,
}

/**
 * Type of Metadata entry.
 *
 * Package metadata stored either in a package archive or inside a package
 * entry in a `PkgDB::DBType::Files` package database is contained in various
 * files prefixed with `+`.
 *
 * This enum supports all of those filenames and avoids having to hardcode
 * their values.  It supports converting to and from the filename or enum.
 *
 * ## Example
 *
 * ```
 * use pkgsrc::MetadataEntry;
 *
 * let e = MetadataEntry::Desc;
 *
 * /*
 *  * Validate that the `Desc` entry matches our expected filename.
 *  */
 * assert_eq!(e.to_filename(), "+DESC");
 * assert_eq!(MetadataEntry::from_filename("+DESC"), Some(e));
 *
 * /*
 *  * This is not a known +FILE
 *  */
 * assert_eq!(MetadataEntry::from_filename("+BADFILE"), None);
 * ```
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MetadataEntry {
    /**
     * Optional package build information stored in `+BUILD_INFO`.
     */
    BuildInfo,
    /**
     * Optional version information (usually CVS Id's) for the files used to
     * create the package stored in `+BUILD_VERSION`.
     */
    BuildVersion,
    /**
     * Single line description of the package stored in `+COMMENT`.
     */
    Comment,
    /**
     * Packing list contents, also known as the `packlist` or `PLIST`, stored
     * in `+CONTENTS`.
     */
    Contents,
    /**
     * Optional script executed upon deinstall, stored in `+DEINSTALL`.
     */
    DeInstall,
    /**
     * Multi-line description of the package stored in `+DESC`.
     */
    Desc,
    /**
     * Optional file, also known as `MESSAGE`, to be shown during package
     * install or deinstall, stored in `+DISPLAY`.
     */
    Display,
    /**
     * Optional script executed upon install, stored in `+INSTALL`.
     */
    Install,
    /**
     * Variables set by this package, currently only `automatic=yes` being
     * supported, stored in `+INSTALLED_INFO`.
     */
    InstalledInfo,
    /**
     * Obsolete file used to pre-create directories prior to a package install,
     * stored in `+MTREE_DIRS`.
     */
    MtreeDirs,
    /**
     * Optional marker that this package should not be deleted under normal
     * circumstances, stored in `+PRESERVE`.
     */
    Preserve,
    /**
     * Optional list of packages that are reverse dependencies of (i.e. depend
     * upon) this package, stored in `+REQUIRED_BY`.
     */
    RequiredBy,
    /**
     * Optional size of this package plus all of its dependencies, stored in
     * `+SIZE_ALL`.
     */
    SizeAll,
    /**
     * Optional size of this package, stored in `+SIZE_ALL`.
     */
    SizePkg,
}

impl Metadata {
    /**
     * Return a new empty `Metadata` container.
     */
    pub fn new() -> Metadata {
        let metadata: Metadata = Default::default();
        metadata
    }

    /**
     * Return the optional `+BUILD_INFO` file as a vector of strings.
     */
    pub fn build_info(&self) -> &Option<Vec<String>> {
        &self.build_info
    }

    /**
     * Return the optional `+BUILD_VERSION` file as a vector of strings.
     */
    pub fn build_version(&self) -> &Option<Vec<String>> {
        &self.build_version
    }

    /**
     * Return the mandatory `+COMMENT` file as a string.  This should be a
     * single line.
     */
    pub fn comment(&self) -> &String {
        &self.comment
    }

    /**
     * Return the mandatory `+CONTENTS` (i.e. packlist or PLIST) file as a
     * complete string.
     */
    pub fn contents(&self) -> &String {
        &self.contents
    }

    /**
     * Return the optional `+DEINSTALL` script as complete string.
     */
    pub fn deinstall(&self) -> &Option<String> {
        &self.deinstall
    }

    /**
     * Return the mandatory `+DESC` file as a complete string.
     */
    pub fn desc(&self) -> &String {
        &self.desc
    }

    /**
     * Return the optional `+DISPLAY` (i.e. MESSAGE) file as a complete string.
     */
    pub fn display(&self) -> &Option<String> {
        &self.display
    }

    /**
     * Return the optional `+INSTALL` script as a complete string.
     */
    pub fn install(&self) -> &Option<String> {
        &self.install
    }

    /**
     * Return the optional `+INSTALLED_INFO` file as a vector of strings.
     */
    pub fn installed_info(&self) -> &Option<Vec<String>> {
        &self.installed_info
    }

    /**
     * Return the optional `+MTREE_DIRS` file (obsolete) as a vector of strings.
     */
    pub fn mtree_dirs(&self) -> &Option<Vec<String>> {
        &self.mtree_dirs
    }

    /**
     * Return the optional `+PRESERVE` file as a vector of strings.
     */
    pub fn preserve(&self) -> &Option<Vec<String>> {
        &self.preserve
    }

    /**
     * Return the optional `+REQUIRED_BY` file as a vector of strings.
     */
    pub fn required_by(&self) -> &Option<Vec<String>> {
        &self.required_by
    }

    /**
     * Return the optional `+SIZE_ALL` file as an i64.
     */
    pub fn size_all(&self) -> &Option<i64> {
        &self.size_all
    }

    /**
     * Return the optional `+SIZE_PKG` file as an i64.
     */
    pub fn size_pkg(&self) -> &Option<i64> {
        &self.size_pkg
    }

    /**
     * Read in a metadata file `fname` and its `value` as strings, populating
     * the associated Metadata struct.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::{Metadata, MetadataEntry};
     *
     * let mut m = Metadata::new();
     * m.read_metadata(MetadataEntry::Comment, "This is a package comment");
     * ```
     */
    pub fn read_metadata(
        &mut self,
        entry: MetadataEntry,
        value: &str,
    ) -> Result<(), &'static str> {
        /*
         * Lazily compute values only when needed to avoid unnecessary
         * allocations. For most metadata, trim() is appropriate.
         * For +DESC specifically, we only strip trailing newlines to
         * preserve leading whitespace on description lines (required
         * for pkg_info compatibility).
         */

        // Helper to create trimmed string - only allocates when called
        let make_string = || value.trim().to_string();

        // Helper to create vec of lines - only allocates when called
        let make_vec = || {
            value
                .trim()
                .lines()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        };

        match entry {
            MetadataEntry::BuildInfo => self.build_info = Some(make_vec()),
            MetadataEntry::BuildVersion => {
                self.build_version = Some(make_vec())
            }
            MetadataEntry::Comment => self.comment.push_str(value.trim()),
            MetadataEntry::Contents => self.contents.push_str(value.trim()),
            MetadataEntry::DeInstall => self.deinstall = Some(make_string()),
            MetadataEntry::Desc => {
                // Only strip trailing newlines, preserve leading whitespace
                self.desc.push_str(value.trim_end_matches('\n'));
            }
            MetadataEntry::Display => self.display = Some(make_string()),
            MetadataEntry::Install => self.install = Some(make_string()),
            MetadataEntry::InstalledInfo => {
                self.installed_info = Some(make_vec())
            }
            MetadataEntry::MtreeDirs => self.mtree_dirs = Some(make_vec()),
            MetadataEntry::Preserve => self.preserve = Some(make_vec()),
            MetadataEntry::RequiredBy => self.required_by = Some(make_vec()),
            MetadataEntry::SizeAll => {
                self.size_all = Some(value.trim().parse::<i64>().unwrap())
            }
            MetadataEntry::SizePkg => {
                self.size_pkg = Some(value.trim().parse::<i64>().unwrap())
            }
        }

        Ok(())
    }

    /**
     * Ensure the required files (`+COMMENT`, `+CONTENTS`, and `+DESC`) have
     * been registered, indicating that this is a valid package.
     */
    pub fn is_valid(&self) -> Result<(), &'static str> {
        if self.comment.is_empty() {
            return Err("Missing or empty +COMMENT");
        }
        if self.contents.is_empty() {
            return Err("Missing or empty +CONTENTS");
        }
        if self.desc.is_empty() {
            return Err("Missing or empty +DESC");
        }
        Ok(())
    }
}

impl MetadataEntry {
    /**
     * Return filename for the associated `MetadataEntry` type.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::MetadataEntry;
     *
     * let e = MetadataEntry::Contents;
     * assert_eq!(e.to_filename(), "+CONTENTS");
     * ```
     */
    pub fn to_filename(&self) -> &str {
        match self {
            MetadataEntry::BuildInfo => "+BUILD_INFO",
            MetadataEntry::BuildVersion => "+BUILD_VERSION",
            MetadataEntry::Comment => "+COMMENT",
            MetadataEntry::Contents => "+CONTENTS",
            MetadataEntry::DeInstall => "+DEINSTALL",
            MetadataEntry::Desc => "+DESC",
            MetadataEntry::Display => "+DISPLAY",
            MetadataEntry::Install => "+INSTALL",
            MetadataEntry::InstalledInfo => "+INSTALLED_INFO",
            MetadataEntry::MtreeDirs => "+MTREE_DIRS",
            MetadataEntry::Preserve => "+PRESERVE",
            MetadataEntry::RequiredBy => "+REQUIRED_BY",
            MetadataEntry::SizeAll => "+SIZE_ALL",
            MetadataEntry::SizePkg => "+SIZE_PKG",
        }
    }
    /**
     * Return `MetadataEntry` enum in an Option for requested file.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::MetadataEntry;
     *
     * assert_eq!(MetadataEntry::from_filename("+CONTENTS"),
     *            Some(MetadataEntry::Contents));
     * assert_eq!(MetadataEntry::from_filename("+BADFILE"), None);
     * ```
     */
    pub fn from_filename(file: &str) -> Option<MetadataEntry> {
        match file {
            "+BUILD_INFO" => Some(MetadataEntry::BuildInfo),
            "+BUILD_VERSION" => Some(MetadataEntry::BuildVersion),
            "+COMMENT" => Some(MetadataEntry::Comment),
            "+CONTENTS" => Some(MetadataEntry::Contents),
            "+DEINSTALL" => Some(MetadataEntry::DeInstall),
            "+DESC" => Some(MetadataEntry::Desc),
            "+DISPLAY" => Some(MetadataEntry::Display),
            "+INSTALL" => Some(MetadataEntry::Install),
            "+INSTALLED_INFO" => Some(MetadataEntry::InstalledInfo),
            "+MTREE_DIRS" => Some(MetadataEntry::MtreeDirs),
            "+PRESERVE" => Some(MetadataEntry::Preserve),
            "+REQUIRED_BY" => Some(MetadataEntry::RequiredBy),
            "+SIZE_ALL" => Some(MetadataEntry::SizeAll),
            "+SIZE_PKG" => Some(MetadataEntry::SizePkg),
            _ => None,
        }
    }
}
