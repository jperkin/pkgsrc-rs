/*
 * Copyright (c) 2024 Jonathan Perkin <jonathan@perkin.org.uk>
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

use pkgsrc::Pattern;
use std::env;
use std::fs;
use std::io::BufRead;

fn main() {
    if env::args().len() != 3 {
        eprintln!("usage: test-pkgmatch <pkgdeps.txt> <pkgnames.txt>");
        std::process::exit(1);
    }

    let pkgdeps = fs::read(env::args().nth(1).unwrap()).unwrap();
    let pkgnames = fs::read(env::args().nth(2).unwrap()).unwrap();

    let mut deps = vec![];
    let mut pkgs = vec![];

    for dep in pkgdeps.lines() {
        let dep = dep.unwrap();
        deps.push(dep);
    }

    for pkg in pkgnames.lines() {
        let pkg = pkg.unwrap();
        pkgs.push(pkg);
    }

    for dep in &deps {
        let m = Pattern::new(dep).unwrap();
        for pkg in &pkgs {
            if m.matches(pkg) {
                println!("{dep} {pkg}");
            }
        }
    }
}
