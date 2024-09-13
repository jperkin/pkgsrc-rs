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
 * Digest hashing and validation.  The [`Digest`] module is mostly a thin
 * wrapper around the [`digest`] crate and a selection of [`hashes`] provided
 * by the [`RustCrypto`] project, with some additional features for a pkgsrc
 * context.
 *
 * ## Examples
 *
 * ```
 * use pkgsrc::digest::{Digest, DigestResult};
 * use std::fs::File;
 * use std::path::PathBuf;
 * use std::str::FromStr;
 *
 * fn main() -> DigestResult<()> {
 *     /* Select digest using an explicit type. */
 *     let d = Digest::BLAKE2s;
 *     let h = d.hash_str("hello world")?;
 *     assert_eq!(h, "9aec6806794561107e594b1f6a8a6b0c92a0cba9acf5e5e93cca06f781813b0b");
 *
 *     /* Set digest from an input string. */
 *     let d = Digest::from_str("RMD160")?;
 *     let h = d.hash_str("hello world")?;
 *     assert_eq!(h, "98c615784ccb5fe5936fbc0cbe9dfdb408d92f0f");
 *
 *     /*
 *      * Internally .finalize() is called on the underlying digest type, so
 *      * state is reset each time and a new string can be hashed.
 *      */
 *     let h = d.hash_str("hello again")?;
 *     assert_eq!(h, "4240355d8422a9f6d7cca0aee38751fb287d2cc2");
 *
 *     /*
 *      * Hash a file.  The entire file contents are hashed with no special
 *      * parsing.
 *      */
 *     let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
 *     file.push("tests/data/digest.txt");
 *     let mut f = File::open(&file)?;
 *     let h = d.hash_file(&mut f)?;
 *     assert_eq!(h, "f20aa3e2ffd45a2915c663e46be79d97e10dd6a5");
 *
 *     /*
 *      * Hash a patch.  These have special handling to remove any lines that
 *      * contain the string "$NetBSD", so that CVS expansion does not affect
 *      * the hash.
 *      */
 *     let d = Digest::from_str("SHA1")?;
 *     let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
 *     file.push("tests/data/patch-Makefile");
 *     let mut f = File::open(&file)?;
 *     let h = d.hash_patch(&mut f)?;
 *     assert_eq!(h, "ab5ce8a374d3aca7948eecabc35386d8195e3fbf");
 *
 *     Ok(())
 * }
 * ```
 *
 * [`RustCrypto`]: https://github.com/RustCrypto
 * [`digest`]: https://docs.rs/digest/latest/digest/
 * [`hashes`]: https://github.com/RustCrypto/hashes
 */

use std::fmt;
use std::io::{BufReader, Read};
use std::str::FromStr;

/**
 * A type alias for the result from the creation of a [`Digest`], with
 * [`DigestError`] returned in [`Err`] variants.
 */
pub type DigestResult<T> = std::result::Result<T, DigestError>;

/**
 * The [`DigestError`] enum contains all of the possible [`Digest`] errors.
 */
#[derive(Debug)]
pub enum DigestError {
    /**
     * An I/O error when reading a file for hashing.
     */
    Io(std::io::Error),
    /**
     * An unknown digest type.
     */
    Unsupported(String),
}

impl PartialEq for DigestError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (DigestError::Io(e1), DigestError::Io(e2)) => {
                e1.kind() == e2.kind()
            }
            (DigestError::Unsupported(e1), DigestError::Unsupported(e2)) => {
                e1 == e2
            }
            _ => false,
        }
    }
}

impl From<std::io::Error> for DigestError {
    fn from(err: std::io::Error) -> Self {
        DigestError::Io(err)
    }
}

impl fmt::Display for DigestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DigestError::Io(s) => write!(f, "I/O error: {}", s),
            DigestError::Unsupported(s) => {
                write!(f, "Unsupported digest: {}", s)
            }
        }
    }
}

impl std::error::Error for DigestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DigestError::Io(err) => Some(err),
            DigestError::Unsupported(_) => None,
        }
    }
}

/**
 * The [`Digest`] enum contains an entry for every supported digest algorithm.
 * All of the algorithms are from the RustCrypto [`hashes`] collection.
 *
 * [`hashes`]: https://github.com/RustCrypto/hashes
 */
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Digest {
    /**
     * Implements `BLAKE2s` hash support using `Blake2s256` from the
     * [`blake2`] crate.
     *
     * [`blake2`]: https://docs.rs/blake2/
     */
    BLAKE2s,
    /**
     * Implements `MD5` hash support using `md5` from the [`md5`]
     * crate.
     *
     * [`md5`]: https://docs.rs/md5/
     */
    MD5,
    /**
     * Implements `RMD160` hash support using `Ripemd160` from the [`ripemd`]
     * crate.
     *
     * [`ripemd`]: https://docs.rs/ripemd/
     */
    RMD160,
    /**
     * Implements `SHA1` hash support using `Sha1` from the [`sha1`] crate.
     *
     * [`sha1`]: https://docs.rs/sha1/
     */
    SHA1,
    /**
     * Implements `SHA256` hash support using `Sha256` from the [`sha2`] crate.
     * This isn't used anywhere in pkgsrc as of time of implementation, but as
     * we are already importing the [`sha2`] crate we may as well support it.
     *
     * [`sha2`]: https://docs.rs/sha2/
     */
    SHA256,
    /**
     * Implements `SHA512` hash support using `Sha512` from the [`sha2`] crate.
     *
     * [`sha2`]: https://docs.rs/sha2/
     */
    SHA512,
}

fn hash_file_internal<R: Read, D: digest::Digest + std::io::Write>(
    reader: &mut R,
) -> DigestResult<String> {
    let mut hasher = D::new();
    std::io::copy(reader, &mut hasher)?;
    let hash = hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut output, b| {
            output.push_str(&format!("{b:02x}"));
            output
        });
    Ok(hash)
}

fn hash_patch_internal<R: Read, D: digest::Digest + std::io::Write>(
    reader: &mut R,
) -> DigestResult<String> {
    let mut hasher = D::new();
    let mut r = BufReader::new(reader);
    let mut s = String::new();
    r.read_to_string(&mut s)?;

    for line in s.split_inclusive('\n') {
        if line.contains("$NetBSD") {
            continue;
        }
        hasher.update(line.as_bytes());
    }

    let hash = hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut output, b| {
            output.push_str(&format!("{b:02x}"));
            output
        });
    Ok(hash)
}

fn hash_str_internal<D: digest::Digest + std::io::Write>(
    s: &str,
) -> DigestResult<String> {
    let mut hasher = D::new();
    hasher.update(s);
    let hash = hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut output, b| {
            output.push_str(&format!("{b:02x}"));
            output
        });
    Ok(hash)
}

impl Digest {
    /**
     * Hash a file.  The full contents of the file are hashed, it is not
     * processed in any way.  Suitable for distfiles.
     */
    pub fn hash_file<R: Read>(&self, reader: &mut R) -> DigestResult<String> {
        match self {
            Digest::BLAKE2s => {
                hash_file_internal::<_, blake2::Blake2s256>(reader)
            }
            Digest::MD5 => hash_file_internal::<_, md5::Md5>(reader),
            Digest::RMD160 => {
                hash_file_internal::<_, ripemd::Ripemd160>(reader)
            }
            Digest::SHA1 => hash_file_internal::<_, sha1::Sha1>(reader),
            Digest::SHA256 => hash_file_internal::<_, sha2::Sha256>(reader),
            Digest::SHA512 => hash_file_internal::<_, sha2::Sha512>(reader),
        }
    }

    /**
     * Hash a pkgsrc patch file.  Any lines containing `$NetBSD` are skipped,
     * so that CVS Id expansion does not change the hash.
     */
    pub fn hash_patch<R: Read>(&self, reader: &mut R) -> DigestResult<String> {
        match self {
            Digest::BLAKE2s => {
                hash_patch_internal::<_, blake2::Blake2s256>(reader)
            }
            Digest::MD5 => hash_patch_internal::<_, md5::Md5>(reader),
            Digest::RMD160 => {
                hash_patch_internal::<_, ripemd::Ripemd160>(reader)
            }
            Digest::SHA1 => hash_patch_internal::<_, sha1::Sha1>(reader),
            Digest::SHA256 => hash_patch_internal::<_, sha2::Sha256>(reader),
            Digest::SHA512 => hash_patch_internal::<_, sha2::Sha512>(reader),
        }
    }
    /**
     * Hash a string.  Mostly useful for testing.
     */
    pub fn hash_str(&self, s: &str) -> DigestResult<String> {
        match self {
            Digest::BLAKE2s => hash_str_internal::<blake2::Blake2s256>(s),
            Digest::MD5 => hash_str_internal::<md5::Md5>(s),
            Digest::RMD160 => hash_str_internal::<ripemd::Ripemd160>(s),
            Digest::SHA1 => hash_str_internal::<sha1::Sha1>(s),
            Digest::SHA256 => hash_str_internal::<sha2::Sha256>(s),
            Digest::SHA512 => hash_str_internal::<sha2::Sha512>(s),
        }
    }
}

impl FromStr for Digest {
    type Err = DigestError;

    fn from_str(s: &str) -> DigestResult<Self> {
        match s.to_lowercase().as_str() {
            "blake2s" => Ok(Digest::BLAKE2s),
            "md5" => Ok(Digest::MD5),
            "rmd160" => Ok(Digest::RMD160),
            "sha1" => Ok(Digest::SHA1),
            "sha256" => Ok(Digest::SHA256),
            "sha512" => Ok(Digest::SHA512),
            _ => Err(DigestError::Unsupported(s.to_string())),
        }
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Digest::BLAKE2s => write!(f, "BLAKE2s"),
            Digest::MD5 => write!(f, "MD5"),
            Digest::RMD160 => write!(f, "RMD160"),
            Digest::SHA1 => write!(f, "SHA1"),
            Digest::SHA256 => write!(f, "SHA256"),
            Digest::SHA512 => write!(f, "SHA512"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_invalid() -> DigestResult<()> {
        let moo = String::from("moo");
        let d = Digest::from_str(&moo);
        assert_eq!(d, Err(DigestError::Unsupported(moo)));
        Ok(())
    }

    #[test]
    fn digest_str() -> DigestResult<()> {
        let d = Digest::from_str("SHA1")?;
        let h = d.hash_str("hello there")?;
        assert_eq!(h, "6e71b3cac15d32fe2d36c270887df9479c25c640");
        Ok(())
    }

    #[test]
    fn digest_str_lower() -> DigestResult<()> {
        let d = Digest::from_str("sha1")?;
        let h = d.hash_str("hello there")?;
        assert_eq!(h, "6e71b3cac15d32fe2d36c270887df9479c25c640");
        Ok(())
    }
}
