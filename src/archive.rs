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
 * Read and write pkgsrc binary packages.
 *
 * pkgsrc binary packages come in two formats:
 *
 * 1. **Unsigned packages**: Compressed tar archives (`.tgz`, `.tbz`, etc.)
 *    containing package metadata (`+CONTENTS`, `+COMMENT`, `+DESC`, etc.)
 *    and the package files.
 *
 * 2. **Signed packages**: `ar(1)` archives containing:
 *    - `+PKG_HASH`: Hash metadata for verification
 *    - `+PKG_GPG_SIGNATURE`: GPG signature of the hash file
 *    - The original compressed tarball
 *
 * This module provides a two-layer API:
 *
 * ## Low-level (tar-style streaming)
 *
 * - [`Archive`]: Streaming access to archive entries
 * - [`Builder`]: Create new archives by appending entries
 *
 * ## High-level (convenience)
 *
 * - [`BinaryPackage`]: Cached metadata with fast reads and convenience methods
 * - [`SignedArchive`]: Output type for signed packages
 *
 * # Examples
 *
 * ## Fast metadata reading
 *
 * ```no_run
 * use pkgsrc::archive::BinaryPackage;
 *
 * let pkg = BinaryPackage::open("package-1.0.tgz").unwrap();
 * println!("Package: {}", pkg.pkgname().unwrap_or("unknown"));
 * println!("Comment: {}", pkg.metadata().comment());
 *
 * // Convert to summary for repository management
 * let summary = pkg.to_summary().unwrap();
 * ```
 *
 * ## Installing a package (iterating entries)
 *
 * ```no_run
 * use pkgsrc::archive::BinaryPackage;
 *
 * let pkg = BinaryPackage::open("package-1.0.tgz").unwrap();
 *
 * // Check dependencies first (fast, uses cached metadata)
 * for dep in pkg.plist().depends() {
 *     println!("Depends: {}", dep);
 * }
 *
 * // Extract files (re-reads archive)
 * pkg.extract_to("/usr/pkg").unwrap();
 * ```
 *
 * ## Building a new package
 *
 * ```no_run
 * use pkgsrc::archive::Builder;
 *
 * // Auto-detect compression from filename
 * let mut builder = Builder::create("package-1.0.tgz").unwrap();
 * builder.append_metadata_file("+COMMENT", b"A test package").unwrap();
 * builder.append_file("bin/hello", b"#!/bin/sh\necho hello", 0o755).unwrap();
 * builder.finish().unwrap();
 * ```
 *
 * ## Signing an existing package
 *
 * ```no_run
 * use pkgsrc::archive::BinaryPackage;
 *
 * let pkg = BinaryPackage::open("package-1.0.tgz").unwrap();
 * let signature = b"GPG SIGNATURE DATA";
 * pkg.sign(signature).unwrap().write_to("package-1.0-signed.tgz").unwrap();
 * ```
 */

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, Permissions};
use std::io::{self, BufReader, Cursor, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use tar::{Archive as TarArchive, Builder as TarBuilder, Entries, Header};

use crate::metadata::{Entry, FileRead, Metadata};
use crate::plist::Plist;
use crate::summary::Summary;

/// Parse a mode string (octal) into a u32.
///
/// Supports formats like "0755", "755", "0644", etc.
fn parse_mode(mode_str: &str) -> Option<u32> {
    // Handle both "0755" and "755" formats
    u32::from_str_radix(mode_str, 8).ok()
}

/// Default block size for package hashing (64KB).
pub const DEFAULT_BLOCK_SIZE: usize = 65536;

/// Current pkgsrc signature version.
pub const PKGSRC_SIGNATURE_VERSION: u32 = 1;

/// Magic bytes identifying gzip compressed data.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Magic bytes identifying zstd compressed data.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

/// Result type for archive operations.
pub type Result<T> = std::result::Result<T, Error>;

// ============================================================================
// Compression
// ============================================================================

/// Compression format for package archives.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Compression {
    /// No compression (plain .tar)
    None,
    /// Gzip compression (.tgz, .tar.gz)
    #[default]
    Gzip,
    /// Zstandard compression (.tzst, .tar.zst)
    Zstd,
}

impl Compression {
    /// Detect compression format from magic bytes.
    #[must_use]
    pub fn from_magic(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < ZSTD_MAGIC.len() {
            return None;
        }
        if bytes.starts_with(&GZIP_MAGIC) {
            Some(Self::Gzip)
        } else if bytes.starts_with(&ZSTD_MAGIC) {
            Some(Self::Zstd)
        } else {
            None
        }
    }

    /// Detect compression format from file extension.
    #[must_use]
    pub fn from_extension(path: impl AsRef<Path>) -> Option<Self> {
        let name = path.as_ref().file_name()?.to_str()?;
        let lower = name.to_lowercase();

        if lower.ends_with(".tgz") || lower.ends_with(".tar.gz") {
            Some(Self::Gzip)
        } else if lower.ends_with(".tzst") || lower.ends_with(".tar.zst") {
            Some(Self::Zstd)
        } else if lower.ends_with(".tar") {
            Some(Self::None)
        } else {
            None
        }
    }

    /// Return the canonical file extension for this compression type.
    #[must_use]
    pub fn extension(&self) -> &'static str {
        match self {
            Self::None => "tar",
            Self::Gzip => "tgz",
            Self::Zstd => "tzst",
        }
    }
}

impl fmt::Display for Compression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Gzip => write!(f, "gzip"),
            Self::Zstd => write!(f, "zstd"),
        }
    }
}

// ============================================================================
// PkgHashAlgorithm
// ============================================================================

/// Hash algorithm used for package signing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PkgHashAlgorithm {
    /// SHA-512 (recommended, default)
    #[default]
    Sha512,
    /// SHA-256
    Sha256,
}

impl PkgHashAlgorithm {
    /// Return the string representation as used in +PKG_HASH.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sha512 => "SHA512",
            Self::Sha256 => "SHA256",
        }
    }

    /// Return the hash output size in bytes.
    #[must_use]
    pub fn hash_size(&self) -> usize {
        match self {
            Self::Sha512 => 64,
            Self::Sha256 => 32,
        }
    }

    /// Compute hash of data.
    #[must_use]
    pub fn hash(&self, data: &[u8]) -> Vec<u8> {
        use sha2::{Digest, Sha256, Sha512};
        match self {
            Self::Sha512 => Sha512::digest(data).to_vec(),
            Self::Sha256 => Sha256::digest(data).to_vec(),
        }
    }

    /// Format hash as lowercase hex string.
    #[must_use]
    pub fn hash_hex(&self, data: &[u8]) -> String {
        self.hash(data)
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}

impl fmt::Display for PkgHashAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for PkgHashAlgorithm {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "SHA512" => Ok(Self::Sha512),
            "SHA256" => Ok(Self::Sha256),
            _ => Err(Error::UnsupportedAlgorithm(s.to_string())),
        }
    }
}

// ============================================================================
// Error
// ============================================================================

/// Error type for archive operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Invalid archive format.
    #[error("invalid archive format: {0}")]
    InvalidFormat(String),

    /// Invalid +PKG_HASH format.
    #[error("invalid +PKG_HASH format: {0}")]
    InvalidPkgHash(String),

    /// Missing required metadata.
    #[error("missing required metadata: {0}")]
    MissingMetadata(String),

    /// Invalid metadata content.
    #[error("invalid metadata: {0}")]
    InvalidMetadata(String),

    /// Plist parsing error.
    #[error("plist error: {0}")]
    Plist(#[from] crate::plist::PlistError),

    /// Hash verification failed.
    #[error("hash verification failed: {0}")]
    HashMismatch(String),

    /// Unsupported algorithm.
    #[error("unsupported hash algorithm: {0}")]
    UnsupportedAlgorithm(String),

    /// Unsupported compression.
    #[error("unsupported compression: {0}")]
    UnsupportedCompression(String),

    /// Summary generation error.
    #[error("summary error: {0}")]
    Summary(String),

    /// No path available for operation.
    #[error("no path available: {0}")]
    NoPath(String),
}

// ============================================================================
// ExtractOptions
// ============================================================================

/// Options for extracting package files.
#[derive(Clone, Debug, Default)]
pub struct ExtractOptions {
    /// Apply file modes from plist `@mode` directives.
    pub apply_mode: bool,
    /// Apply file ownership from plist `@owner`/`@group` directives.
    /// Note: Requires root privileges to change ownership.
    pub apply_ownership: bool,
    /// Preserve original timestamps from the archive.
    pub preserve_mtime: bool,
}

impl ExtractOptions {
    /// Create new extract options with all options disabled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable applying file modes from plist.
    #[must_use]
    pub fn with_mode(mut self) -> Self {
        self.apply_mode = true;
        self
    }

    /// Enable applying file ownership from plist.
    #[must_use]
    pub fn with_ownership(mut self) -> Self {
        self.apply_ownership = true;
        self
    }

    /// Enable preserving original timestamps.
    #[must_use]
    pub fn with_mtime(mut self) -> Self {
        self.preserve_mtime = true;
        self
    }
}

/// Result of extracting a single file.
#[derive(Clone, Debug)]
pub struct ExtractedFile {
    /// Path where the file was extracted.
    pub path: PathBuf,
    /// Whether this is a metadata file (starts with +).
    pub is_metadata: bool,
    /// MD5 checksum from plist, if present.
    pub expected_checksum: Option<String>,
    /// Mode applied to the file.
    pub mode: Option<u32>,
}

// ============================================================================
// PkgHash
// ============================================================================

/// The `+PKG_HASH` file contents for signed packages.
///
/// This structure represents the hash metadata file used in signed pkgsrc
/// packages. It contains information needed to verify the package integrity.
///
/// # Format
///
/// The `+PKG_HASH` file has the following format:
///
/// ```text
/// pkgsrc signature
/// version: 1
/// pkgname: package-1.0
/// algorithm: SHA512
/// block size: 65536
/// file size: 123456
/// <hash1>
/// <hash2>
/// ...
/// ```
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PkgHash {
    version: u32,
    pkgname: String,
    algorithm: PkgHashAlgorithm,
    block_size: usize,
    file_size: u64,
    hashes: Vec<String>,
}

impl PkgHash {
    /// Create a new `PkgHash` with default settings.
    #[must_use]
    pub fn new(pkgname: impl Into<String>) -> Self {
        Self {
            version: PKGSRC_SIGNATURE_VERSION,
            pkgname: pkgname.into(),
            algorithm: PkgHashAlgorithm::default(),
            block_size: DEFAULT_BLOCK_SIZE,
            file_size: 0,
            hashes: Vec::new(),
        }
    }

    /// Parse a `PkgHash` from `+PKG_HASH` file contents.
    pub fn parse(content: &str) -> Result<Self> {
        let lines: Vec<&str> = content.lines().collect();

        if lines.is_empty() || lines[0] != "pkgsrc signature" {
            return Err(Error::InvalidPkgHash(
                "missing 'pkgsrc signature' header".into(),
            ));
        }

        let mut pkg_hash = PkgHash::default();
        let mut header_complete = false;
        let mut line_idx = 1;

        while line_idx < lines.len() && !header_complete {
            let line = lines[line_idx];

            if let Some((key, value)) = line.split_once(": ") {
                match key {
                    "version" => {
                        pkg_hash.version = value.parse().map_err(|_| {
                            Error::InvalidPkgHash(format!(
                                "invalid version: {}",
                                value
                            ))
                        })?;
                    }
                    "pkgname" => {
                        pkg_hash.pkgname = value.to_string();
                    }
                    "algorithm" => {
                        pkg_hash.algorithm = value.parse()?;
                    }
                    "block size" => {
                        pkg_hash.block_size = value.parse().map_err(|_| {
                            Error::InvalidPkgHash(format!(
                                "invalid block size: {}",
                                value
                            ))
                        })?;
                    }
                    "file size" => {
                        pkg_hash.file_size = value.parse().map_err(|_| {
                            Error::InvalidPkgHash(format!(
                                "invalid file size: {}",
                                value
                            ))
                        })?;
                        header_complete = true;
                    }
                    _ => {
                        return Err(Error::InvalidPkgHash(format!(
                            "unknown header field: {}",
                            key
                        )));
                    }
                }
            } else if !line.is_empty() {
                header_complete = true;
                line_idx -= 1;
            }
            line_idx += 1;
        }

        while line_idx < lines.len() {
            let line = lines[line_idx].trim();
            if !line.is_empty() {
                pkg_hash.hashes.push(line.to_string());
            }
            line_idx += 1;
        }

        if pkg_hash.pkgname.is_empty() {
            return Err(Error::InvalidPkgHash("missing pkgname".into()));
        }

        Ok(pkg_hash)
    }

    /// Generate `PkgHash` from a tarball.
    pub fn from_tarball<R: Read>(
        pkgname: impl Into<String>,
        mut reader: R,
        algorithm: PkgHashAlgorithm,
        block_size: usize,
    ) -> Result<Self> {
        let mut pkg_hash = PkgHash::new(pkgname);
        pkg_hash.algorithm = algorithm;
        pkg_hash.block_size = block_size;

        let mut buffer = vec![0u8; block_size];
        let mut total_size: u64 = 0;

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            total_size += bytes_read as u64;
            let hash = algorithm.hash_hex(&buffer[..bytes_read]);
            pkg_hash.hashes.push(hash);
        }

        pkg_hash.file_size = total_size;
        Ok(pkg_hash)
    }

    /// Return the pkgsrc signature version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Return the package name.
    #[must_use]
    pub fn pkgname(&self) -> &str {
        &self.pkgname
    }

    /// Return the hash algorithm.
    #[must_use]
    pub fn algorithm(&self) -> PkgHashAlgorithm {
        self.algorithm
    }

    /// Return the block size.
    #[must_use]
    pub fn block_size(&self) -> usize {
        self.block_size
    }

    /// Return the original file size.
    #[must_use]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Return the block hashes.
    #[must_use]
    pub fn hashes(&self) -> &[String] {
        &self.hashes
    }

    /// Verify a tarball against this hash.
    pub fn verify<R: Read>(&self, mut reader: R) -> Result<bool> {
        let mut buffer = vec![0u8; self.block_size];
        let mut hash_idx = 0;
        let mut total_size: u64 = 0;

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            total_size += bytes_read as u64;

            if hash_idx >= self.hashes.len() {
                return Err(Error::HashMismatch(
                    "more data than expected".into(),
                ));
            }

            let computed = self.algorithm.hash_hex(&buffer[..bytes_read]);
            if computed != self.hashes[hash_idx] {
                return Err(Error::HashMismatch(format!(
                    "block {} hash mismatch",
                    hash_idx
                )));
            }

            hash_idx += 1;
        }

        if total_size != self.file_size {
            return Err(Error::HashMismatch(format!(
                "file size mismatch: expected {}, got {}",
                self.file_size, total_size
            )));
        }

        if hash_idx != self.hashes.len() {
            return Err(Error::HashMismatch(
                "fewer blocks than expected".into(),
            ));
        }

        Ok(true)
    }
}

impl fmt::Display for PkgHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "pkgsrc signature")?;
        writeln!(f, "version: {}", self.version)?;
        writeln!(f, "pkgname: {}", self.pkgname)?;
        writeln!(f, "algorithm: {}", self.algorithm)?;
        writeln!(f, "block size: {}", self.block_size)?;
        writeln!(f, "file size: {}", self.file_size)?;
        for hash in &self.hashes {
            writeln!(f, "{}", hash)?;
        }
        Ok(())
    }
}

// ============================================================================
// ArchiveType
// ============================================================================

/// Type of binary package archive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ArchiveType {
    /// Unsigned package (plain compressed tarball)
    Unsigned,
    /// Signed package (ar archive containing tarball + signatures)
    Signed,
}

// ============================================================================
// Archive (low-level, tar-style)
// ============================================================================

/// Wrapper for different decompression decoders.
///
/// This is an implementation detail exposed due to the generic nature of
/// [`Archive`]. Users should not need to interact with this type directly.
#[doc(hidden)]
#[allow(clippy::large_enum_variant)]
pub enum Decoder<R: Read> {
    None(R),
    Gzip(GzDecoder<R>),
    Zstd(zstd::stream::Decoder<'static, BufReader<R>>),
}

impl<R: Read> Read for Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Decoder::None(r) => r.read(buf),
            Decoder::Gzip(d) => d.read(buf),
            Decoder::Zstd(d) => d.read(buf),
        }
    }
}

/// Low-level streaming access to package archives.
///
/// This provides tar-style streaming access to archive entries. For most use
/// cases, prefer [`BinaryPackage`] which provides cached metadata and convenience
/// methods.
///
/// # Example
///
/// ```no_run
/// use pkgsrc::archive::{Archive, Compression};
/// use std::io::Read;
///
/// let mut archive = Archive::open("package-1.0.tgz").unwrap();
/// for entry in archive.entries().unwrap() {
///     let entry = entry.unwrap();
///     println!("{}", entry.path().unwrap().display());
/// }
/// ```
pub struct Archive<R: Read> {
    inner: TarArchive<Decoder<R>>,
    compression: Compression,
}

impl Archive<BufReader<File>> {
    /// Open an archive from a file path.
    ///
    /// Automatically detects compression format from magic bytes.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        // Read magic bytes for compression detection
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        reader.seek(SeekFrom::Start(0))?;

        let compression = Compression::from_magic(&magic)
            .or_else(|| Compression::from_extension(path))
            .unwrap_or(Compression::Gzip);

        Archive::with_compression(reader, compression)
    }
}

impl<R: Read> Archive<R> {
    /// Create a new archive from a reader.
    ///
    /// Defaults to gzip compression. Use [`Archive::with_compression`] to
    /// specify a different format, or [`Archive::open`] to auto-detect from
    /// a file path.
    #[must_use = "creating an archive has no effect if not used"]
    pub fn new(reader: R) -> Result<Self> {
        Self::with_compression(reader, Compression::Gzip)
    }

    /// Create a new archive from a reader with explicit compression.
    #[must_use = "creating an archive has no effect if not used"]
    pub fn with_compression(
        reader: R,
        compression: Compression,
    ) -> Result<Self> {
        let decoder = match compression {
            Compression::None => Decoder::None(reader),
            Compression::Gzip => Decoder::Gzip(GzDecoder::new(reader)),
            Compression::Zstd => {
                Decoder::Zstd(zstd::stream::Decoder::new(reader)?)
            }
        };

        Ok(Archive {
            inner: TarArchive::new(decoder),
            compression,
        })
    }

    /// Return the compression format.
    #[must_use]
    pub fn compression(&self) -> Compression {
        self.compression
    }

    /// Return an iterator over the entries in this archive.
    #[must_use = "entries iterator must be used to iterate"]
    pub fn entries(&mut self) -> Result<Entries<'_, Decoder<R>>> {
        Ok(self.inner.entries()?)
    }
}

// ============================================================================
// Package (high-level, cached metadata)
// ============================================================================

/// Options for converting a [`BinaryPackage`] to a [`Summary`].
#[derive(Debug, Clone, Default)]
pub struct SummaryOptions {
    /// Compute the SHA256 checksum of the package file.
    ///
    /// This requires re-reading the entire package file, which can be slow
    /// for large packages. Default is `false`.
    pub compute_file_cksum: bool,
}

/// A pkgsrc binary package with cached metadata.
///
/// This provides fast access to package metadata without re-reading the
/// archive. The metadata is read once during [`BinaryPackage::open`], and subsequent
/// operations like [`BinaryPackage::entries`] or [`BinaryPackage::extract_to`] re-open
/// the archive as needed.
///
/// # Example
///
/// ```no_run
/// use pkgsrc::archive::BinaryPackage;
///
/// // Fast metadata access
/// let pkg = BinaryPackage::open("package-1.0.tgz").unwrap();
/// println!("Name: {}", pkg.pkgname().unwrap_or("unknown"));
/// println!("Comment: {}", pkg.metadata().comment());
///
/// // Generate summary for repository
/// let summary = pkg.to_summary().unwrap();
///
/// // Extract files (re-reads archive)
/// pkg.extract_to("/usr/pkg").unwrap();
/// ```
#[derive(Debug)]
pub struct BinaryPackage {
    /// Path to the package file.
    path: PathBuf,

    /// Detected compression format.
    compression: Compression,

    /// Type of package (signed or unsigned).
    archive_type: ArchiveType,

    /// Parsed metadata from the package.
    metadata: Metadata,

    /// Parsed packing list.
    plist: Plist,

    /// Build info key-value pairs.
    build_info: HashMap<String, Vec<String>>,

    /// Package hash (for signed packages).
    pkg_hash: Option<PkgHash>,

    /// GPG signature (for signed packages).
    gpg_signature: Option<Vec<u8>>,

    /// File size of the package.
    file_size: u64,
}

impl BinaryPackage {
    /// Open a package from a file path.
    ///
    /// This reads only the metadata entries at the start of the archive,
    /// providing fast access to package information without decompressing
    /// the entire file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let file_size = file.metadata()?.len();
        let mut reader = BufReader::new(file);

        // Read magic bytes
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        reader.seek(SeekFrom::Start(0))?;

        // Check for ar archive (signed package)
        if &magic[..7] == b"!<arch>" {
            Self::read_signed(path, reader, file_size)
        } else {
            Self::read_unsigned(path, reader, &magic, file_size)
        }
    }

    /// Read an unsigned package (compressed tarball).
    fn read_unsigned<R: Read + Seek>(
        path: &Path,
        reader: R,
        magic: &[u8],
        file_size: u64,
    ) -> Result<Self> {
        let compression = Compression::from_magic(magic)
            .or_else(|| Compression::from_extension(path))
            .unwrap_or(Compression::Gzip);

        let decompressed: Box<dyn Read> = match compression {
            Compression::None => Box::new(reader),
            Compression::Gzip => Box::new(GzDecoder::new(reader)),
            Compression::Zstd => Box::new(zstd::stream::Decoder::new(reader)?),
        };

        let mut archive = TarArchive::new(decompressed);
        let mut metadata = Metadata::new();
        let mut plist = Plist::new();
        let mut build_info: HashMap<String, Vec<String>> = HashMap::new();

        for entry_result in archive.entries()? {
            let mut entry = entry_result?;
            let entry_path = entry.path()?.into_owned();

            // Stop at first non-metadata file (fast path)
            let Some(entry_type) =
                entry_path.to_str().and_then(Entry::from_filename)
            else {
                break;
            };

            // Pre-allocate based on entry size to avoid reallocation during read
            let entry_size = entry.header().size().unwrap_or(0) as usize;
            let mut content = String::with_capacity(entry_size);
            entry.read_to_string(&mut content)?;
            metadata.read_metadata(entry_type, &content).map_err(|e| {
                Error::InvalidMetadata(format!(
                    "{}: {}",
                    entry_path.display(),
                    e
                ))
            })?;

            if entry_path.as_os_str() == "+CONTENTS" {
                plist = Plist::from_bytes(content.as_bytes())?;
            } else if entry_path.as_os_str() == "+BUILD_INFO" {
                for line in content.lines() {
                    if let Some((key, value)) = line.split_once('=') {
                        build_info
                            .entry(key.to_string())
                            .or_default()
                            .push(value.to_string());
                    }
                }
            }
        }

        metadata.validate().map_err(|e| {
            Error::MissingMetadata(format!("incomplete package: {}", e))
        })?;

        Ok(Self {
            path: path.to_path_buf(),
            compression,
            archive_type: ArchiveType::Unsigned,
            metadata,
            plist,
            build_info,
            pkg_hash: None,
            gpg_signature: None,
            file_size,
        })
    }

    /// Read a signed package (ar archive).
    fn read_signed<R: Read>(
        path: &Path,
        reader: R,
        file_size: u64,
    ) -> Result<Self> {
        let mut ar = ar::Archive::new(reader);

        let mut pkg_hash_content: Option<String> = None;
        let mut gpg_signature: Option<Vec<u8>> = None;
        let mut metadata = Metadata::new();
        let mut plist = Plist::new();
        let mut build_info: HashMap<String, Vec<String>> = HashMap::new();
        let mut compression = Compression::Gzip;

        loop {
            let mut entry = match ar.next_entry() {
                Some(Ok(entry)) => entry,
                Some(Err(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    break;
                }
                Some(Err(e)) => return Err(e.into()),
                None => break,
            };
            let name = String::from_utf8_lossy(entry.header().identifier())
                .to_string();

            match name.as_str() {
                "+PKG_HASH" => {
                    let mut content = String::new();
                    entry.read_to_string(&mut content)?;
                    pkg_hash_content = Some(content);
                }
                "+PKG_GPG_SIGNATURE" => {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data)?;
                    gpg_signature = Some(data);
                }
                _ if name.ends_with(".tgz")
                    || name.ends_with(".tzst")
                    || name.ends_with(".tar") =>
                {
                    // Detect compression from inner tarball name
                    compression = Compression::from_extension(&name)
                        .unwrap_or(Compression::Gzip);

                    let decompressed: Box<dyn Read> = match compression {
                        Compression::None => Box::new(entry),
                        Compression::Gzip => Box::new(GzDecoder::new(entry)),
                        Compression::Zstd => {
                            Box::new(zstd::stream::Decoder::new(entry)?)
                        }
                    };

                    let mut archive = TarArchive::new(decompressed);

                    for tar_entry_result in archive.entries()? {
                        let mut tar_entry = tar_entry_result?;
                        let entry_path = tar_entry.path()?.into_owned();

                        let Some(entry_type) =
                            entry_path.to_str().and_then(Entry::from_filename)
                        else {
                            break;
                        };

                        // Pre-allocate based on entry size to avoid reallocation
                        let entry_size =
                            tar_entry.header().size().unwrap_or(0) as usize;
                        let mut content = String::with_capacity(entry_size);
                        tar_entry.read_to_string(&mut content)?;
                        metadata.read_metadata(entry_type, &content).map_err(
                            |e| {
                                Error::InvalidMetadata(format!(
                                    "{}: {}",
                                    entry_path.display(),
                                    e
                                ))
                            },
                        )?;

                        if entry_path.as_os_str() == "+CONTENTS" {
                            plist = Plist::from_bytes(content.as_bytes())?;
                        } else if entry_path.as_os_str() == "+BUILD_INFO" {
                            for line in content.lines() {
                                if let Some((key, value)) = line.split_once('=')
                                {
                                    build_info
                                        .entry(key.to_string())
                                        .or_default()
                                        .push(value.to_string());
                                }
                            }
                        }
                    }
                    break;
                }
                _ => {}
            }
        }

        let pkg_hash =
            pkg_hash_content.map(|c| PkgHash::parse(&c)).transpose()?;

        metadata.validate().map_err(|e| {
            Error::MissingMetadata(format!("incomplete package: {}", e))
        })?;

        Ok(Self {
            path: path.to_path_buf(),
            compression,
            archive_type: ArchiveType::Signed,
            metadata,
            plist,
            build_info,
            pkg_hash,
            gpg_signature,
            file_size,
        })
    }

    /// Return the path to the package file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the compression format.
    #[must_use]
    pub fn compression(&self) -> Compression {
        self.compression
    }

    /// Return the archive type (signed or unsigned).
    #[must_use]
    pub fn archive_type(&self) -> ArchiveType {
        self.archive_type
    }

    /// Return whether this package is signed.
    #[must_use]
    pub fn is_signed(&self) -> bool {
        self.archive_type == ArchiveType::Signed
    }

    /// Return the package metadata.
    #[must_use]
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Return the packing list.
    #[must_use]
    pub fn plist(&self) -> &Plist {
        &self.plist
    }

    /// Return the package name from the plist.
    #[must_use]
    pub fn pkgname(&self) -> Option<&str> {
        self.plist.pkgname()
    }

    /// Return the build info key-value pairs.
    #[must_use]
    pub fn build_info(&self) -> &HashMap<String, Vec<String>> {
        &self.build_info
    }

    /// Get a specific build info value (first value if multiple exist).
    #[must_use]
    pub fn build_info_value(&self, key: &str) -> Option<&str> {
        self.build_info
            .get(key)
            .and_then(|v| v.first())
            .map(|s| s.as_str())
    }

    /// Get all values for a build info key.
    #[must_use]
    pub fn build_info_values(&self, key: &str) -> Option<&[String]> {
        self.build_info.get(key).map(|v| v.as_slice())
    }

    /// Return the package hash (for signed packages).
    #[must_use]
    pub fn pkg_hash(&self) -> Option<&PkgHash> {
        self.pkg_hash.as_ref()
    }

    /// Return the GPG signature (for signed packages).
    #[must_use]
    pub fn gpg_signature(&self) -> Option<&[u8]> {
        self.gpg_signature.as_deref()
    }

    /// Return the file size of the package.
    #[must_use]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Open the archive for iteration (re-reads the file).
    pub fn archive(&self) -> Result<Archive<BufReader<File>>> {
        Archive::open(&self.path)
    }

    /// Extract all files to a destination directory.
    ///
    /// This re-reads the archive and extracts all entries.
    pub fn extract_to(&self, dest: impl AsRef<Path>) -> Result<()> {
        let mut archive = self.archive()?;
        for entry in archive.entries()? {
            let mut entry = entry?;
            entry.unpack_in(dest.as_ref())?;
        }
        Ok(())
    }

    /// Extract files to a destination directory with plist-based permissions.
    ///
    /// This method extracts files and applies permissions specified in the
    /// packing list (`@mode`, `@owner`, `@group` directives).
    ///
    /// # Arguments
    ///
    /// * `dest` - Destination directory for extraction
    /// * `options` - Extraction options controlling mode/ownership application
    ///
    /// # Returns
    ///
    /// A vector of [`ExtractedFile`] describing each extracted file.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pkgsrc::archive::{BinaryPackage, ExtractOptions};
    ///
    /// let pkg = BinaryPackage::open("package-1.0.tgz").unwrap();
    /// let options = ExtractOptions::new().with_mode();
    /// let extracted = pkg.extract_with_plist("/usr/pkg", options).unwrap();
    /// for file in &extracted {
    ///     println!("Extracted: {}", file.path.display());
    /// }
    /// ```
    #[cfg(unix)]
    pub fn extract_with_plist(
        &self,
        dest: impl AsRef<Path>,
        options: ExtractOptions,
    ) -> Result<Vec<ExtractedFile>> {
        use crate::plist::FileInfo;
        use std::os::unix::ffi::OsStrExt;

        let dest = dest.as_ref();
        let mut extracted = Vec::new();

        // Build a map of file paths to their plist metadata
        let file_infos: HashMap<OsString, FileInfo> = self
            .plist
            .files_with_info()
            .into_iter()
            .map(|info| (info.path.clone(), info))
            .collect();

        let mut archive = self.archive()?;
        for entry_result in archive.entries()? {
            let mut entry = entry_result?;
            let entry_path = entry.path()?.into_owned();

            // Determine if this is a metadata file
            let is_metadata =
                entry_path.as_os_str().as_bytes().starts_with(b"+");

            // Extract the file
            entry.unpack_in(dest)?;

            let full_path = dest.join(&entry_path);

            // Look up plist metadata for this file
            let file_info = file_infos.get(entry_path.as_os_str());

            let mut applied_mode = None;

            // Apply mode from plist if requested
            if options.apply_mode && !is_metadata {
                if let Some(info) = file_info {
                    if let Some(mode_str) = &info.mode {
                        if let Some(mode) = parse_mode(mode_str) {
                            if full_path.exists() && !full_path.is_symlink() {
                                fs::set_permissions(
                                    &full_path,
                                    Permissions::from_mode(mode),
                                )?;
                                applied_mode = Some(mode);
                            }
                        }
                    }
                }
            }

            // Apply ownership from plist if requested
            // Note: This requires root privileges
            #[cfg(unix)]
            if options.apply_ownership && !is_metadata {
                if let Some(info) = file_info {
                    if info.owner.is_some() || info.group.is_some() {
                        // Ownership changes require the nix crate or libc
                        // For now, we just note it in the result but don't apply
                        // To implement: use nix::unistd::{chown, Uid, Gid}
                    }
                }
            }

            extracted.push(ExtractedFile {
                path: full_path,
                is_metadata,
                expected_checksum: file_info.and_then(|i| i.checksum.clone()),
                mode: applied_mode,
            });
        }

        Ok(extracted)
    }

    /// Verify checksums of extracted files against plist MD5 values.
    ///
    /// This method checks that files in the destination directory match
    /// the MD5 checksums recorded in the packing list.
    ///
    /// # Arguments
    ///
    /// * `dest` - Directory where files were extracted
    ///
    /// # Returns
    ///
    /// A vector of tuples containing (file_path, expected_hash, actual_hash)
    /// for files that failed verification. Empty vector means all passed.
    pub fn verify_checksums(
        &self,
        dest: impl AsRef<Path>,
    ) -> Result<Vec<(PathBuf, String, String)>> {
        use md5::{Digest, Md5};

        let dest = dest.as_ref();
        let mut failures = Vec::new();

        for info in self.plist.files_with_info() {
            // Skip files without checksums
            let Some(expected) = &info.checksum else {
                continue;
            };

            // Skip symlinks (they have Symlink: comments instead of MD5:)
            if info.symlink_target.is_some() {
                continue;
            }

            let file_path = dest.join(&info.path);

            if !file_path.exists() {
                failures.push((
                    file_path,
                    expected.clone(),
                    "FILE_NOT_FOUND".to_string(),
                ));
                continue;
            }

            // Compute MD5 of the file
            let mut file = File::open(&file_path)?;
            let mut hasher = Md5::new();
            io::copy(&mut file, &mut hasher)?;
            let result = hasher.finalize();
            let actual = format!("{:032x}", result);

            if actual != *expected {
                failures.push((file_path, expected.clone(), actual));
            }
        }

        Ok(failures)
    }

    /// Sign this package.
    ///
    /// Re-reads the package file to compute hashes and create a signed archive.
    pub fn sign(&self, signature: &[u8]) -> Result<SignedArchive> {
        let pkgname = self
            .pkgname()
            .ok_or_else(|| Error::MissingMetadata("pkgname".into()))?
            .to_string();

        // Read the tarball data
        let tarball = std::fs::read(&self.path)?;

        // Generate hash
        let pkg_hash = PkgHash::from_tarball(
            &pkgname,
            Cursor::new(&tarball),
            PkgHashAlgorithm::Sha512,
            DEFAULT_BLOCK_SIZE,
        )?;

        Ok(SignedArchive {
            pkgname,
            compression: self.compression,
            pkg_hash,
            signature: signature.to_vec(),
            tarball,
        })
    }

    /// Convert this package to a [`Summary`] entry.
    ///
    /// This uses default options (no file checksum computation).
    /// Use [`to_summary_with_opts`](Self::to_summary_with_opts) for more control.
    pub fn to_summary(&self) -> Result<Summary> {
        self.to_summary_with_opts(&SummaryOptions::default())
    }

    /// Convert this package to a [`Summary`] entry with options.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use pkgsrc::archive::{BinaryPackage, SummaryOptions};
    ///
    /// let pkg = BinaryPackage::open("package-1.0.tgz").unwrap();
    /// let opts = SummaryOptions { compute_file_cksum: true };
    /// let summary = pkg.to_summary_with_opts(&opts).unwrap();
    /// ```
    pub fn to_summary_with_opts(
        &self,
        opts: &SummaryOptions,
    ) -> Result<Summary> {
        use sha2::{Digest, Sha256};

        let pkgname = self
            .plist
            .pkgname()
            .map(crate::PkgName::new)
            .ok_or_else(|| Error::MissingMetadata("PKGNAME".into()))?;

        // Helper to convert Vec<&str> to Option<Vec<String>>, avoiding allocation when empty
        fn to_opt_vec(v: Vec<&str>) -> Option<Vec<String>> {
            if v.is_empty() {
                None
            } else {
                Some(v.into_iter().map(String::from).collect())
            }
        }

        // Helper to filter empty/whitespace-only strings
        let non_empty = |s: &&str| !s.trim().is_empty();

        // Helper to convert &str to String, avoiding redundant into() calls
        let to_string = |s: &str| String::from(s);

        // Compute SHA256 checksum of the package file if requested
        let file_cksum = if opts.compute_file_cksum && self.file_size > 0 {
            let mut file = File::open(&self.path)?;
            let mut hasher = Sha256::new();
            io::copy(&mut file, &mut hasher)?;
            let hash = hasher.finalize();
            Some(format!(
                "sha256 {}",
                hash.iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>()
            ))
        } else {
            None
        };

        Ok(Summary::new(
            pkgname,
            self.metadata.comment().to_string(),
            self.metadata.size_pkg().unwrap_or(0),
            to_string(self.build_info_value("BUILD_DATE").unwrap_or("")),
            self.build_info_value("CATEGORIES")
                .unwrap_or("")
                .split_whitespace()
                .map(String::from)
                .collect(),
            to_string(self.build_info_value("MACHINE_ARCH").unwrap_or("")),
            to_string(self.build_info_value("OPSYS").unwrap_or("")),
            to_string(self.build_info_value("OS_VERSION").unwrap_or("")),
            to_string(self.build_info_value("PKGPATH").unwrap_or("")),
            to_string(self.build_info_value("PKGTOOLS_VERSION").unwrap_or("")),
            self.metadata.desc().lines().map(String::from).collect(),
            // Optional fields - avoid Vec<String> allocation when empty
            to_opt_vec(self.plist.conflicts()),
            to_opt_vec(self.plist.depends()),
            self.build_info_value("HOMEPAGE")
                .filter(non_empty)
                .map(to_string),
            self.build_info_value("LICENSE").map(to_string),
            self.build_info_value("PKG_OPTIONS").map(to_string),
            self.build_info_value("PREV_PKGPATH")
                .filter(non_empty)
                .map(to_string),
            self.build_info_values("PROVIDES").map(|v| v.to_vec()),
            self.build_info_values("REQUIRES").map(|v| v.to_vec()),
            self.build_info_values("SUPERSEDES").map(|v| v.to_vec()),
            self.path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned()),
            if self.file_size > 0 {
                Some(self.file_size)
            } else {
                None
            },
            file_cksum,
        ))
    }
}

impl FileRead for BinaryPackage {
    fn pkgname(&self) -> &str {
        self.plist.pkgname().unwrap_or("")
    }

    fn comment(&self) -> std::io::Result<String> {
        Ok(self.metadata.comment().to_string())
    }

    fn contents(&self) -> std::io::Result<String> {
        Ok(self.metadata.contents().to_string())
    }

    fn desc(&self) -> std::io::Result<String> {
        Ok(self.metadata.desc().to_string())
    }

    fn build_info(&self) -> std::io::Result<Option<String>> {
        Ok(self.metadata.build_info().map(|v| v.join("\n")))
    }

    fn build_version(&self) -> std::io::Result<Option<String>> {
        Ok(self.metadata.build_version().map(|v| v.join("\n")))
    }

    fn deinstall(&self) -> std::io::Result<Option<String>> {
        Ok(self.metadata.deinstall().map(|s| s.to_string()))
    }

    fn display(&self) -> std::io::Result<Option<String>> {
        Ok(self.metadata.display().map(|s| s.to_string()))
    }

    fn install(&self) -> std::io::Result<Option<String>> {
        Ok(self.metadata.install().map(|s| s.to_string()))
    }

    fn installed_info(&self) -> std::io::Result<Option<String>> {
        Ok(self.metadata.installed_info().map(|v| v.join("\n")))
    }

    fn mtree_dirs(&self) -> std::io::Result<Option<String>> {
        Ok(self.metadata.mtree_dirs().map(|v| v.join("\n")))
    }

    fn preserve(&self) -> std::io::Result<Option<String>> {
        Ok(self.metadata.preserve().map(|v| v.join("\n")))
    }

    fn required_by(&self) -> std::io::Result<Option<String>> {
        Ok(self.metadata.required_by().map(|v| v.join("\n")))
    }

    fn size_all(&self) -> std::io::Result<Option<String>> {
        Ok(self.metadata.size_all().map(|n| n.to_string()))
    }

    fn size_pkg(&self) -> std::io::Result<Option<String>> {
        Ok(self.metadata.size_pkg().map(|n| n.to_string()))
    }
}

impl TryFrom<&BinaryPackage> for Summary {
    type Error = Error;

    fn try_from(pkg: &BinaryPackage) -> Result<Self> {
        pkg.to_summary()
    }
}

// ============================================================================
// Builder (low-level, tar-style)
// ============================================================================

/// Wrapper for different compression encoders.
enum Encoder<W: Write> {
    Gzip(GzEncoder<W>),
    Zstd(zstd::stream::Encoder<'static, W>),
}

impl<W: Write> Write for Encoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Encoder::Gzip(e) => e.write(buf),
            Encoder::Zstd(e) => e.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Encoder::Gzip(e) => e.flush(),
            Encoder::Zstd(e) => e.flush(),
        }
    }
}

impl<W: Write> Encoder<W> {
    fn finish(self) -> io::Result<W> {
        match self {
            Encoder::Gzip(e) => e.finish(),
            Encoder::Zstd(e) => e.finish(),
        }
    }
}

/// Build a new compressed package archive.
///
/// This provides tar-style streaming construction of package archives.
/// Supports gzip and zstd compression.
///
/// # Example
///
/// ```no_run
/// use pkgsrc::archive::Builder;
///
/// // Create a package with auto-detected compression from filename
/// let mut builder = Builder::create("package-1.0.tgz").unwrap();
///
/// // Add metadata files first
/// builder.append_metadata_file("+CONTENTS", b"@name package-1.0\n").unwrap();
/// builder.append_metadata_file("+COMMENT", b"A test package").unwrap();
/// builder.append_metadata_file("+DESC", b"Description here").unwrap();
///
/// // Add package files
/// builder.append_file("bin/hello", b"#!/bin/sh\necho hello", 0o755).unwrap();
///
/// builder.finish().unwrap();
/// ```
pub struct Builder<W: Write> {
    inner: TarBuilder<Encoder<W>>,
    compression: Compression,
}

impl Builder<File> {
    /// Create a new archive file with compression auto-detected from extension.
    ///
    /// Supported extensions:
    /// - `.tgz`, `.tar.gz` → gzip
    /// - `.tzst`, `.tar.zst` → zstd
    ///
    /// Falls back to gzip for unrecognized extensions.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let compression =
            Compression::from_extension(path).unwrap_or(Compression::Gzip);
        let file = File::create(path)?;
        Self::with_compression(file, compression)
    }
}

impl<W: Write> Builder<W> {
    /// Create a new archive builder with gzip compression (default).
    ///
    /// Use [`Builder::with_compression`] for other formats, or
    /// [`Builder::create`] to auto-detect from a file path.
    pub fn new(writer: W) -> Result<Self> {
        Self::with_compression(writer, Compression::Gzip)
    }

    /// Create a new archive builder with explicit compression.
    pub fn with_compression(
        writer: W,
        compression: Compression,
    ) -> Result<Self> {
        let encoder = match compression {
            Compression::Gzip => Encoder::Gzip(GzEncoder::new(
                writer,
                flate2::Compression::default(),
            )),
            Compression::Zstd => Encoder::Zstd(zstd::stream::Encoder::new(
                writer,
                zstd::DEFAULT_COMPRESSION_LEVEL,
            )?),
            Compression::None => {
                return Err(Error::UnsupportedCompression(
                    "uncompressed archives not supported for building".into(),
                ));
            }
        };

        Ok(Self {
            inner: TarBuilder::new(encoder),
            compression,
        })
    }

    /// Return the compression format.
    #[must_use]
    pub fn compression(&self) -> Compression {
        self.compression
    }

    /// Append a metadata file (e.g., +CONTENTS, +COMMENT).
    pub fn append_metadata_file(
        &mut self,
        name: &str,
        content: &[u8],
    ) -> Result<()> {
        let mut header = Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();

        self.inner.append_data(&mut header, name, content)?;
        Ok(())
    }

    /// Append a file with the given path, content, and mode.
    pub fn append_file(
        &mut self,
        path: impl AsRef<Path>,
        content: &[u8],
        mode: u32,
    ) -> Result<()> {
        let mut header = Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(mode);
        header.set_mtime(0);
        header.set_cksum();

        self.inner.append_data(&mut header, path, content)?;
        Ok(())
    }

    /// Append a file from disk.
    pub fn append_path(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.inner.append_path(path)?;
        Ok(())
    }

    /// Finish building the archive and return the underlying writer.
    pub fn finish(self) -> Result<W> {
        let encoder = self.inner.into_inner()?;
        let writer = encoder.finish()?;
        Ok(writer)
    }
}

// ============================================================================
// SignedArchive
// ============================================================================

/// A signed binary package ready to be written.
///
/// This is created by [`BinaryPackage::sign`] or [`SignedArchive::from_unsigned`].
#[derive(Debug)]
pub struct SignedArchive {
    pkgname: String,
    compression: Compression,
    pkg_hash: PkgHash,
    signature: Vec<u8>,
    tarball: Vec<u8>,
}

impl SignedArchive {
    /// Create a signed archive from unsigned tarball bytes.
    ///
    /// This is useful for signing a freshly-built package without writing
    /// it to disk first.
    pub fn from_unsigned(
        data: Vec<u8>,
        pkgname: impl Into<String>,
        signature: &[u8],
        compression: Compression,
    ) -> Result<Self> {
        let pkgname = pkgname.into();
        let pkg_hash = PkgHash::from_tarball(
            &pkgname,
            Cursor::new(&data),
            PkgHashAlgorithm::Sha512,
            DEFAULT_BLOCK_SIZE,
        )?;

        Ok(Self {
            pkgname,
            compression,
            pkg_hash,
            signature: signature.to_vec(),
            tarball: data,
        })
    }

    /// Return the package name.
    #[must_use]
    pub fn pkgname(&self) -> &str {
        &self.pkgname
    }

    /// Return the compression format of the inner tarball.
    #[must_use]
    pub fn compression(&self) -> Compression {
        self.compression
    }

    /// Return the package hash.
    #[must_use]
    pub fn pkg_hash(&self) -> &PkgHash {
        &self.pkg_hash
    }

    /// Write the signed package to a file.
    pub fn write_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let file = File::create(path)?;
        self.write(file)
    }

    /// Write the signed package to a writer.
    pub fn write<W: Write>(&self, writer: W) -> Result<()> {
        let mut ar = ar::Builder::new(writer);

        // Write +PKG_HASH
        let hash_content = self.pkg_hash.to_string();
        let hash_bytes = hash_content.as_bytes();
        let mut header =
            ar::Header::new(b"+PKG_HASH".to_vec(), hash_bytes.len() as u64);
        header.set_mode(0o644);
        ar.append(&header, hash_bytes)?;

        // Write +PKG_GPG_SIGNATURE
        let mut header = ar::Header::new(
            b"+PKG_GPG_SIGNATURE".to_vec(),
            self.signature.len() as u64,
        );
        header.set_mode(0o644);
        ar.append(&header, self.signature.as_slice())?;

        // Write tarball with appropriate extension
        let tarball_name =
            format!("{}.{}", self.pkgname, self.compression.extension());
        let mut header = ar::Header::new(
            tarball_name.into_bytes(),
            self.tarball.len() as u64,
        );
        header.set_mode(0o644);
        ar.append(&header, self.tarball.as_slice())?;

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_compression_from_magic() {
        assert_eq!(
            Compression::from_magic(&[0x1f, 0x8b, 0, 0, 0, 0]),
            Some(Compression::Gzip)
        );
        assert_eq!(
            Compression::from_magic(&[0x28, 0xb5, 0x2f, 0xfd, 0, 0]),
            Some(Compression::Zstd)
        );
        assert_eq!(Compression::from_magic(&[0, 0, 0, 0, 0, 0]), None);
    }

    #[test]
    fn test_compression_from_extension() {
        assert_eq!(
            Compression::from_extension("foo.tgz"),
            Some(Compression::Gzip)
        );
        assert_eq!(
            Compression::from_extension("foo.tar.gz"),
            Some(Compression::Gzip)
        );
        assert_eq!(
            Compression::from_extension("foo.tzst"),
            Some(Compression::Zstd)
        );
        assert_eq!(
            Compression::from_extension("foo.tar.zst"),
            Some(Compression::Zstd)
        );
        assert_eq!(
            Compression::from_extension("foo.tar"),
            Some(Compression::None)
        );
    }

    #[test]
    fn test_hash_algorithm() {
        assert_eq!(
            "SHA512".parse::<PkgHashAlgorithm>().ok(),
            Some(PkgHashAlgorithm::Sha512)
        );
        assert_eq!(
            "sha256".parse::<PkgHashAlgorithm>().ok(),
            Some(PkgHashAlgorithm::Sha256)
        );
        assert!("MD5".parse::<PkgHashAlgorithm>().is_err());

        assert_eq!(PkgHashAlgorithm::Sha512.as_str(), "SHA512");
        assert_eq!(PkgHashAlgorithm::Sha256.as_str(), "SHA256");

        assert_eq!(PkgHashAlgorithm::Sha512.hash_size(), 64);
        assert_eq!(PkgHashAlgorithm::Sha256.hash_size(), 32);
    }

    #[test]
    fn test_pkg_hash_parse() {
        let content = "\
pkgsrc signature
version: 1
pkgname: test-1.0
algorithm: SHA512
block size: 65536
file size: 12345
abc123
def456
";
        let pkg_hash = PkgHash::parse(content).unwrap();

        assert_eq!(pkg_hash.version(), 1);
        assert_eq!(pkg_hash.pkgname(), "test-1.0");
        assert_eq!(pkg_hash.algorithm(), PkgHashAlgorithm::Sha512);
        assert_eq!(pkg_hash.block_size(), 65536);
        assert_eq!(pkg_hash.file_size(), 12345);
        assert_eq!(pkg_hash.hashes(), &["abc123", "def456"]);
    }

    #[test]
    fn test_pkg_hash_generate() {
        let data = b"Hello, World!";
        let pkg_hash = PkgHash::from_tarball(
            "test-1.0",
            Cursor::new(data),
            PkgHashAlgorithm::Sha512,
            1024,
        )
        .unwrap();

        assert_eq!(pkg_hash.pkgname(), "test-1.0");
        assert_eq!(pkg_hash.algorithm(), PkgHashAlgorithm::Sha512);
        assert_eq!(pkg_hash.block_size(), 1024);
        assert_eq!(pkg_hash.file_size(), 13);
        assert_eq!(pkg_hash.hashes().len(), 1);
    }

    #[test]
    fn test_pkg_hash_verify() {
        let data = b"Hello, World!";
        let pkg_hash = PkgHash::from_tarball(
            "test-1.0",
            Cursor::new(data),
            PkgHashAlgorithm::Sha512,
            1024,
        )
        .unwrap();

        assert!(pkg_hash.verify(Cursor::new(data)).unwrap());

        let bad_data = b"Goodbye, World!";
        assert!(pkg_hash.verify(Cursor::new(bad_data)).is_err());
    }

    #[test]
    fn test_pkg_hash_roundtrip() {
        let data = vec![0u8; 200_000];
        let pkg_hash = PkgHash::from_tarball(
            "test-1.0",
            Cursor::new(&data),
            PkgHashAlgorithm::Sha512,
            65536,
        )
        .unwrap();

        let serialized = pkg_hash.to_string();
        let parsed = PkgHash::parse(&serialized).unwrap();

        assert_eq!(pkg_hash.version(), parsed.version());
        assert_eq!(pkg_hash.pkgname(), parsed.pkgname());
        assert_eq!(pkg_hash.algorithm(), parsed.algorithm());
        assert_eq!(pkg_hash.block_size(), parsed.block_size());
        assert_eq!(pkg_hash.file_size(), parsed.file_size());
        assert_eq!(pkg_hash.hashes(), parsed.hashes());

        assert!(parsed.verify(Cursor::new(&data)).unwrap());
    }

    #[test]
    fn test_build_package_gzip() {
        // Use new() which defaults to gzip
        let mut builder = Builder::new(Vec::new()).unwrap();

        let plist = "@name testpkg-1.0\n@cwd /opt/test\nbin/test\n";
        builder
            .append_metadata_file("+CONTENTS", plist.as_bytes())
            .unwrap();
        builder
            .append_metadata_file("+COMMENT", b"A test package")
            .unwrap();
        builder
            .append_metadata_file("+DESC", b"This is a test.\nMultiple lines.")
            .unwrap();
        builder
            .append_metadata_file(
                "+BUILD_INFO",
                b"OPSYS=NetBSD\nMACHINE_ARCH=x86_64\n",
            )
            .unwrap();
        builder
            .append_file("bin/test", b"#!/bin/sh\necho test", 0o755)
            .unwrap();
        let output = builder.finish().unwrap();

        assert!(!output.is_empty());

        // Verify we can read it back using low-level Archive (default gzip)
        let mut archive = Archive::new(Cursor::new(&output)).unwrap();
        let mut found_contents = false;
        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            if entry.path().unwrap().to_str() == Some("+CONTENTS") {
                found_contents = true;
                break;
            }
        }
        assert!(found_contents);
    }

    #[test]
    fn test_build_package_zstd() {
        // Use with_compression for explicit zstd
        let mut builder =
            Builder::with_compression(Vec::new(), Compression::Zstd).unwrap();

        let plist = "@name testpkg-1.0\n@cwd /opt/test\nbin/test\n";
        builder
            .append_metadata_file("+CONTENTS", plist.as_bytes())
            .unwrap();
        builder
            .append_metadata_file("+COMMENT", b"A test package")
            .unwrap();
        builder
            .append_metadata_file("+DESC", b"This is a test.\nMultiple lines.")
            .unwrap();
        builder
            .append_file("bin/test", b"#!/bin/sh\necho test", 0o755)
            .unwrap();
        let output = builder.finish().unwrap();

        assert!(!output.is_empty());

        // Verify we can read it back using low-level Archive
        let mut archive =
            Archive::with_compression(Cursor::new(&output), Compression::Zstd)
                .unwrap();
        let mut found_contents = false;
        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            if entry.path().unwrap().to_str() == Some("+CONTENTS") {
                found_contents = true;
                break;
            }
        }
        assert!(found_contents);
    }

    #[test]
    fn test_signed_archive_from_unsigned() {
        // Build an unsigned package (default gzip)
        let mut builder = Builder::new(Vec::new()).unwrap();
        builder
            .append_metadata_file("+CONTENTS", b"@name testpkg-1.0\n")
            .unwrap();
        builder
            .append_metadata_file("+COMMENT", b"A test package")
            .unwrap();
        builder
            .append_metadata_file("+DESC", b"Test description")
            .unwrap();
        let output = builder.finish().unwrap();

        let fake_signature = b"FAKE GPG SIGNATURE";
        let signed = SignedArchive::from_unsigned(
            output,
            "testpkg-1.0",
            fake_signature,
            Compression::Gzip,
        )
        .unwrap();

        assert_eq!(signed.pkgname(), "testpkg-1.0");
        assert_eq!(signed.pkg_hash().algorithm(), PkgHashAlgorithm::Sha512);
        assert_eq!(signed.compression(), Compression::Gzip);

        // Write to buffer and verify it's an ar archive
        let mut signed_output = Vec::new();
        signed.write(&mut signed_output).unwrap();
        assert!(&signed_output[..7] == b"!<arch>");
    }

    #[test]
    fn test_signed_archive_zstd() {
        // Build an unsigned zstd package
        let mut builder =
            Builder::with_compression(Vec::new(), Compression::Zstd).unwrap();
        builder
            .append_metadata_file("+CONTENTS", b"@name testpkg-1.0\n")
            .unwrap();
        builder
            .append_metadata_file("+COMMENT", b"A test package")
            .unwrap();
        builder
            .append_metadata_file("+DESC", b"Test description")
            .unwrap();
        let output = builder.finish().unwrap();

        let fake_signature = b"FAKE GPG SIGNATURE";
        let signed = SignedArchive::from_unsigned(
            output,
            "testpkg-1.0",
            fake_signature,
            Compression::Zstd,
        )
        .unwrap();

        assert_eq!(signed.pkgname(), "testpkg-1.0");
        assert_eq!(signed.compression(), Compression::Zstd);

        // Write to buffer and verify it's an ar archive
        let mut signed_output = Vec::new();
        signed.write(&mut signed_output).unwrap();
        assert!(&signed_output[..7] == b"!<arch>");
    }

    #[test]
    fn test_parse_mode() {
        // Standard octal formats
        assert_eq!(super::parse_mode("0755"), Some(0o755));
        assert_eq!(super::parse_mode("755"), Some(0o755));
        assert_eq!(super::parse_mode("0644"), Some(0o644));
        assert_eq!(super::parse_mode("644"), Some(0o644));
        assert_eq!(super::parse_mode("0777"), Some(0o777));
        assert_eq!(super::parse_mode("0400"), Some(0o400));

        // Invalid formats
        assert_eq!(super::parse_mode(""), None);
        assert_eq!(super::parse_mode("abc"), None);
        assert_eq!(super::parse_mode("999"), None); // 9 is not valid octal
    }

    #[test]
    fn test_extract_options() {
        let opts = ExtractOptions::new();
        assert!(!opts.apply_mode);
        assert!(!opts.apply_ownership);
        assert!(!opts.preserve_mtime);

        let opts = ExtractOptions::new().with_mode().with_ownership();
        assert!(opts.apply_mode);
        assert!(opts.apply_ownership);
        assert!(!opts.preserve_mtime);
    }
}
