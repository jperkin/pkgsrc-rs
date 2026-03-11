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
use std::io::{BufRead, BufReader, Read};
use std::str::FromStr;
use thiserror::Error;

/**
 * A type alias for the result from the creation of a [`Digest`], with
 * [`DigestError`] returned in [`Err`] variants.
 */
pub type DigestResult<T> = std::result::Result<T, DigestError>;

/**
 * The [`DigestError`] enum contains all of the possible [`Digest`] errors.
 */
#[derive(Debug, Error)]
pub enum DigestError {
    /**
     * An I/O error when reading a file for hashing.
     */
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /**
     * An unknown digest type.
     */
    #[error("unsupported digest: {0}")]
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

/**
 * The [`Digest`] enum contains an entry for every supported digest algorithm.
 * All of the algorithms are from the RustCrypto [`hashes`] collection.
 *
 * [`hashes`]: https://github.com/RustCrypto/hashes
 */
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn file_hash<R: Read, D: digest::Digest + std::io::Write>(
    reader: &mut R,
) -> DigestResult<String> {
    let mut hasher = D::new();
    std::io::copy(reader, &mut hasher)?;
    Ok(hex_encode(&hasher.finalize()))
}

fn patch_hash<R: Read, D: digest::Digest + std::io::Write>(
    reader: &mut R,
) -> DigestResult<String> {
    let mut hasher = D::new();
    let bufreader = BufReader::new(reader);

    for line in bufreader.split(b'\n') {
        let line = line?;
        if line.windows(7).any(|window| window == b"$NetBSD") {
            continue;
        }
        hasher.update(&line);
        hasher.update(b"\n");
    }

    Ok(hex_encode(&hasher.finalize()))
}

fn str_hash<D: digest::Digest + std::io::Write>(
    s: &str,
) -> DigestResult<String> {
    let mut hasher = D::new();
    hasher.update(s);
    Ok(hex_encode(&hasher.finalize()))
}

impl Digest {
    /**
     * Hash a file.  The full contents of the file are hashed, it is not
     * processed in any way.  Suitable for distfiles.
     */
    pub fn hash_file<R: Read>(&self, reader: &mut R) -> DigestResult<String> {
        match self {
            Digest::BLAKE2s => file_hash::<_, blake2::Blake2s256>(reader),
            Digest::MD5 => file_hash::<_, md5::Md5>(reader),
            Digest::RMD160 => file_hash::<_, ripemd::Ripemd160>(reader),
            Digest::SHA1 => file_hash::<_, sha1::Sha1>(reader),
            Digest::SHA256 => file_hash::<_, sha2::Sha256>(reader),
            Digest::SHA512 => file_hash::<_, sha2::Sha512>(reader),
        }
    }

    /**
     * Hash a pkgsrc patch file.  Any lines containing `$NetBSD` are skipped,
     * so that CVS Id expansion does not change the hash.
     */
    pub fn hash_patch<R: Read>(&self, reader: &mut R) -> DigestResult<String> {
        match self {
            Digest::BLAKE2s => patch_hash::<_, blake2::Blake2s256>(reader),
            Digest::MD5 => patch_hash::<_, md5::Md5>(reader),
            Digest::RMD160 => patch_hash::<_, ripemd::Ripemd160>(reader),
            Digest::SHA1 => patch_hash::<_, sha1::Sha1>(reader),
            Digest::SHA256 => patch_hash::<_, sha2::Sha256>(reader),
            Digest::SHA512 => patch_hash::<_, sha2::Sha512>(reader),
        }
    }
    /**
     * Hash a string.  Mostly useful for testing.
     */
    pub fn hash_str(&self, s: &str) -> DigestResult<String> {
        match self {
            Digest::BLAKE2s => str_hash::<blake2::Blake2s256>(s),
            Digest::MD5 => str_hash::<md5::Md5>(s),
            Digest::RMD160 => str_hash::<ripemd::Ripemd160>(s),
            Digest::SHA1 => str_hash::<sha1::Sha1>(s),
            Digest::SHA256 => str_hash::<sha2::Sha256>(s),
            Digest::SHA512 => str_hash::<sha2::Sha512>(s),
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

    #[test]
    fn digest_all_algorithms() -> DigestResult<()> {
        let input = "pkgsrc";
        let expected = [
            (
                Digest::BLAKE2s,
                "blake2s",
                64,
                "5bf50a94babe8cb54ed81cf3356ea2f4fb252edb820c6601a5999ca726736a29",
            ),
            (Digest::MD5, "md5", 32, "f0a983b4f820b134e46ed3ace7e15987"),
            (
                Digest::RMD160,
                "rmd160",
                40,
                "5179342a9242a1699b65232f4d1138d90de53dcb",
            ),
            (
                Digest::SHA1,
                "sha1",
                40,
                "87d10de1d38d207c8404ff018e5e7247a4c9d109",
            ),
            (
                Digest::SHA256,
                "sha256",
                64,
                "e04ac068955c93d64bcfe27eaa409d43ff8242e0ae8c4613292cfe282764627f",
            ),
            (
                Digest::SHA512,
                "sha512",
                128,
                "65cc5090b4f5fbe26ea134d12e55bb8db88bc6498e671f9b931fdce02394f9bf\
              0b792389afcecb4ab6ecb3a5e6457b0ca32d88ab4f23ff905b711f059fcc0ade",
            ),
        ];
        for (digest, name, hex_len, hash) in expected {
            let d = Digest::from_str(name)?;
            assert_eq!(d, digest);
            assert_eq!(d.to_string().to_lowercase(), name);
            let h = d.hash_str(input)?;
            assert_eq!(h.len(), hex_len);
            assert_eq!(h, hash);
            let mut cursor = std::io::Cursor::new(input);
            let h2 = d.hash_file(&mut cursor)?;
            assert_eq!(h2, hash);
            let mut cursor = std::io::Cursor::new(input);
            let h3 = d.hash_patch(&mut cursor)?;
            assert_eq!(h3.len(), hex_len);
        }
        Ok(())
    }

    #[test]
    fn digest_error_io_eq() {
        let e1 = DigestError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "a",
        ));
        let e2 = DigestError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "b",
        ));
        let e3 = DigestError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "a",
        ));
        assert_eq!(e1, e2);
        assert_ne!(e1, e3);
        assert_ne!(e1, DigestError::Unsupported("x".to_string()));
    }
}
