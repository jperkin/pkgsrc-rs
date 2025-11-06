/*
 * Demonstration of the Summary API
 *
 * This example shows the idiomatic usage of the summary module.
 */

use pkgsrc::summary::*;
use std::io::BufReader;

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("=== Summary API Demo ===\n");

    // Example 1: Parse a single summary from a string
    println!("1. Parse single summary:");
    let single_text = r#"
BUILD_DATE=2024-01-01 12:00:00 +0000
CATEGORIES=devel pkgtools
COMMENT=A test package for demonstration
DESCRIPTION=This is a test package.
DESCRIPTION=It demonstrates the new Summary API.
DESCRIPTION=Multiple description lines are supported.
MACHINE_ARCH=x86_64
OPSYS=Linux
OS_VERSION=5.15.0
PKGNAME=demo-pkg-1.0nb1
PKGPATH=devel/demo-pkg
PKGTOOLS_VERSION=20091115
SIZE_PKG=12345
HOMEPAGE=https://example.com/demo-pkg
LICENSE=isc
DEPENDS=dep-pkg-[0-9]*
DEPENDS=another-dep>=2.0
"#;

    // Parse using FromStr trait
    let summary: Summary = single_text.parse()?;
    println!("  Package: {}", summary.pkgname);
    println!("  Base: {}", summary.pkgbase());
    println!("  Version: {}", summary.pkgversion());
    println!("  Comment: {}", summary.comment);
    println!();

    // Example 2: Build a summary programmatically
    println!("2. Build summary with builder pattern:");
    let built = SummaryBuilder::new()
        .pkgname("built-pkg-2.0")
        .comment("Built with SummaryBuilder")
        .categories("net www")
        .description(vec!["Line 1", "Line 2", "Line 3"])
        .machine_arch("aarch64")
        .opsys("NetBSD")
        .os_version("10.0")
        .pkgpath("www/built-pkg")
        .pkgtools_version("20091115")
        .size_pkg(54321)
        .build_date("2024-11-06 10:00:00 +0000")
        .homepage("https://example.com/built")
        .depends(vec!["lib-a>=1.0", "lib-b-[0-9]*"])
        .build()?;
    println!("  Built package: {}", built.pkgname);
    println!("  Fluent API makes construction easy!");
    println!();

    // Example 3: Parse multiple summaries
    println!("3. Parse multiple summaries:");
    let multi_text = format!(
        "{}\n\n{}",
        make_summary("pkg-a", "1.0", "First package"),
        make_summary("pkg-b", "2.0", "Second package")
    );

    let summaries: Summaries = multi_text.parse()?;
    println!("  Parsed {} summaries", summaries.len());
    println!();

    // Example 4: Iterate over summaries
    println!("4. Iterate over summaries:");
    for summary in &summaries {
        println!("  - {}: {}", summary.pkgname, summary.comment);
    }
    println!();

    // Example 5: Index access
    println!("5. Index access:");
    let first = &summaries[0];
    println!("  First package: {}", first.pkgname);
    println!();

    // Example 6: Search and filter
    println!("6. Find summaries:");
    let found = summaries.find_by_pkgname("pkg-a-1.0");
    if let Some(s) = found {
        println!("  Found: {}", s.pkgname);
    }

    // Find by base name
    let by_base: Vec<_> = summaries.find_by_pkgbase("pkg-a").collect();
    println!("  Found {} packages with base 'pkg-a'", by_base.len());
    println!();

    // Example 7: Serialize to JSON (requires serde_json dev-dependency)
    // #[cfg(feature = "serde")]
    // {
    //     println!("7. Serialize to JSON:");
    //     let json = serde_json::to_string_pretty(&summaries)?;
    //     println!("{}", json);
    //     println!();
    // }

    // Example 8: Read from file (streaming)
    println!("8. Stream from reader:");
    let data = multi_text.as_bytes();
    let reader = BufReader::new(data);
    let from_reader = Summaries::from_reader(reader)?;
    println!("  Loaded {} summaries from reader", from_reader.len());
    println!();

    // Example 9: Collect from iterator
    println!("9. Collect from iterator:");
    let collected: Summaries = vec![summary.clone(), built]
        .into_iter()
        .collect();
    println!("  Collected {} summaries", collected.len());
    println!();

    // Example 10: Display output
    println!("10. Display formatting:");
    println!("{}", summaries[0]);

    Ok(())
}

fn make_summary(name: &str, version: &str, comment: &str) -> String {
    format!(
        r#"BUILD_DATE=2024-01-01
CATEGORIES=devel
COMMENT={}
DESCRIPTION=Package {}
MACHINE_ARCH=x86_64
OPSYS=Linux
OS_VERSION=5.15
PKGNAME={}-{}
PKGPATH=devel/{}
PKGTOOLS_VERSION=20091115
SIZE_PKG=1000"#,
        comment, name, name, version, name
    )
}
