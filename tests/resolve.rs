use pkgsrc::{DependError, PatternCache, PatternError, PkgName, ScanIndex};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
enum ResolveError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Depend(#[from] DependError),
    #[error(transparent)]
    Pattern(#[from] PatternError),
}

fn load_scan_index() -> Result<Vec<ScanIndex>, ResolveError> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/data/scanindex/pscan.zstd");

    let file = File::open(&path)?;
    let decoder = zstd::stream::Decoder::new(file)?;
    let reader = BufReader::new(decoder);

    Ok(ScanIndex::from_reader(reader).collect::<Result<Vec<_>, _>>()?)
}

#[test]
fn resolve_full_scan() -> Result<(), ResolveError> {
    let start = Instant::now();
    let mut packages = load_scan_index()?;
    let load_time = start.elapsed();

    let has_reason =
        |r: &Option<String>| r.as_ref().is_some_and(|s| !s.is_empty());

    // Collect all package names for pattern matching
    let pkgnames: Vec<PkgName> =
        packages.iter().map(|p| p.pkgname.clone()).collect();

    // Build index by package base name for fast lookups
    let mut by_base: HashMap<&str, Vec<&PkgName>> = HashMap::new();
    for pkg in &pkgnames {
        by_base.entry(pkg.pkgbase()).or_default().push(pkg);
    }

    let start = Instant::now();
    let mut total_patterns = 0usize;
    let mut unresolved: Vec<(String, String)> = Vec::new();
    let mut cache = PatternCache::with_capacity(packages.len());
    let mut resolutions: Vec<Option<Vec<PkgName>>> = vec![None; packages.len()];
    let mut complete_flags = vec![true; packages.len()];
    let mut first_unresolved: Vec<Option<String>> = vec![None; packages.len()];

    /*
     * Resolve every package, not just those without skip/fail reasons -- a
     * skipped package still appears in the pbulk report with a DEPENDS line
     * when its patterns all resolve.  DEPENDS is all-or-nothing: pbulk omits
     * the line entirely if any pattern fails to resolve.
     */
    for (i, pkg) in packages.iter().enumerate() {
        let Some(deps) = &pkg.all_depends else {
            continue;
        };
        let track_unresolved = !has_reason(&pkg.pkg_skip_reason)
            && !has_reason(&pkg.pkg_fail_reason);

        let mut resolved = Vec::new();
        let mut complete = true;

        for dep in deps {
            let dep = dep?;
            total_patterns += 1;
            let pattern = cache.compile(dep.pattern())?;

            let mut best: Option<&str> = None;
            if let Some(bases) = pattern.pkgbases() {
                for base in bases {
                    if let Some(candidates) = by_base.get(base) {
                        for candidate in candidates {
                            best = pattern
                                .best_match_pbulk(best, candidate.pkgname())?;
                        }
                    }
                }
            } else {
                for candidate in &pkgnames {
                    best =
                        pattern.best_match_pbulk(best, candidate.pkgname())?;
                }
            }

            match best {
                Some(name) => resolved.push(
                    name.parse().expect("resolver returns valid pkgname"),
                ),
                None => {
                    complete = false;
                    if first_unresolved[i].is_none() {
                        first_unresolved[i] =
                            Some(pattern.pattern().to_string());
                    }
                    if track_unresolved {
                        unresolved.push((
                            pattern.pattern().to_string(),
                            pkg.pkgname.to_string(),
                        ));
                    }
                }
            }
        }

        complete_flags[i] = complete;
        if complete && !resolved.is_empty() {
            resolutions[i] = Some(resolved);
        }
    }

    /*
     * Apply resolutions so report() can emit DEPENDS lines.  Also synthesize
     * PKG_FAIL_REASON on packages whose first unresolved pattern was set --
     * matches pbulk's own resolver, which writes `"could not resolve
     * dependency "PATTERN""` (quotes included) into pkg_fail_reason so the
     * failure is visible in the report.
     */
    for (i, pkg) in packages.iter_mut().enumerate() {
        pkg.resolved_depends = resolutions[i].take();
        if pkg.pkg_fail_reason.as_deref().is_none_or(str::is_empty) {
            if let Some(pat) = &first_unresolved[i] {
                pkg.pkg_fail_reason =
                    Some(format!("\"could not resolve dependency \"{pat}\"\""));
            }
        }
    }

    let resolve_time = start.elapsed();

    eprintln!("Packages:     {}", packages.len());
    eprintln!("Patterns:     {}", total_patterns);
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

    /*
     * Build the reverse-dependency graph from resolved_depends and compute
     * PKG_DEPTH = |transitive reverse dependents| + 1 (the package itself),
     * matching pbulk's semantics.  Uses the epoch trick to avoid clearing
     * the visited array on each BFS.
     */
    let mut pkg_to_idx: HashMap<&str, usize> =
        HashMap::with_capacity(packages.len());
    for (i, p) in packages.iter().enumerate() {
        pkg_to_idx.insert(p.pkgname.pkgname(), i);
    }
    let mut rev_deps: Vec<Vec<usize>> = vec![Vec::new(); packages.len()];
    for (i, p) in packages.iter().enumerate() {
        if let Some(deps) = &p.resolved_depends {
            for d in deps {
                if let Some(&j) = pkg_to_idx.get(d.pkgname()) {
                    rev_deps[j].push(i);
                }
            }
        }
    }

    let mut pkg_depth = vec![1usize; packages.len()];
    let mut visit_gen = vec![0u32; packages.len()];
    let mut epoch = 0u32;
    let mut queue: VecDeque<usize> = VecDeque::new();
    for i in 0..packages.len() {
        epoch += 1;
        queue.clear();
        queue.push_back(i);
        visit_gen[i] = epoch;
        let mut count = 0usize;
        while let Some(node) = queue.pop_front() {
            count += 1;
            for &r in &rev_deps[node] {
                if visit_gen[r] != epoch {
                    visit_gen[r] = epoch;
                    queue.push_back(r);
                }
            }
        }
        pkg_depth[i] = count;
    }

    /*
     * Synthesize BUILD_STATUS.  pbulk has several statuses (done / failed /
     * prefailed / indirect-failed / indirect-prefailed); without real build
     * results we collapse to two: prefailed when the package is skipped,
     * was marked as failed at scan time, or has unresolved deps; done
     * otherwise.  Exercises the format, not pbulk's full status taxonomy.
     */
    let build_status: Vec<&str> = packages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if !complete_flags[i]
                || has_reason(&p.pkg_skip_reason)
                || has_reason(&p.pkg_fail_reason)
            {
                "prefailed"
            } else {
                "done"
            }
        })
        .collect();

    /*
     * Render the post-resolution report and compare byte-for-byte against
     * the committed report.zst.  Exercises the full pbulk report shape
     * across the whole scan.
     */
    use std::fmt::Write as _;
    let mut rendered = String::new();
    for (i, p) in packages.iter().enumerate() {
        write!(rendered, "{}", p.report()).expect("string write");
        writeln!(rendered, "PKG_DEPTH={}", pkg_depth[i]).expect("string write");
        writeln!(rendered, "BUILD_STATUS={}", build_status[i])
            .expect("string write");
    }

    let mut report_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    report_path.push("tests/data/scanindex/report.zst");
    let mut expected = String::new();
    zstd::stream::Decoder::new(File::open(&report_path)?)?
        .read_to_string(&mut expected)?;

    assert_eq!(rendered, expected, "report() output differs from report.zst");

    Ok(())
}
