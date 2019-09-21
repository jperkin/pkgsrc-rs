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
use pkgsrc::summary::{Result, Summary, SummaryVariable};
use pkgsrc::MetadataEntry;
use regex::Regex;
use std::path::Path;
use std::str::FromStr;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "pkg_info", about = "An example pkg_info(8) command")]
pub struct OptArgs {
    #[structopt(short = "K", long = "pkg-dbdir", help = "Set PKG_DBDIR")]
    pkg_dbdir: Option<String>,
    #[structopt(short = "v", long = "verbose", help = "Enable verbose output")]
    verbose: bool,
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
    let mut sum = Summary::new();
    sum.set_pkgname(pkg.pkgname());
    sum.set_comment(pkg.read_metadata(MetadataEntry::Comment)?.trim());
    sum.set_size_pkg(
        pkg.read_metadata(MetadataEntry::SizePkg)?
            .trim()
            .parse::<u64>()?,
    );
    let bi = pkg.read_metadata(MetadataEntry::BuildInfo)?;
    for line in bi.lines() {
        let v: Vec<&str> = line.splitn(2, '=').collect();
        let key = match SummaryVariable::from_str(v[0]) {
            Ok(k) => k,
            Err(_) => continue,
        };
        match key {
            SummaryVariable::BuildDate => sum.set_build_date(v[1]),
            SummaryVariable::Categories => sum.set_categories(v[1]),
            SummaryVariable::Homepage => sum.set_homepage(v[1]),
            SummaryVariable::License => sum.set_license(v[1]),
            SummaryVariable::MachineArch => sum.set_machine_arch(v[1]),
            SummaryVariable::Opsys => sum.set_opsys(v[1]),
            SummaryVariable::OsVersion => sum.set_os_version(v[1]),
            SummaryVariable::PkgOptions => sum.set_pkg_options(v[1]),
            SummaryVariable::Pkgpath => sum.set_pkgpath(v[1]),
            SummaryVariable::PkgtoolsVersion => sum.set_pkgtools_version(v[1]),
            SummaryVariable::PrevPkgpath => sum.set_prev_pkgpath(v[1]),
            SummaryVariable::Provides => sum.push_provides(v[1]),
            SummaryVariable::Requires => sum.push_requires(v[1]),
            SummaryVariable::Supersedes => sum.push_supersedes(v[1]),
            _ => {}
        }
    }
    for line in pkg.read_metadata(MetadataEntry::Desc)?.lines() {
        sum.push_description(line);
    }

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
    for dep in plist.depends() {
        sum.push_depends(dep);
    }
    for cfl in plist.conflicts() {
        sum.push_conflicts(cfl);
    }

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
