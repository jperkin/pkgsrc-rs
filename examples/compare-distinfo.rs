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
 *
 * Utility to verify that parsing and writing a distinfo file results in the
 * same contents as the original.  If not a rudimentary diff is printed.
 */

use pkgsrc::distinfo::Distinfo;
use std::env;
use std::fs;

fn main() {
    for arg in env::args().skip(1) {
        let input = match fs::read(&arg) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("WARNING: Could not open {}: {}", arg, e);
                continue;
            }
        };
        let distinfo = Distinfo::from_bytes(&input);
        let output = distinfo.as_bytes();
        if input != output {
            eprintln!("ERROR: {}: contents differ!", arg);
            for (bi, bo) in input
                .split(|c| *c == b'\n')
                .zip(output.split(|c| *c == b'\n'))
            {
                if bi != bo {
                    eprintln!(">>>");
                    eprintln!("{}", String::from_utf8_lossy(bi));
                    eprintln!("===");
                    eprintln!("{}", String::from_utf8_lossy(bo));
                    eprintln!("<<<");
                }
            }
            eprintln!();
        }
    }
}
