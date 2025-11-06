/*
 * Copyright (c) 2019 Jonathan Perkin <jonathan@perkin.org.uk>
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

use pkgsrc::pkgdb::{Package, PkgDB};
use pkgsrc::plist::Plist;
use pkgsrc::summary::{Result, SummaryBuilder};
use pkgsrc::MetadataEntry;
use regex::Regex;
use std::path::Path;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "pkg_info", about = "An example pkg_info(8) command")]
pub struct OptArgs {
    #[structopt(short = "K", long = "pkg-dbdir", help = "Set PKG_DBDIR")]
    pkg_dbdir: Option<String>,
    #[structopt(
        short = "X",
        long = "summary",
        help = "Enable pkg_summary(5) output"
    )]
    sumout: bool,
    #[structopt(parse(from_str))]
    pkgmatch: Option<String>,
}

fn output_default(pkg: &Package) -> Result<()> {
    println!(
        "{:20} {}",
        pkg.pkgname(),
        pkg.read_metadata(MetadataEntry::Comment)?.trim()
    );
    Ok(())
}

fn output_summary(pkg: &Package) -> Result<()> {
    let mut builder = SummaryBuilder::new();

    builder = builder.pkgname(pkg.pkgname());
    builder = builder.comment(pkg.read_metadata(MetadataEntry::Comment)?.trim());
    builder = builder.size_pkg(
        pkg.read_metadata(MetadataEntry::SizePkg)?
            .trim()
            .parse::<i64>()?,
    );

    let bi = pkg.read_metadata(MetadataEntry::BuildInfo)?;
    for line in bi.lines() {
        let v: Vec<&str> = line.splitn(2, '=').collect();
        if v.len() < 2 {
            continue;
        }
        builder = match v[0] {
            "BUILD_DATE" => builder.build_date(v[1]),
            "CATEGORIES" => builder.categories(v[1]),
            "HOMEPAGE" => builder.homepage(v[1]),
            "LICENSE" => builder.license(v[1]),
            "MACHINE_ARCH" => builder.machine_arch(v[1]),
            "OPSYS" => builder.opsys(v[1]),
            "OS_VERSION" => builder.os_version(v[1]),
            "PKG_OPTIONS" => builder.pkg_options(v[1]),
            "PKGPATH" => builder.pkgpath(v[1]),
            "PKGTOOLS_VERSION" => builder.pkgtools_version(v[1]),
            "PREV_PKGPATH" => builder.prev_pkgpath(v[1]),
            _ => builder,
        };
    }

    let desc = pkg.read_metadata(MetadataEntry::Desc)?;
    let desc_lines: Vec<&str> = desc.lines().collect();
    builder = builder.description(desc_lines);

    /*
     * XXX: convert plist Result to summary Result
     */
    let plist = Plist::from_bytes(
        pkg.read_metadata(MetadataEntry::Contents)?.as_bytes(),
    );
    let plist = match plist {
        Ok(p) => p,
        Err(e) => panic!("bad plist: {}", e),
    };

    let deps = plist.depends();
    if !deps.is_empty() {
        builder = builder.depends(deps);
    }

    let conflicts = plist.conflicts();
    if !conflicts.is_empty() {
        builder = builder.conflicts(conflicts);
    }

    let provides: Vec<String> = Vec::new();
    if !provides.is_empty() {
        builder = builder.provides(provides);
    }

    let requires: Vec<String> = Vec::new();
    if !requires.is_empty() {
        builder = builder.requires(requires);
    }

    let sum = builder.build()?;
    println!("{}", sum);

    Ok(())
}

fn main() -> Result<()> {
    let cmd = OptArgs::from_args();
    let mut pkgm: Option<Regex> = None;

    if let Some(m) = cmd.pkgmatch {
        match Regex::new(&m) {
            Ok(p) => pkgm = Some(p),
            Err(e) => panic!("bad regex: {}", e),
        }
    }

    let dbpath = match cmd.pkg_dbdir {
        Some(dir) => dir,
        None => "/opt/pkg/.pkgdb".to_string(),
    };

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
