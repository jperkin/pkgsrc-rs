# pkgsrc-rs

[![Downloads](https://img.shields.io/crates/d/pkgsrc.svg)](https://crates.io/crates/pkgsrc)
[![Crates.io](https://img.shields.io/crates/v/pkgsrc.svg)](https://crates.io/crates/pkgsrc)
[![Documentation](https://docs.rs/pkgsrc/badge.svg)](https://docs.rs/pkgsrc)

A Rust interface to the pkgsrc infrastructure, binary package archives, and the
pkg\_install pkgdb.

This is being developed alongside:

 * [mktool](https://github.com/jperkin/mktool), a collection of tools that
   provide fast alternate implementations for various pkgsrc/mk scripts.
 * [bob](https://github.com/jperkin/bob), a pkgsrc package builder.
 * [pm](https://github.com/jperkin/pm), an exploration of what a binary package
   manager might look like (not currently being developed).

You should expect things to change over time as each interface adapts to better
support these utilities, though I will still make sure to use semver versioning
accordingly to avoid gratuitously breaking downstream utilities.

## Example

This is a simple implementation of `pkg_info(8)` that supports the default
output format, i.e. list all currently installed packages and their single-line
comment.

```rust
use pkgsrc::{MetadataEntry, PkgDB};
use std::path::Path;

fn main() -> Result<(), std::io::Error> {
    let pkgdb = PkgDB::open(Path::new("/var/db/pkg"))?;

    for pkg in pkgdb {
        let pkg = pkg?;
        println!("{:20} {}",
            pkg.pkgname(),
            pkg.read_metadata(MetadataEntry::Comment)?
               .trim()
        );
    }

    Ok(())
}
```

## Status

* pkg\_match() is implemented and verified to be correct against a large input
  of matches.
* Metadata handles "+\*" files contained in an archive and is able to verify
  that the archive contains a valid package.
* PkgDB handles local pkg databases, currently supporting the regular
  file-backed repository, but with flexible support for future sqlite3-backed
  repositories.
* Summary handles pkg\_summary(5) parsing and generation.


# License

This project is licensed under the [ISC](https://opensource.org/licenses/ISC) license.

## Testing/compatibility notes

Generate list of dependency matches.

```bash
sqlite3 /var/db/pkgin/pkgin.db 'SELECT remote_deps_dewey FROM remote_deps' | sort | uniq > pkgdeps.txt
```

Generate list of package names

```bash
sqlite3 /var/db/pkgin/pkgin.db 'SELECT fullpkgname FROM remote_pkg' >pkgnames.txt
```

Implement the following algorithm in both C and Rust and compare output

```bash
while read pattern; do
    while read pkg; do
        pkg_match "${pattern}" "${pkg}"
        printf "%s\t%s\t%s", ${pattern}, ${pkg}, $? >> outfile
    done < pkgnames.txt
done < pkgdeps.txt
```

As an added bonus, the C version took 55 seconds to generate 158,916,879
matches, whilst the Rust version took 42 seconds.
