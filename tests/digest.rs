use pkgsrc::digest::*;
use std::fs::File;
use std::path::PathBuf;
use std::str::FromStr;

#[test]
fn test_digest_file() -> DigestResult<()> {
    let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file.push("tests/data/digest.txt");

    let mut f = File::open(&file)?;
    let d = Digest::from_str("BLAKE2s")?;
    let h = d.hash_file(&mut f)?;
    assert_eq!(
        h,
        "555e56e8177159b7d7fe96d5068dcf5335b554b917c8daaa4c893ec4f04b5303"
    );

    let mut f = File::open(&file)?;
    let d = Digest::from_str("RMD160")?;
    let h = d.hash_file(&mut f)?;
    assert_eq!(h, "f20aa3e2ffd45a2915c663e46be79d97e10dd6a5");

    let mut f = File::open(&file)?;
    let d = Digest::from_str("SHA1")?;
    let h = d.hash_file(&mut f)?;
    assert_eq!(h, "5289ee33f2b9a205fdefa2633d568681100e94fc");

    let mut f = File::open(&file)?;
    let d = Digest::from_str("SHA256")?;
    let h = d.hash_file(&mut f)?;
    assert_eq!(
        h,
        "89f85dcb8da0c75cff33a7a63eddb72b1122cfa4f5b6003a872f0fd5b63725e2"
    );

    let mut f = File::open(&file)?;
    let d = Digest::from_str("SHA512")?;
    let h = d.hash_file(&mut f)?;
    assert_eq!(h, "1b8bd4264ac86f9535376965b3e94a622a4da4daf1f516184609541f9a12139e0accf24fd41bfab95114d0ba3fcfc589fa911e2597b29c3221b66898ae4cfa13");

    Ok(())
}
