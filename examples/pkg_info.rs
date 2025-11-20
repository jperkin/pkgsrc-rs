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
use pkgsrc::summary::{self, Summary};
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

fn output_default(pkg: &Package) -> summary::Result<()> {
    println!(
        "{:20} {}",
        pkg.pkgname(),
        pkg.read_metadata(MetadataEntry::Comment)?.trim()
    );
    Ok(())
}

fn output_summary(pkg: &Package) -> summary::Result<()> {
    let mut summary_text = String::new();

    summary_text.push_str(&format!("PKGNAME={}\n", pkg.pkgname()));
    summary_text.push_str(&format!(
        "COMMENT={}\n",
        pkg.read_metadata(MetadataEntry::Comment)?.trim()
    ));
    summary_text.push_str(&format!(
        "SIZE_PKG={}\n",
        pkg.read_metadata(MetadataEntry::SizePkg)?.trim()
    ));
    summary_text.push_str(&pkg.read_metadata(MetadataEntry::BuildInfo)?);

    for line in pkg.read_metadata(MetadataEntry::Desc)?.lines() {
        summary_text.push_str(&format!("DESCRIPTION={}\n", line));
    }

    let sum: Summary = summary_text.parse()?;
    println!("{}", sum);

    Ok(())
}

fn main() -> summary::Result<()> {
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
