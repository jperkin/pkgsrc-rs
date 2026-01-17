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

/*!
 * Package metadata from `+*` files.
 *
 * This module handles the metadata files found in pkgsrc package archives
 * (files starting with `+` such as `+COMMENT`, `+DESC`, `+CONTENTS`, etc.).
 *
 * # Example
 *
 * ```no_run
 * use flate2::read::GzDecoder;
 * use pkgsrc::metadata::{Entry, Error, Metadata};
 * use std::fs::File;
 * use std::io::Read;
 * use tar::Archive;
 *
 * fn main() -> Result<(), Error> {
 *     let pkg = File::open("package-1.0.tgz")?;
 *     let mut archive = Archive::new(GzDecoder::new(pkg));
 *     let mut metadata = Metadata::new();
 *
 *     for file in archive.entries()? {
 *         let mut file = file?;
 *         let fname = String::from(file.header().path()?.to_str().unwrap());
 *         let mut s = String::new();
 *
 *         if let Some(entry) = Entry::from_filename(fname.as_str()) {
 *             file.read_to_string(&mut s)?;
 *             metadata.read_metadata(entry, &s)?;
 *         }
 *     }
 *
 *     metadata.validate()?;
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

use std::fmt;
use std::io;
use std::num::ParseIntError;
use std::str::FromStr;
use thiserror::Error;

/**
 * A metadata parsing or validation error.
 */
#[derive(Debug, Error)]
pub enum Error {
    /**
     * A required metadata field is missing or empty.
     */
    #[error("Missing or empty {0}")]
    MissingRequired(&'static str),
    /**
     * A metadata field contains an invalid value.
     */
    #[error("Invalid value in {field}: {source}")]
    InvalidValue {
        /** The name of the field that contained the invalid value. */
        field: &'static str,
        /** The underlying parse error. */
        #[source]
        source: ParseIntError,
    },
    /**
     * An unknown metadata entry filename was provided.
     */
    #[error("Unknown metadata entry: {0}")]
    UnknownEntry(String),
    /**
     * An I/O error occurred reading metadata.
     */
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

/**
 * Trait for types that provide package metadata.
 *
 * This trait abstracts over different package sources (binary archives,
 * installed packages) providing a unified interface for accessing metadata.
 *
 * # Return Types
 *
 * - Required metadata (`comment`, `contents`, `desc`) returns `io::Result<String>`
 *   since these files must exist for a valid package.
 *
 * - Optional metadata returns `io::Result<Option<String>>`:
 *   - `Ok(Some(content))` - File exists and was read successfully
 *   - `Ok(None)` - File does not exist (this is normal for optional metadata)
 *   - `Err(e)` - An I/O error occurred (permission denied, disk failure, etc.)
 *
 * This design ensures that real I/O errors are propagated to callers rather
 * than being silently swallowed as "file not found".
 */
pub trait FileRead {
    /** Package name including version (e.g., "foo-1.0"). */
    fn pkgname(&self) -> &str;

    /** Package comment (`+COMMENT`). Single line description. */
    fn comment(&self) -> io::Result<String>;

    /** Package contents (`+CONTENTS`). The packing list. */
    fn contents(&self) -> io::Result<String>;

    /** Package description (`+DESC`). Multi-line description. */
    fn desc(&self) -> io::Result<String>;

    /** Build information (`+BUILD_INFO`). */
    fn build_info(&self) -> io::Result<Option<String>>;

    /** Build version (`+BUILD_VERSION`). */
    fn build_version(&self) -> io::Result<Option<String>>;

    /** Deinstall script (`+DEINSTALL`). */
    fn deinstall(&self) -> io::Result<Option<String>>;

    /** Display file (`+DISPLAY`). */
    fn display(&self) -> io::Result<Option<String>>;

    /** Install script (`+INSTALL`). */
    fn install(&self) -> io::Result<Option<String>>;

    /** Installed info (`+INSTALLED_INFO`). */
    fn installed_info(&self) -> io::Result<Option<String>>;

    /** Mtree dirs (`+MTREE_DIRS`). */
    fn mtree_dirs(&self) -> io::Result<Option<String>>;

    /** Preserve file (`+PRESERVE`). */
    fn preserve(&self) -> io::Result<Option<String>>;

    /** Required by (`+REQUIRED_BY`). */
    fn required_by(&self) -> io::Result<Option<String>>;

    /** Total size including dependencies (`+SIZE_ALL`). */
    fn size_all(&self) -> io::Result<Option<String>>;

    /** Package size (`+SIZE_PKG`). */
    fn size_pkg(&self) -> io::Result<Option<String>>;
}

/**
 * Parsed metadata from `+*` files in a package archive.
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
    size_all: Option<u64>,
    size_pkg: Option<u64>,
}

/**
 * Type of metadata entry (`+COMMENT`, `+DESC`, etc.).
 *
 * Package metadata is stored in files prefixed with `+`. This enum
 * represents all known metadata file types and provides conversion
 * to/from filenames.
 *
 * # Example
 *
 * ```
 * use pkgsrc::metadata::Entry;
 * use std::str::FromStr;
 *
 * let e = Entry::Desc;
 * assert_eq!(e.to_filename(), "+DESC");
 * assert_eq!(Entry::from_str("+DESC").unwrap(), e);
 * assert!(Entry::from_str("+BADFILE").is_err());
 * ```
 */
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Entry {
    /**
     * Package build information (`+BUILD_INFO`).
     */
    BuildInfo,
    /**
     * Version info for files used to create the package (`+BUILD_VERSION`).
     */
    BuildVersion,
    /**
     * Single line package description (`+COMMENT`).
     */
    Comment,
    /**
     * Packing list / PLIST (`+CONTENTS`).
     */
    Contents,
    /**
     * Deinstall script (`+DEINSTALL`).
     */
    DeInstall,
    /**
     * Multi-line package description (`+DESC`).
     */
    Desc,
    /**
     * Message shown during install/deinstall (`+DISPLAY`).
     */
    Display,
    /**
     * Install script (`+INSTALL`).
     */
    Install,
    /**
     * Package variables like `automatic=yes` (`+INSTALLED_INFO`).
     */
    InstalledInfo,
    /**
     * Obsolete directory pre-creation file (`+MTREE_DIRS`).
     */
    MtreeDirs,
    /**
     * Marker that package should not be deleted (`+PRESERVE`).
     */
    Preserve,
    /**
     * Packages that depend on this one (`+REQUIRED_BY`).
     */
    RequiredBy,
    /**
     * Size of package plus dependencies (`+SIZE_ALL`).
     */
    SizeAll,
    /**
     * Size of package (`+SIZE_PKG`).
     */
    SizePkg,
}

impl Metadata {
    /**
     * Create a new empty metadata container.
     */
    #[must_use]
    pub fn new() -> Metadata {
        Metadata::default()
    }

    /**
     * Return the `+BUILD_INFO` content.
     */
    #[must_use]
    pub fn build_info(&self) -> Option<&[String]> {
        self.build_info.as_deref()
    }

    /**
     * Return the `+BUILD_VERSION` content.
     */
    #[must_use]
    pub fn build_version(&self) -> Option<&[String]> {
        self.build_version.as_deref()
    }

    /**
     * Return the `+COMMENT` content (single line description).
     */
    #[must_use]
    pub fn comment(&self) -> &str {
        &self.comment
    }

    /**
     * Return the `+CONTENTS` (packing list).
     */
    #[must_use]
    pub fn contents(&self) -> &str {
        &self.contents
    }

    /**
     * Return the `+DEINSTALL` script.
     */
    #[must_use]
    pub fn deinstall(&self) -> Option<&str> {
        self.deinstall.as_deref()
    }

    /**
     * Return the `+DESC` content (multi-line description).
     */
    #[must_use]
    pub fn desc(&self) -> &str {
        &self.desc
    }

    /**
     * Return the `+DISPLAY` message.
     */
    #[must_use]
    pub fn display(&self) -> Option<&str> {
        self.display.as_deref()
    }

    /**
     * Return the `+INSTALL` script.
     */
    #[must_use]
    pub fn install(&self) -> Option<&str> {
        self.install.as_deref()
    }

    /**
     * Return the `+INSTALLED_INFO` content.
     */
    #[must_use]
    pub fn installed_info(&self) -> Option<&[String]> {
        self.installed_info.as_deref()
    }

    /**
     * Return the `+MTREE_DIRS` content (obsolete).
     */
    #[must_use]
    pub fn mtree_dirs(&self) -> Option<&[String]> {
        self.mtree_dirs.as_deref()
    }

    /**
     * Return the `+PRESERVE` content.
     */
    #[must_use]
    pub fn preserve(&self) -> Option<&[String]> {
        self.preserve.as_deref()
    }

    /**
     * Return the `+REQUIRED_BY` content.
     */
    #[must_use]
    pub fn required_by(&self) -> Option<&[String]> {
        self.required_by.as_deref()
    }

    /**
     * Return the `+SIZE_ALL` value.
     */
    #[must_use]
    pub fn size_all(&self) -> Option<u64> {
        self.size_all
    }

    /**
     * Return the `+SIZE_PKG` value.
     */
    #[must_use]
    pub fn size_pkg(&self) -> Option<u64> {
        self.size_pkg
    }

    /**
     * Read a metadata file into this container.
     *
     * # Errors
     *
     * Returns [`Error::InvalidValue`] if `+SIZE_ALL` or `+SIZE_PKG`
     * contain invalid integer values.
     */
    pub fn read_metadata(
        &mut self,
        entry: Entry,
        value: &str,
    ) -> Result<(), Error> {
        let make_string = || value.trim().to_string();
        let make_vec = || {
            value
                .trim()
                .lines()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        };

        match entry {
            Entry::BuildInfo => self.build_info = Some(make_vec()),
            Entry::BuildVersion => self.build_version = Some(make_vec()),
            Entry::Comment => self.comment.push_str(value.trim()),
            Entry::Contents => self.contents.push_str(value.trim()),
            Entry::DeInstall => self.deinstall = Some(make_string()),
            Entry::Desc => {
                self.desc.push_str(value.trim_end_matches('\n'));
            }
            Entry::Display => self.display = Some(make_string()),
            Entry::Install => self.install = Some(make_string()),
            Entry::InstalledInfo => self.installed_info = Some(make_vec()),
            Entry::MtreeDirs => self.mtree_dirs = Some(make_vec()),
            Entry::Preserve => self.preserve = Some(make_vec()),
            Entry::RequiredBy => self.required_by = Some(make_vec()),
            Entry::SizeAll => {
                self.size_all =
                    Some(value.trim().parse::<u64>().map_err(|e| {
                        Error::InvalidValue {
                            field: "+SIZE_ALL",
                            source: e,
                        }
                    })?);
            }
            Entry::SizePkg => {
                self.size_pkg =
                    Some(value.trim().parse::<u64>().map_err(|e| {
                        Error::InvalidValue {
                            field: "+SIZE_PKG",
                            source: e,
                        }
                    })?);
            }
        }

        Ok(())
    }

    /**
     * Validate that required fields are populated.
     *
     * The required fields are `+COMMENT`, `+CONTENTS`, and `+DESC`.
     *
     * # Errors
     *
     * Returns [`Error::MissingRequired`] if any required field is
     * missing or empty.
     */
    pub fn validate(&self) -> Result<(), Error> {
        if self.comment.is_empty() {
            return Err(Error::MissingRequired("+COMMENT"));
        }
        if self.contents.is_empty() {
            return Err(Error::MissingRequired("+CONTENTS"));
        }
        if self.desc.is_empty() {
            return Err(Error::MissingRequired("+DESC"));
        }
        Ok(())
    }

    /**
     * Return whether all required fields are populated.
     */
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.comment.is_empty()
            && !self.contents.is_empty()
            && !self.desc.is_empty()
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_filename())
    }
}

impl FromStr for Entry {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_filename(s).ok_or_else(|| Error::UnknownEntry(s.to_string()))
    }
}

impl Entry {
    /**
     * Return the filename for this entry type.
     */
    #[must_use]
    pub const fn to_filename(&self) -> &'static str {
        match self {
            Entry::BuildInfo => "+BUILD_INFO",
            Entry::BuildVersion => "+BUILD_VERSION",
            Entry::Comment => "+COMMENT",
            Entry::Contents => "+CONTENTS",
            Entry::DeInstall => "+DEINSTALL",
            Entry::Desc => "+DESC",
            Entry::Display => "+DISPLAY",
            Entry::Install => "+INSTALL",
            Entry::InstalledInfo => "+INSTALLED_INFO",
            Entry::MtreeDirs => "+MTREE_DIRS",
            Entry::Preserve => "+PRESERVE",
            Entry::RequiredBy => "+REQUIRED_BY",
            Entry::SizeAll => "+SIZE_ALL",
            Entry::SizePkg => "+SIZE_PKG",
        }
    }

    /**
     * Parse a filename into an entry type.
     *
     * Returns `None` for unknown filenames. For fallible conversion
     * that returns an error, use [`FromStr`].
     */
    #[must_use]
    pub fn from_filename(file: &str) -> Option<Entry> {
        match file {
            "+BUILD_INFO" => Some(Entry::BuildInfo),
            "+BUILD_VERSION" => Some(Entry::BuildVersion),
            "+COMMENT" => Some(Entry::Comment),
            "+CONTENTS" => Some(Entry::Contents),
            "+DEINSTALL" => Some(Entry::DeInstall),
            "+DESC" => Some(Entry::Desc),
            "+DISPLAY" => Some(Entry::Display),
            "+INSTALL" => Some(Entry::Install),
            "+INSTALLED_INFO" => Some(Entry::InstalledInfo),
            "+MTREE_DIRS" => Some(Entry::MtreeDirs),
            "+PRESERVE" => Some(Entry::Preserve),
            "+REQUIRED_BY" => Some(Entry::RequiredBy),
            "+SIZE_ALL" => Some(Entry::SizeAll),
            "+SIZE_PKG" => Some(Entry::SizePkg),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_new() {
        let m = Metadata::new();
        assert!(m.comment().is_empty());
        assert!(m.contents().is_empty());
        assert!(m.desc().is_empty());
        assert!(m.build_info().is_none());
        assert!(m.size_all().is_none());
    }

    #[test]
    fn test_read_metadata_comment() {
        let mut m = Metadata::new();
        m.read_metadata(Entry::Comment, "  Test comment  ").unwrap();
        assert_eq!(m.comment(), "Test comment");
    }

    #[test]
    fn test_read_metadata_contents() {
        let mut m = Metadata::new();
        m.read_metadata(Entry::Contents, "bin/foo\nbin/bar\n")
            .unwrap();
        assert_eq!(m.contents(), "bin/foo\nbin/bar");
    }

    #[test]
    fn test_read_metadata_desc() {
        let mut m = Metadata::new();
        m.read_metadata(Entry::Desc, "  Line 1\n  Line 2\n\n")
            .unwrap();
        assert_eq!(m.desc(), "  Line 1\n  Line 2");
    }

    #[test]
    fn test_read_metadata_build_info() {
        let mut m = Metadata::new();
        m.read_metadata(Entry::BuildInfo, "KEY1=val1\nKEY2=val2\n")
            .unwrap();
        let info = m.build_info().unwrap();
        assert_eq!(info.len(), 2);
        assert_eq!(info[0], "KEY1=val1");
        assert_eq!(info[1], "KEY2=val2");
    }

    #[test]
    fn test_read_metadata_size() {
        let mut m = Metadata::new();
        m.read_metadata(Entry::SizeAll, "  12345  ").unwrap();
        m.read_metadata(Entry::SizePkg, "67890").unwrap();
        assert_eq!(m.size_all(), Some(12345));
        assert_eq!(m.size_pkg(), Some(67890));
    }

    #[test]
    fn test_read_metadata_invalid_size() {
        let mut m = Metadata::new();
        let result = m.read_metadata(Entry::SizeAll, "not a number");
        assert!(matches!(result, Err(Error::InvalidValue { .. })));
    }

    #[test]
    fn test_read_metadata_negative_size() {
        let mut m = Metadata::new();
        let result = m.read_metadata(Entry::SizeAll, "-100");
        assert!(matches!(result, Err(Error::InvalidValue { .. })));
    }

    #[test]
    fn test_read_metadata_optional_fields() {
        let mut m = Metadata::new();
        m.read_metadata(Entry::DeInstall, "#!/bin/sh\nexit 0")
            .unwrap();
        m.read_metadata(Entry::Install, "#!/bin/sh\nexit 0")
            .unwrap();
        m.read_metadata(Entry::Display, "Important message")
            .unwrap();
        m.read_metadata(Entry::Preserve, "yes").unwrap();
        m.read_metadata(Entry::RequiredBy, "pkg1\npkg2").unwrap();

        assert_eq!(m.deinstall(), Some("#!/bin/sh\nexit 0"));
        assert_eq!(m.install(), Some("#!/bin/sh\nexit 0"));
        assert_eq!(m.display(), Some("Important message"));
        assert_eq!(m.preserve().unwrap(), &["yes"]);
        assert_eq!(m.required_by().unwrap(), &["pkg1", "pkg2"]);
    }

    #[test]
    fn test_validate_empty() {
        let m = Metadata::new();
        assert!(matches!(
            m.validate(),
            Err(Error::MissingRequired("+COMMENT"))
        ));
    }

    #[test]
    fn test_validate_missing_contents() {
        let mut m = Metadata::new();
        m.read_metadata(Entry::Comment, "Test").unwrap();
        assert!(matches!(
            m.validate(),
            Err(Error::MissingRequired("+CONTENTS"))
        ));
    }

    #[test]
    fn test_validate_missing_desc() {
        let mut m = Metadata::new();
        m.read_metadata(Entry::Comment, "Test").unwrap();
        m.read_metadata(Entry::Contents, "bin/foo").unwrap();
        assert!(matches!(m.validate(), Err(Error::MissingRequired("+DESC"))));
    }

    #[test]
    fn test_validate_success() {
        let mut m = Metadata::new();
        m.read_metadata(Entry::Comment, "Test").unwrap();
        m.read_metadata(Entry::Contents, "bin/foo").unwrap();
        m.read_metadata(Entry::Desc, "Description").unwrap();
        assert!(m.validate().is_ok());
        assert!(m.is_valid());
    }

    #[test]
    fn test_is_valid() {
        let mut m = Metadata::new();
        assert!(!m.is_valid());
        m.read_metadata(Entry::Comment, "Test").unwrap();
        assert!(!m.is_valid());
        m.read_metadata(Entry::Contents, "bin/foo").unwrap();
        assert!(!m.is_valid());
        m.read_metadata(Entry::Desc, "Description").unwrap();
        assert!(m.is_valid());
    }

    #[test]
    fn test_entry_display() {
        assert_eq!(Entry::BuildInfo.to_string(), "+BUILD_INFO");
        assert_eq!(Entry::Comment.to_string(), "+COMMENT");
        assert_eq!(Entry::SizePkg.to_string(), "+SIZE_PKG");
    }

    #[test]
    fn test_entry_from_str() {
        assert_eq!("+BUILD_INFO".parse::<Entry>().unwrap(), Entry::BuildInfo);
        assert_eq!("+COMMENT".parse::<Entry>().unwrap(), Entry::Comment);
        assert_eq!("+SIZE_PKG".parse::<Entry>().unwrap(), Entry::SizePkg);

        let result = "+INVALID".parse::<Entry>();
        assert!(matches!(result, Err(Error::UnknownEntry(_))));
    }

    #[test]
    fn test_entry_from_filename() {
        assert_eq!(Entry::from_filename("+BUILD_INFO"), Some(Entry::BuildInfo));
        assert_eq!(Entry::from_filename("+COMMENT"), Some(Entry::Comment));
        assert_eq!(Entry::from_filename("+INVALID"), None);
        assert_eq!(Entry::from_filename("COMMENT"), None);
    }

    #[test]
    fn test_entry_to_filename() {
        assert_eq!(Entry::BuildInfo.to_filename(), "+BUILD_INFO");
        assert_eq!(Entry::BuildVersion.to_filename(), "+BUILD_VERSION");
        assert_eq!(Entry::Comment.to_filename(), "+COMMENT");
        assert_eq!(Entry::Contents.to_filename(), "+CONTENTS");
        assert_eq!(Entry::DeInstall.to_filename(), "+DEINSTALL");
        assert_eq!(Entry::Desc.to_filename(), "+DESC");
        assert_eq!(Entry::Display.to_filename(), "+DISPLAY");
        assert_eq!(Entry::Install.to_filename(), "+INSTALL");
        assert_eq!(Entry::InstalledInfo.to_filename(), "+INSTALLED_INFO");
        assert_eq!(Entry::MtreeDirs.to_filename(), "+MTREE_DIRS");
        assert_eq!(Entry::Preserve.to_filename(), "+PRESERVE");
        assert_eq!(Entry::RequiredBy.to_filename(), "+REQUIRED_BY");
        assert_eq!(Entry::SizeAll.to_filename(), "+SIZE_ALL");
        assert_eq!(Entry::SizePkg.to_filename(), "+SIZE_PKG");
    }

    #[test]
    fn test_entry_roundtrip() {
        let entries = [
            Entry::BuildInfo,
            Entry::BuildVersion,
            Entry::Comment,
            Entry::Contents,
            Entry::DeInstall,
            Entry::Desc,
            Entry::Display,
            Entry::Install,
            Entry::InstalledInfo,
            Entry::MtreeDirs,
            Entry::Preserve,
            Entry::RequiredBy,
            Entry::SizeAll,
            Entry::SizePkg,
        ];
        for entry in entries {
            let filename = entry.to_filename();
            let parsed = Entry::from_filename(filename).unwrap();
            assert_eq!(entry, parsed);
        }
    }

    #[test]
    fn test_error_display() {
        let err = Error::MissingRequired("+COMMENT");
        assert_eq!(err.to_string(), "Missing or empty +COMMENT");

        let err = Error::UnknownEntry("+BADFILE".to_string());
        assert_eq!(err.to_string(), "Unknown metadata entry: +BADFILE");
    }

    #[test]
    fn test_all_optional_getters() {
        let mut m = Metadata::new();

        assert!(m.build_info().is_none());
        assert!(m.build_version().is_none());
        assert!(m.deinstall().is_none());
        assert!(m.display().is_none());
        assert!(m.install().is_none());
        assert!(m.installed_info().is_none());
        assert!(m.mtree_dirs().is_none());
        assert!(m.preserve().is_none());
        assert!(m.required_by().is_none());
        assert!(m.size_all().is_none());
        assert!(m.size_pkg().is_none());

        m.read_metadata(Entry::BuildInfo, "a").unwrap();
        m.read_metadata(Entry::BuildVersion, "b").unwrap();
        m.read_metadata(Entry::DeInstall, "c").unwrap();
        m.read_metadata(Entry::Display, "d").unwrap();
        m.read_metadata(Entry::Install, "e").unwrap();
        m.read_metadata(Entry::InstalledInfo, "f").unwrap();
        m.read_metadata(Entry::MtreeDirs, "g").unwrap();
        m.read_metadata(Entry::Preserve, "h").unwrap();
        m.read_metadata(Entry::RequiredBy, "i").unwrap();
        m.read_metadata(Entry::SizeAll, "100").unwrap();
        m.read_metadata(Entry::SizePkg, "200").unwrap();

        assert_eq!(m.build_info().unwrap(), &["a"]);
        assert_eq!(m.build_version().unwrap(), &["b"]);
        assert_eq!(m.deinstall().unwrap(), "c");
        assert_eq!(m.display().unwrap(), "d");
        assert_eq!(m.install().unwrap(), "e");
        assert_eq!(m.installed_info().unwrap(), &["f"]);
        assert_eq!(m.mtree_dirs().unwrap(), &["g"]);
        assert_eq!(m.preserve().unwrap(), &["h"]);
        assert_eq!(m.required_by().unwrap(), &["i"]);
        assert_eq!(m.size_all().unwrap(), 100);
        assert_eq!(m.size_pkg().unwrap(), 200);
    }
}
