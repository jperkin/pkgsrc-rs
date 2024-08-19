use pkgsrc::digest::Digest;
use pkgsrc::distinfo::*;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

#[test]
fn test_distinfo() {
    let mut distinfo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    distinfo.push("tests/data/distinfo");
    let file = fs::read(&distinfo).unwrap();
    let di = Distinfo::from_bytes(&file);

    assert_eq!(
        di.rcsid,
        Some(OsString::from(
            "$NetBSD: distinfo,v 1.15 2021/10/26 10:17:59 nia Exp $"
        ))
    );
    assert_eq!(di.files[0].filename, PathBuf::from("patch-2.7.6.tar.xz"));
    assert_eq!(di.files[0].size, Some(783756));
    assert_eq!(di.files[0].checksums[0].digest, Digest::BLAKE2s);
    assert_eq!(
        di.files[0].checksums[0].hash,
        "712c28f8a0fbfbd5ec4cd71ef45204a3780a332d559b5566070138554b89e400"
    );
    assert_eq!(di.files[0].checksums[1].digest, Digest::SHA512);
    assert_eq!(di.files[0].checksums[1].hash, "fcca87bdb67a88685a8a25597f9e015f5e60197b9a269fa350ae35a7991ed8da553939b4bbc7f7d3cfd863c67142af403b04165633acbce4339056a905e87fbd");
    assert_eq!(di.patches[0].filename, PathBuf::from("patch-src_pch.c"));
    assert_eq!(di.patches[0].checksums[0].digest, Digest::SHA1);
    assert_eq!(
        di.patches[0].checksums[0].hash,
        "0aed6cd0d64c380767c39908c388c91ddf3003d1"
    );
    assert_eq!(
        di.patches[1].filename,
        PathBuf::from("patch-tests_Makefile.in")
    );
    assert_eq!(di.patches[1].checksums[0].digest, Digest::SHA1);
    assert_eq!(
        di.patches[1].checksums[0].hash,
        "f7fd200672b65f466a982084d2b907d32a8f0a77"
    );
    assert_eq!(
        di.patches[2].filename,
        PathBuf::from("patch-tests_ed-style")
    );
    assert_eq!(di.patches[2].checksums[0].digest, Digest::SHA1);
    assert_eq!(
        di.patches[2].checksums[0].hash,
        "7d7c2d04eddaab1d07c05022908a98ef9c984e08"
    );
}
