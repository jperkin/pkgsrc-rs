use pkgsrc::digest::Digest;
use pkgsrc::distinfo::*;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

/*
 * Perform size and checksum tests against a distfile entry.
 */
#[test]
fn test_distinfo_distfile_checks() -> Result<(), CheckError> {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let di = Distinfo::from_bytes(&fs::read(&distinfo).unwrap());

    let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file.push("tests/data/digest.txt");
    di.verify_size(&file)?;
    di.verify_checksum(&file, Digest::SHA512)?;
    di.verify_checksum(&file, Digest::BLAKE2s)?;
    assert!(matches!(
        di.verify_checksum(&file, Digest::RMD160),
        Err(CheckError::MissingChecksum(_, _))
    ));
    di.verify_checksums(&file)?;
    di.verify_all(&file)?;

    Ok(())
}

/*
 * Perform checksum tests against a patchfile entry.
 */
#[test]
fn test_distinfo_patchfile_checks() -> Result<(), CheckError> {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let di = Distinfo::from_bytes(&fs::read(&distinfo).unwrap());

    let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file.push("tests/data/patch-Makefile");
    /*
     * Patches don't have size information.  If explicitly calling
     * verify_size() then it's an error, calling verify_all() it is not, only
     * valid tests are counted.
     */
    assert!(matches!(
        di.verify_size(&file),
        Err(CheckError::MissingSize(_))
    ));
    di.verify_checksum(&file, Digest::SHA1)?;
    assert!(matches!(
        di.verify_checksum(&file, Digest::BLAKE2s),
        Err(CheckError::MissingChecksum(_, _))
    ));
    di.verify_checksums(&file)?;
    di.verify_all(&file)?;

    Ok(())
}

#[test]
fn test_distinfo_contents() -> Result<(), CheckError> {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let di = Distinfo::from_bytes(&fs::read(&distinfo).unwrap());

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
    assert_eq!(di.distfiles()[0].checksums[1].hash, "fcca87bdb67a88685a8a25597f9e015f5e60197b9a269fa350ae35a7991ed8da553939b4bbc7f7d3cfd863c67142af403b04165633acbce4339056a905e87fbd");

    assert_eq!(di.patchfiles()[0].filename, PathBuf::from("patch-Makefile"));
    assert_eq!(di.patchfiles()[0].checksums[0].digest, Digest::SHA1);
    assert_eq!(
        di.patchfiles()[0].checksums[0].hash,
        "cb8fb8c2db4b4edebd2ae259b554e766060a45ee"
    );

    Ok(())
}

/*
 * Test that an entry of the form "subdirectory/file.txt" in distinfo is
 * handled correctly.
 */
#[test]
fn test_distinfo_subdir() -> Result<(), CheckError> {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo.subdir");
    let di = Distinfo::from_bytes(&fs::read(&distinfo).unwrap());

    let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file.push("tests/data/subdir/subfile.txt");

    di.verify_all(&file)?;

    Ok(())
}
