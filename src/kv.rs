/*
 * Copyright (c) 2025 Jonathan Perkin <jonathan@perkin.org.uk>
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
 * Type-safe `KEY=VALUE` parsing for various pkgsrc formats.
 *
 * This module re-exports the runtime types from the [`pkgsrc-kv`] crate —
 * [`Span`], [`KvError`], [`KvWarning`], and the [`FromKv`] trait — which power
 * parsing of pkgsrc formats that use `KEY=VALUE` pairs, including:
 *
 * - [`pkg_summary(5)`] via [`Summary`]
 * - [`pbulk-index`] via [`ScanIndex`]
 *
 * Types such as [`PkgName`] only need to implement the [`FromKv`] trait to
 * be used directly.
 *
 * The `Kv` derive macro itself lives in [`pkgsrc-kv`] and is not re-exported
 * here; add that crate as a dependency to derive `Kv` for your own structs.
 * Multi-line variables such as `DESCRIPTION` are collected into a [`Vec`] with
 * `#[kv(multiline)]`, and single-line lists such as `CATEGORIES` by declaring
 * the field as [`Vec`].
 *
 * # Example
 *
 * ```
 * use indoc::indoc;
 * use pkgsrc::PkgName;
 * use pkgsrc_kv::Kv;
 *
 * #[derive(Kv, Debug, PartialEq)]
 * #[kv(allow_unknown)]
 * struct Package {
 *     pkgname: PkgName,
 *     size_pkg: u64,
 *     categories: Vec<String>,
 *     #[kv(variable = "DESCRIPTION", multiline)]
 *     desc: Vec<String>,
 *     /*
 *      * There is no known multi-line variable that also contains multiple
 *      * values per line, this is purely to show how one might be handled if
 *      * necessary, though it would be strongly recommended against.
 *      */
 *     #[kv(multiline)]
 *     all_depends: Vec<Vec<String>>,
 * }
 *
 * let input = indoc! {"
 *     PKGNAME=mktool-1.4.2
 *     SIZE_PKG=6999600
 *     CATEGORIES=devel pkgtools
 *     DESCRIPTION=This is a highly-performant collection of utilities that provide
 *     DESCRIPTION=alternate implementations for parts of the pkgsrc mk infrastructure.
 *     UNKNOWN=Without allow_unknown this would trigger parse failure.
 *     ALL_DEPENDS=cwrappers>=20150314:../../pkgtools/cwrappers
 *     ALL_DEPENDS=checkperms>=1.1:../../sysutils/checkperms rust>=1.74.0:../../lang/rust
 * "};
 *
 * let pkg = Package::parse(input)?;
 * assert_eq!(pkg.pkgname, "mktool-1.4.2");
 * assert_eq!(pkg.size_pkg, 6999600);
 * assert_eq!(pkg.categories, vec!["devel", "pkgtools"]);
 * assert!(pkg.desc[1].starts_with("alternate implementations "));
 * assert_eq!(pkg.all_depends.len(), 2);
 * assert_eq!(pkg.all_depends[0].len(), 1);
 * assert_eq!(pkg.all_depends[1].len(), 2);
 * # Ok::<(), pkgsrc::kv::KvError>(())
 * ```
 *
 * [`pkgsrc-kv`]: https://docs.rs/pkgsrc-kv
 * [`PkgName`]: crate::PkgName
 * [`ScanIndex`]: crate::ScanIndex
 * [`Summary`]: crate::summary::Summary
 * [`pkg_summary(5)`]: https://man.netbsd.org/pkg_summary.5
 * [`pbulk-index`]: https://man.netbsd.org/pbulk-build.1
 */

pub use pkgsrc_kv::{FromKv, KvError, KvWarning, Result, Span};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Depend, PkgName};
    use indoc::indoc;
    use pkgsrc_kv::Kv;
    use std::collections::HashMap;
    use std::path::PathBuf;

    // Standard mktool test data matching pkg_summary.gz
    const MKTOOL_INPUT: &str = indoc! {"
        PKGNAME=mktool-1.4.2
        COMMENT=High performance alternatives for pkgsrc/mk
        SIZE_PKG=6999600
        CATEGORIES=pkgtools
        HOMEPAGE=https://github.com/jperkin/mktool/
    "};

    #[test]
    fn span_to_range() {
        let span = Span { offset: 10, len: 5 };
        let range: std::ops::Range<usize> = span.into();
        assert_eq!(range, 10..15);
    }

    #[test]
    fn fromkv_string() -> Result<()> {
        let span = Span::default();
        assert_eq!(String::from_kv("hello", span)?, "hello");
        Ok(())
    }

    #[test]
    fn fromkv_u64() -> Result<()> {
        let span = Span::default();
        assert_eq!(u64::from_kv("6999600", span)?, 6999600);
        assert!(u64::from_kv("not_a_number", span).is_err());
        Ok(())
    }

    #[test]
    fn fromkv_bool() -> Result<()> {
        let span = Span::default();
        assert!(bool::from_kv("true", span)?);
        assert!(bool::from_kv("yes", span)?);
        assert!(bool::from_kv("1", span)?);
        assert!(!bool::from_kv("false", span)?);
        assert!(!bool::from_kv("no", span)?);
        assert!(!bool::from_kv("0", span)?);
        assert!(bool::from_kv("maybe", span).is_err());
        Ok(())
    }

    #[test]
    fn fromkv_pathbuf() -> Result<()> {
        let span = Span::default();
        let path = PathBuf::from_kv("/usr/bin", span)?;
        assert_eq!(path, PathBuf::from("/usr/bin"));
        Ok(())
    }

    #[derive(Kv, Debug, PartialEq)]
    #[kv(allow_unknown)]
    struct SimplePackage {
        pkgname: String,
        #[kv(variable = "SIZE_PKG")]
        size: u64,
        comment: Option<String>,
    }

    #[test]
    fn derive_simple() -> Result<()> {
        let pkg = SimplePackage::parse(MKTOOL_INPUT)?;
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(pkg.size, 6999600);
        assert_eq!(
            pkg.comment,
            Some("High performance alternatives for pkgsrc/mk".to_string())
        );
        Ok(())
    }

    #[test]
    fn derive_with_optional() -> Result<()> {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            SIZE_PKG=6999600
            COMMENT=High performance alternatives for pkgsrc/mk
        "};
        let pkg = SimplePackage::parse(input)?;
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(pkg.size, 6999600);
        assert_eq!(
            pkg.comment,
            Some("High performance alternatives for pkgsrc/mk".to_string())
        );
        Ok(())
    }

    #[test]
    fn derive_optional_missing() -> Result<()> {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            SIZE_PKG=6999600
        "};
        let pkg = SimplePackage::parse(input)?;
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(pkg.size, 6999600);
        assert_eq!(pkg.comment, None);
        Ok(())
    }

    #[test]
    fn derive_unknown_ignored() -> Result<()> {
        let pkg = SimplePackage::parse(MKTOOL_INPUT)?;
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        Ok(())
    }

    #[test]
    fn derive_missing_required() {
        let input = "PKGNAME=mktool-1.4.2\n";
        let result = SimplePackage::parse(input);
        assert!(matches!(result, Err(KvError::Incomplete(_))));
    }

    #[derive(Kv, Debug, PartialEq)]
    struct VecPackage {
        pkgname: String,
        categories: Vec<String>,
    }

    #[test]
    fn derive_vec_whitespace_separated() -> Result<()> {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            CATEGORIES=pkgtools devel
        "};
        let pkg = VecPackage::parse(input)?;
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(pkg.categories, vec!["pkgtools", "devel"]);
        Ok(())
    }

    #[derive(Kv, Debug, PartialEq)]
    struct LenientPackage {
        pkgname: String,
        #[kv(lenient)]
        weight: Option<u32>,
    }

    #[test]
    fn derive_lenient_valid() -> Result<()> {
        let pkg = LenientPackage::parse("PKGNAME=foo-1.0\nWEIGHT=200\n")?;
        assert_eq!(pkg.weight, Some(200));
        Ok(())
    }

    #[test]
    fn derive_lenient_invalid_is_none() -> Result<()> {
        let pkg = LenientPackage::parse("PKGNAME=foo-1.0\nWEIGHT=bad\n")?;
        assert_eq!(pkg.weight, None);
        Ok(())
    }

    #[test]
    fn derive_lenient_absent() -> Result<()> {
        let pkg = LenientPackage::parse("PKGNAME=foo-1.0\n")?;
        assert_eq!(pkg.weight, None);
        Ok(())
    }

    #[derive(Kv, Debug, PartialEq)]
    struct WarnPackage {
        pkgname: String,
        #[kv(lenient)]
        weight: Option<u32>,
        #[kv(warnings)]
        warnings: Vec<KvWarning>,
    }

    #[test]
    fn derive_warnings_records_invalid() -> Result<()> {
        let pkg = WarnPackage::parse("PKGNAME=foo-1.0\nWEIGHT=bad\n")?;
        assert_eq!(pkg.weight, None);
        assert_eq!(pkg.warnings.len(), 1);
        assert_eq!(pkg.warnings[0].variable, "WEIGHT");
        assert_eq!(pkg.warnings[0].value, "bad");
        Ok(())
    }

    #[test]
    fn derive_warnings_empty_when_valid() -> Result<()> {
        let pkg = WarnPackage::parse("PKGNAME=foo-1.0\nWEIGHT=5\n")?;
        assert_eq!(pkg.weight, Some(5));
        assert!(pkg.warnings.is_empty());
        Ok(())
    }

    #[test]
    fn derive_warnings_invalid_overwrites_valid() -> Result<()> {
        let pkg =
            WarnPackage::parse("PKGNAME=foo-1.0\nWEIGHT=5\nWEIGHT=bad\n")?;
        assert_eq!(pkg.weight, None);
        assert_eq!(pkg.warnings.len(), 1);
        Ok(())
    }

    #[derive(Kv, Debug, PartialEq)]
    struct MultiLinePackage {
        pkgname: String,
        #[kv(multiline)]
        description: Vec<String>,
    }

    #[test]
    fn derive_multiline() -> Result<()> {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            DESCRIPTION=This is a highly-performant collection of utilities.
            DESCRIPTION=Many targets under pkgsrc/mk are implemented using shell.
        "};
        let pkg = MultiLinePackage::parse(input)?;
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(pkg.description.len(), 2);
        assert_eq!(
            pkg.description[0],
            "This is a highly-performant collection of utilities."
        );
        assert_eq!(
            pkg.description[1],
            "Many targets under pkgsrc/mk are implemented using shell."
        );
        Ok(())
    }

    #[test]
    fn derive_parse_error() {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            SIZE_PKG=not_a_number
        "};
        let result = SimplePackage::parse(input);
        assert!(matches!(result, Err(KvError::ParseInt { .. })));
    }

    #[test]
    fn derive_bad_line() {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            bad-line
            SIZE_PKG=6999600
        "};
        let result = SimplePackage::parse(input);
        assert!(matches!(result, Err(KvError::ParseLine(_))));
    }

    #[derive(Kv, Debug, PartialEq)]
    #[kv(allow_unknown)]
    struct ScanIndexTest {
        pkgname: PkgName,
        all_depends: Option<Vec<Depend>>,
    }

    #[test]
    fn derive_pkgname() -> Result<()> {
        let input = "PKGNAME=mktool-1.4.2\n";
        let pkg = ScanIndexTest::parse(input)?;
        assert_eq!(pkg.pkgname.pkgbase(), "mktool");
        assert_eq!(pkg.pkgname.pkgversion(), "1.4.2");
        assert_eq!(pkg.all_depends, None);
        Ok(())
    }

    #[test]
    fn derive_depend_vec() -> Result<()> {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            ALL_DEPENDS=rust-[0-9]*:../../lang/rust curl>=7.0:../../www/curl
        "};
        let pkg = ScanIndexTest::parse(input)?;
        let all_depends = pkg
            .all_depends
            .as_ref()
            .ok_or(KvError::Incomplete("all_depends".to_string()))?;
        assert_eq!(all_depends.len(), 2);
        Ok(())
    }

    #[test]
    fn derive_depend_invalid() {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            ALL_DEPENDS=invalid
        "};
        let result = ScanIndexTest::parse(input);
        assert!(matches!(result, Err(KvError::Parse { .. })));
    }

    #[derive(Kv, Debug, PartialEq)]
    struct WithExtras {
        pkgname: String,
        #[kv(collect)]
        extra: HashMap<String, String>,
    }

    #[test]
    fn derive_extras() -> Result<()> {
        let pkg = WithExtras::parse(MKTOOL_INPUT)?;
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(
            pkg.extra.get("COMMENT"),
            Some(&"High performance alternatives for pkgsrc/mk".to_string())
        );
        assert_eq!(pkg.extra.get("SIZE_PKG"), Some(&"6999600".to_string()));
        assert_eq!(pkg.extra.get("CATEGORIES"), Some(&"pkgtools".to_string()));
        assert_eq!(
            pkg.extra.get("HOMEPAGE"),
            Some(&"https://github.com/jperkin/mktool/".to_string())
        );
        assert_eq!(pkg.extra.len(), 4);
        Ok(())
    }

    #[test]
    fn derive_extras_empty() -> Result<()> {
        let input = "PKGNAME=mktool-1.4.2\n";
        let pkg = WithExtras::parse(input)?;
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert!(pkg.extra.is_empty());
        Ok(())
    }

    /*
     * Exercises the generated serde impls across every field kind: a
     * required value, a present and an absent `Option`, a single-line `Vec`,
     * a `multiline` `Vec`, and a `collect` `HashMap` (serialized via
     * `flatten`). The serialize path borrows rather than clones; this proves
     * it produces output the deserialize path reads back identically.
     */
    #[cfg(feature = "serde")]
    #[derive(Kv, Debug, PartialEq)]
    struct SerdePackage {
        pkgname: String,
        #[kv(variable = "SIZE_PKG")]
        size: u64,
        comment: Option<String>,
        homepage: Option<String>,
        categories: Vec<String>,
        #[kv(multiline)]
        description: Vec<String>,
        #[kv(collect)]
        extra: HashMap<String, String>,
    }

    #[test]
    #[cfg(feature = "serde")]
    fn derive_serde_roundtrip() -> std::result::Result<(), serde_json::Error> {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            SIZE_PKG=6999600
            COMMENT=High performance alternatives for pkgsrc/mk
            CATEGORIES=pkgtools devel
            DESCRIPTION=First line of the description.
            DESCRIPTION=Second line of the description.
            PKGPATH=pkgtools/mktool
        "};
        let pkg = SerdePackage::parse(input).expect("parse");

        /* Sanity-check the parsed value before round-tripping it. */
        assert_eq!(pkg.homepage, None);
        assert_eq!(pkg.description.len(), 2);
        assert_eq!(
            pkg.extra.get("PKGPATH").map(String::as_str),
            Some("pkgtools/mktool")
        );

        let json = serde_json::to_string(&pkg)?;
        let back: SerdePackage = serde_json::from_str(&json)?;
        assert_eq!(pkg, back);

        /*
         * The absent Option must be skipped, and the collected key must be
         * flattened to the top level rather than nested under `extra`.
         */
        assert!(
            !json.contains("homepage"),
            "absent Option should be skipped: {json}"
        );
        assert!(
            json.contains("PKGPATH"),
            "collected key should be flattened: {json}"
        );
        assert!(
            !json.contains("extra"),
            "collect field should flatten, not nest: {json}"
        );
        Ok(())
    }
}
