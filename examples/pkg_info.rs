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

use pkgsrc::{MetadataEntry, PkgDB};
use std::path::Path;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "pkg_info", about = "An example pkg_info(8) command")]
pub struct OptArgs {
    #[structopt(short = "K", long = "pkg-dbdir", help = "Set PKG_DBDIR")]
    pkg_dbdir: Option<String>,
    #[structopt(short = "v", long = "verbose", help = "Enable verbose output")]
    verbose: bool,
}

fn main() -> Result<(), std::io::Error> {
    let cmd = OptArgs::from_args();

    let dbpath = match cmd.pkg_dbdir {
        Some(dir) => dir,
        None => "/opt/pkg/.pkgdb".to_string(),
    };

    let pkgdb = PkgDB::open(Path::new(&dbpath))?;

    for pkg in pkgdb {
        let pkg = pkg?;
        println!(
            "{:20} {}",
            pkg.pkgname(),
            pkg.read_metadata(MetadataEntry::Comment)?.trim()
        );
    }

    Ok(())
}
