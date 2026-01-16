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
 *
 * An example pkg_info(8) utility
 */

use anyhow::{Result, bail};
use pkgsrc::archive::{Package, SummaryOptions};
use pkgsrc::metadata::MetadataReader;
use pkgsrc::pkgdb::PkgDB;
use rayon::prelude::*;
use regex::Regex;
use std::path::{Path, PathBuf};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "pkg_info", about = "An example pkg_info(8) command")]
pub struct OptArgs {
    /// Set PKG_DBDIR for installed packages
    #[structopt(short = "K", long = "pkg-dbdir")]
    pkg_dbdir: Option<String>,

    /// Enable pkg_summary(5) output
    #[structopt(short = "X", long = "summary")]
    sumout: bool,

    /// Show all packages (default behavior, for compatibility)
    #[structopt(short = "a", long = "all")]
    all: bool,

    /// Number of parallel jobs (default: number of CPUs)
    #[structopt(short = "j", long = "jobs", default_value = "0")]
    jobs: usize,

    /// Compute FILE_CKSUM for each package
    #[structopt(short = "c", long = "file-cksum")]
    file_cksum: bool,

    /// Package files (.tgz) or pattern to match installed packages
    #[structopt(parse(from_os_str))]
    packages: Vec<PathBuf>,
}

fn output_default<P: MetadataReader>(pkg: &P) -> Result<()> {
    println!("{:<19} {}", pkg.pkgname(), pkg.comment()?);
    Ok(())
}

/// Variables from BUILD_INFO that are valid in pkg_summary.
const SUMMARY_BUILD_VARS: &[&str] = &[
    "BUILD_DATE",
    "CATEGORIES",
    "HOMEPAGE",
    "LICENSE",
    "MACHINE_ARCH",
    "OPSYS",
    "OS_VERSION",
    "PKGPATH",
    "PKGTOOLS_VERSION",
    "PKG_OPTIONS",
    "PREV_PKGPATH",
    "PROVIDES",
    "REQUIRES",
    "SUPERSEDES",
];

fn output_summary<P: MetadataReader>(pkg: &P) -> Result<()> {
    let contents = pkg.contents()?;
    let comment = pkg.comment()?;
    let desc = pkg.desc()?;

    // PKGNAME, DEPENDS, CONFLICTS from +CONTENTS in file order
    for line in contents.lines() {
        if let Some(name) = line.strip_prefix("@name ") {
            println!("PKGNAME={}", name);
        } else if let Some(dep) = line.strip_prefix("@pkgdep ") {
            println!("DEPENDS={}", dep);
        } else if let Some(cfl) = line.strip_prefix("@pkgcfl ") {
            println!("CONFLICTS={}", cfl);
        }
    }

    println!("COMMENT={}", comment);

    if let Some(size) = pkg.size_pkg() {
        println!("SIZE_PKG={}", size.trim());
    }

    // BUILD_INFO variables (filtered, in file order)
    if let Some(build_info) = pkg.build_info() {
        for line in build_info.lines() {
            if let Some(var) = line.split('=').next() {
                if SUMMARY_BUILD_VARS.contains(&var) {
                    println!("{}", line);
                }
            }
        }
    }

    for line in desc.lines() {
        println!("DESCRIPTION={}", line);
    }

    println!();
    Ok(())
}

/// Extract summary from a binary package.
fn extract_summary(
    path: &Path,
    opts: &SummaryOptions,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let pkg = Package::open(path)?;
    let summary = pkg.to_summary_with_opts(opts)?;
    Ok(summary.to_string())
}

/// Process installed packages from PKG_DBDIR.
fn process_installed(cmd: &OptArgs) -> Result<()> {
    let pkgm: Option<Regex> = if cmd.packages.len() == 1 {
        Some(Regex::new(cmd.packages[0].to_string_lossy().as_ref())?)
    } else {
        None
    };

    let dbpath = cmd
        .pkg_dbdir
        .clone()
        .unwrap_or_else(|| "/opt/pkg/.pkgdb".to_string());

    let pkgdb = PkgDB::open(Path::new(&dbpath))?;

    for pkg in pkgdb {
        let pkg = pkg?;

        if let Some(m) = &pkgm {
            if !m.is_match(pkg.pkgname()) {
                continue;
            }
        }

        if cmd.sumout {
            output_summary(&pkg)?;
        } else {
            output_default(&pkg)?;
        }
    }

    Ok(())
}

/// Process binary package files.
fn process_binary_packages(cmd: &OptArgs) -> Result<()> {
    if cmd.jobs > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(cmd.jobs)
            .build_global()
            .ok();
    }

    let summary_opts = SummaryOptions {
        compute_file_cksum: cmd.file_cksum,
    };

    let results: Vec<_> = cmd
        .packages
        .par_iter()
        .map(|path| (path, extract_summary(path, &summary_opts)))
        .collect();

    for (path, result) in results {
        match result {
            Ok(summary) => println!("{}\n", summary),
            Err(e) => eprintln!("Error processing {}: {}", path.display(), e),
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let cmd = OptArgs::from_args();

    if cmd.sumout {
        // -X with file arguments processes binary packages
        if !cmd.packages.is_empty() {
            return process_binary_packages(&cmd);
        }
        // -Xa outputs pkg_summary for all installed packages
        if cmd.all {
            return process_installed(&cmd);
        }
        bail!("missing package name(s)");
    }

    // Process installed packages
    process_installed(&cmd)
}
