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
 */

/*!
 * Pure-Rust equivalent of `pkg_admin rebuild`.
 *
 * Walks the pkgdb directory the same way `pkg_install`'s
 * `iterate_pkg_db` + `add_pkg` do, parses each package's `+CONTENTS`
 * with [`pkgsrc::plist::parse`] (zero-copy), and writes the resulting
 * `pkgdb.byfile.db` through [`db185::Writer`].
 *
 * Two stages pipeline through a bounded channel:
 *
 * * Stage 1 (rayon): per-package reading, plist parsing, and the
 *   `isfile() || islinktodir()` stat checks.  This is where the
 *   syscall and CPU cost lives.
 * * Stage 2 (main thread): apply each package's pre-built operations
 *   to a single [`Writer`], in pkgdb-directory order, so that
 *   cross-package `@pkgdir` merges resolve the same way `pkg_admin`
 *   does.
 *
 * Usage: `pkgdb-byfile-rebuild [<pkgdb-dir>] [-o <out>]`
 *
 * `<pkgdb-dir>` defaults to `/var/db/pkg`.  `-o <out>` defaults to
 * `<pkgdb-dir>/pkgdb.byfile.db`.
 */

use anyhow::{Context, Result, bail};
use db185::Writer;
use hashbrown::HashMap;
use pkgsrc::plist::{PlistEntry, parse};
use rayon::prelude::*;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

fn is_skipped_name(name: &[u8]) -> bool {
    matches!(
        name,
        b"pkgdb.byfile.db" | b".cookie" | b"pkg-vulnerabilities"
    )
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let mut dbdir: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        if arg == "-o" {
            out_path = Some(args.next().context("-o needs a path")?.into());
        } else if dbdir.is_none() {
            dbdir = Some(PathBuf::from(arg));
        } else {
            bail!("unexpected positional argument: {:?}", arg);
        }
    }
    let dbdir = dbdir.unwrap_or_else(|| PathBuf::from("/var/db/pkg"));
    let out_path = out_path.unwrap_or_else(|| dbdir.join("pkgdb.byfile.db"));

    let pkgs = list_packages(&dbdir)?;
    let pkg_count = pkgs.len();

    let _ = fs::remove_file(&out_path);
    let mut writer = Writer::create_new(&out_path)
        .with_context(|| format!("creating {}", out_path.display()))?;
    let mut pkgdirs: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

    let (tx, rx) = mpsc::sync_channel::<(usize, Result<PkgOps>)>(128);
    let dbdir = &dbdir;
    let pkgs = &pkgs;

    std::thread::scope(|s| -> Result<()> {
        s.spawn(move || {
            pkgs.par_iter()
                .enumerate()
                .for_each_with(tx, |tx, (i, pkg)| {
                    let _ = tx.send((i, collect_pkg_ops(dbdir, pkg)));
                });
        });

        let mut buffer: Vec<Option<PkgOps>> =
            (0..pkg_count).map(|_| None).collect();
        let mut next = 0;
        while next < pkg_count {
            let Ok((i, result)) = rx.recv() else {
                bail!("producer hung up at package {next}");
            };
            buffer[i] = Some(result?);
            while next < pkg_count {
                let Some(work) = buffer[next].take() else {
                    break;
                };
                apply_pkg_ops(&mut writer, &mut pkgdirs, work)?;
                next += 1;
            }
        }
        Ok(())
    })?;

    writer.finish().context("finishing writer")?;
    Ok(())
}

struct Package {
    path: PathBuf,
    name: Vec<u8>,
}

fn list_packages(dbdir: &Path) -> Result<Vec<Package>> {
    let mut pkgs = Vec::new();
    for entry in fs::read_dir(dbdir)
        .with_context(|| format!("opening pkgdb {}", dbdir.display()))?
    {
        let entry = entry?;
        let name = entry.file_name();
        if is_skipped_name(name.as_bytes()) {
            continue;
        }
        if !entry.file_type()?.is_dir() {
            continue;
        }
        pkgs.push(Package {
            path: entry.path(),
            name: name.as_bytes().to_vec(),
        });
    }
    Ok(pkgs)
}

enum Op {
    File(Vec<u8>),
    PkgDir(Vec<u8>),
}

struct PkgOps {
    pkgname: Vec<u8>,
    ops: Vec<Op>,
}

fn collect_pkg_ops(dbdir: &Path, pkg: &Package) -> Result<PkgOps> {
    let contents_path = pkg.path.join("+CONTENTS");
    let contents = fs::read(&contents_path)
        .with_context(|| format!("reading {}", contents_path.display()))?;

    let mut ops: Vec<Op> = Vec::new();
    let mut key_buf: Vec<u8> = Vec::with_capacity(256);
    let mut cwd_buf: Vec<u8> = Vec::with_capacity(128);
    let mut cwd_set = false;
    let mut iter = parse(&contents);

    while let Some(directive) = iter.next() {
        let directive = directive.with_context(|| {
            format!("parsing plist for {}", String::from_utf8_lossy(&pkg.name))
        })?;
        match directive {
            PlistEntry::Cwd(d) => {
                cwd_buf.clear();
                let d_bytes = d.as_os_str().as_bytes();
                if d_bytes == b"." {
                    cwd_buf.extend_from_slice(dbdir.as_os_str().as_bytes());
                    cwd_buf.push(b'/');
                    cwd_buf.extend_from_slice(&pkg.name);
                } else {
                    cwd_buf.extend_from_slice(d_bytes);
                }
                cwd_set = true;
            }
            PlistEntry::Ignore => {
                iter.next();
            }
            PlistEntry::File(name) => {
                if !cwd_set {
                    bail!(
                        "@cwd not set before file entry in {}",
                        String::from_utf8_lossy(&pkg.name)
                    );
                }
                build_key(&mut key_buf, &cwd_buf, name.as_os_str().as_bytes());
                let on_disk = OsStr::from_bytes(&key_buf[..key_buf.len() - 1]);
                if !should_store(Path::new(on_disk)) {
                    continue;
                }
                ops.push(Op::File(key_buf.clone()));
            }
            PlistEntry::PkgDir(name) => {
                if !cwd_set {
                    bail!(
                        "@cwd not set before pkgdir entry in {}",
                        String::from_utf8_lossy(&pkg.name)
                    );
                }
                build_key(&mut key_buf, &cwd_buf, name.as_os_str().as_bytes());
                ops.push(Op::PkgDir(key_buf.clone()));
            }
            _ => {}
        }
    }

    Ok(PkgOps {
        pkgname: pkg.name.clone(),
        ops,
    })
}

fn build_key(buf: &mut Vec<u8>, cwd: &[u8], name: &[u8]) {
    buf.clear();
    buf.reserve(cwd.len() + 1 + name.len() + 1);
    buf.extend_from_slice(cwd);
    buf.push(b'/');
    buf.extend_from_slice(name);
    buf.push(0);
}

/**
 * Mirror `pkg_install`'s `isfile(p) || islinktodir(p)` from
 * `lib/file.c`.
 */
fn should_store(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if meta.is_file() {
        return true;
    }
    if meta.is_dir() {
        return matches!(
            fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()),
            Ok(true)
        );
    }
    false
}

fn apply_pkg_ops(
    writer: &mut Writer,
    pkgdirs: &mut HashMap<Vec<u8>, Vec<u8>>,
    work: PkgOps,
) -> Result<()> {
    let mut value = Vec::with_capacity(work.pkgname.len() + 1);
    value.extend_from_slice(&work.pkgname);
    value.push(0);

    for op in work.ops {
        match op {
            Op::File(key) => {
                if !writer.put(&key, &value)? {
                    bail!(
                        "pkgdb collision: {} already stored",
                        String::from_utf8_lossy(&key)
                    );
                }
            }
            Op::PkgDir(key) => {
                let new_val = match pkgdirs.get(&key) {
                    Some(old) => {
                        writer.del(&key)?;
                        let mut v = Vec::with_capacity(
                            old.len() + 1 + work.pkgname.len(),
                        );
                        v.extend_from_slice(&old[..old.len() - 1]);
                        v.push(b' ');
                        v.extend_from_slice(&work.pkgname);
                        v.push(0);
                        v
                    }
                    None => {
                        let mut v =
                            Vec::with_capacity(8 + work.pkgname.len() + 1);
                        v.extend_from_slice(b"@pkgdir ");
                        v.extend_from_slice(&work.pkgname);
                        v.push(0);
                        v
                    }
                };
                writer.put(&key, &new_val)?;
                pkgdirs.insert(key, new_val);
            }
        }
    }
    Ok(())
}
