use pkgsrc::{PkgName, ScanIndex};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::time::Instant;

fn load_scan_index() -> Vec<ScanIndex> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/data/scanindex/pscan.zstd");

    let file = File::open(&path).expect("failed to open pscan.zstd");
    let decoder =
        zstd::stream::Decoder::new(file).expect("failed to create decoder");
    let reader = BufReader::new(decoder);

    ScanIndex::from_reader(reader)
        .collect::<Result<Vec<_>, _>>()
        .expect("failed to parse scan index")
}

#[test]
fn resolve_full_scan() {
    let start = Instant::now();
    let packages = load_scan_index();
    let load_time = start.elapsed();

    let has_reason =
        |r: &Option<String>| r.as_ref().is_some_and(|s| !s.is_empty());

    // Collect all package names for pattern matching
    let pkgnames: Vec<&PkgName> = packages.iter().map(|p| &p.pkgname).collect();

    // Build index by package base name for fast lookups
    let mut by_base: HashMap<&str, Vec<&PkgName>> = HashMap::new();
    for pkg in &pkgnames {
        by_base.entry(pkg.pkgbase()).or_default().push(pkg);
    }

    let start = Instant::now();
    let mut total_matches = 0usize;
    let mut total_patterns = 0usize;
    let mut unresolved: Vec<(String, String)> = Vec::new();

    for pkg in &packages {
        if has_reason(&pkg.pkg_skip_reason) || has_reason(&pkg.pkg_fail_reason)
        {
            continue;
        }
        let Some(deps) = &pkg.all_depends else {
            continue;
        };

        for dep in deps {
            total_patterns += 1;
            let pattern = dep.pattern();

            let candidates: &[&PkgName] = match pattern.pkgbase() {
                Some(base) => {
                    by_base.get(base).map(|v| v.as_slice()).unwrap_or(&[])
                }
                None => &pkgnames,
            };

            let mut best: Option<&PkgName> = None;
            for candidate in candidates {
                if pattern.matches(candidate.pkgname()) {
                    total_matches += 1;
                    best = match best {
                        None => Some(candidate),
                        Some(current) => pattern
                            .best_match_pbulk(
                                current.pkgname(),
                                candidate.pkgname(),
                            )
                            .ok()
                            .flatten()
                            .map(|s| {
                                if s == current.pkgname() {
                                    current
                                } else {
                                    candidate
                                }
                            }),
                    };
                }
            }

            if best.is_none() {
                unresolved.push((
                    pattern.pattern().to_string(),
                    pkg.pkgname.to_string(),
                ));
            }
        }
    }

    let resolve_time = start.elapsed();

    eprintln!("Packages:     {}", packages.len());
    eprintln!("Patterns:     {}", total_patterns);
    eprintln!("Matches:      {}", total_matches);
    eprintln!("Unresolved:   {}", unresolved.len());
    eprintln!("Load time:    {:?}", load_time);
    eprintln!("Resolve time: {:?}", resolve_time);

    let expected: HashSet<(&str, &str)> = [
        ("py311-buildbot-[0-9]*", "py311-buildbot-badges-2.6.0nb1"),
        (
            "py311-buildbot-[0-9]*",
            "py311-buildbot-waterfall-view-2.6.0nb1",
        ),
        ("py311-stevedore>=1.20.0", "py311-e3-core-22.10.0nb3"),
        ("py312-daemon>=2.3.0", "py312-libagent-0.15.0"),
        ("py313-daemon>=2.3.0", "py313-libagent-0.15.0"),
        ("py314-daemon>=2.3.0", "py314-libagent-0.15.0"),
    ]
    .into_iter()
    .collect();

    let actual: HashSet<(&str, &str)> = unresolved
        .iter()
        .map(|(p, pkg)| (p.as_str(), pkg.as_str()))
        .collect();

    assert_eq!(actual, expected, "unresolved dependencies mismatch");
}
