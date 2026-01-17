use anyhow::Result;
use pkgsrc::digest::Digest;
use pkgsrc::distinfo::*;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

/*
 * Perform size and checksum tests against a distfile entry.
 */
#[test]
fn test_distinfo_distfile_checks() -> Result<()> {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let di = Distinfo::from_bytes(&fs::read(&distinfo)?);

    let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file.push("tests/data/digest.txt");

    di.verify_size(&file)?;
    di.verify_checksum(&file, Digest::SHA512)?;
    di.verify_checksum(&file, Digest::BLAKE2s)?;
    assert!(matches!(
        di.verify_checksum(&file, Digest::RMD160),
        Err(DistinfoError::MissingChecksum(_, _))
    ));
    for result in di.verify_checksums(&file) {
        assert!(result.is_ok());
    }

    Ok(())
}

/*
 * Perform checksum tests against a patchfile entry.
 */
#[test]
fn test_distinfo_patchfile_checks() -> Result<()> {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let di = Distinfo::from_bytes(&fs::read(&distinfo)?);

    let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file.push("tests/data/patch-Makefile");

    assert!(matches!(
        di.verify_size(&file),
        Err(DistinfoError::MissingSize(_))
    ));
    di.verify_checksum(&file, Digest::SHA1)?;
    assert!(matches!(
        di.verify_checksum(&file, Digest::BLAKE2s),
        Err(DistinfoError::MissingChecksum(_, _))
    ));
    for result in di.verify_checksums(&file) {
        assert!(result.is_ok());
    }

    Ok(())
}

/*
 * Check errors from a bad distfile file.
 */
#[test]
fn test_distinfo_bad_distinfo() -> Result<()> {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo.bad");
    let di = Distinfo::from_bytes(&fs::read(&distinfo)?);

    let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file.push("tests/data/digest.txt");

    assert!(matches!(
        di.verify_size(&file),
        Err(DistinfoError::Size(_, _, _))
    ));
    assert!(matches!(
        di.verify_checksum(&file, Digest::BLAKE2s),
        Err(DistinfoError::Checksum(_, _, _, _))
    ));
    assert!(matches!(
        di.verify_checksum(&file, Digest::SHA512),
        Err(DistinfoError::MissingChecksum(_, _))
    ));
    assert!(matches!(
        di.verify_checksums(&file)[0],
        Err(DistinfoError::Checksum(_, _, _, _))
    ));

    Ok(())
}

/*
 * Verify that trying to check a file that isn't listed in distinfo results in
 * a NotFound error, by trying to pass the distinfo file itself as input.
 */
#[test]
fn test_distinfo_notfound() -> Result<()> {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let di = Distinfo::from_bytes(&fs::read(&distinfo)?);

    assert!(matches!(
        di.verify_size(&distinfo),
        Err(DistinfoError::NotFound)
    ));
    assert!(matches!(
        di.verify_checksum(&distinfo, Digest::BLAKE2s),
        Err(DistinfoError::NotFound)
    ));
    assert!(matches!(
        di.verify_checksums(&distinfo)[0],
        Err(DistinfoError::NotFound)
    ));

    Ok(())
}

#[test]
fn test_distinfo_contents() -> Result<()> {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let di = Distinfo::from_bytes(&fs::read(&distinfo)?);

    assert_eq!(
        di.rcsid(),
        Some(OsString::from(
            "$NetBSD: distinfo,v 1.1 1970/01/01 00:00:00 ken Exp $"
        ))
        .as_ref()
    );
    assert_eq!(
        di.distfiles()[0].filename,
        PathBuf::from("patch-2.7.6.tar.xz")
    );
    assert_eq!(di.distfiles()[0].size, Some(783756));
    assert_eq!(di.distfiles()[0].checksums[0].digest, Digest::BLAKE2s);
    assert_eq!(
        di.distfiles()[0].checksums[0].hash,
        "712c28f8a0fbfbd5ec4cd71ef45204a3780a332d559b5566070138554b89e400"
    );
    assert_eq!(di.distfiles()[0].checksums[1].digest, Digest::SHA512);
    assert_eq!(
        di.distfiles()[0].checksums[1].hash,
        "fcca87bdb67a88685a8a25597f9e015f5e60197b9a269fa350ae35a7991ed8da553939b4bbc7f7d3cfd863c67142af403b04165633acbce4339056a905e87fbd"
    );

    assert_eq!(di.patchfiles()[0].filename, PathBuf::from("patch-Makefile"));
    assert_eq!(di.patchfiles()[0].checksums[0].digest, Digest::SHA1);
    assert_eq!(
        di.patchfiles()[0].checksums[0].hash,
        "ab5ce8a374d3aca7948eecabc35386d8195e3fbf"
    );

    Ok(())
}

/*
 * Test that an entry of the form "subdirectory/file.txt" in distinfo is
 * handled correctly.
 */
#[test]
fn test_distinfo_subdir() -> Result<()> {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo.subdir");
    let di = Distinfo::from_bytes(&fs::read(&distinfo)?);

    let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file.push("tests/data/subdir/subfile.txt");

    assert_eq!(di.verify_size(&file)?, 158);

    let results = di.verify_checksums(&file);
    assert_eq!(results.len(), 2);
    assert!(matches!(results[0], Ok(Digest::BLAKE2s)));
    assert!(matches!(results[1], Ok(Digest::SHA512)));

    Ok(())
}
