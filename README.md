# pkgsrc-rs

[![Build Status](https://travis-ci.org/jperkin/pkgsrc-rs.svg?branch=master)](https://travis-ci.org/jperkin/pkgsrc-rs)
[![Crates.io](https://img.shields.io/crates/v/pkgsrc.svg?maxAge=2592000)](https://crates.io/crates/pkgsrc)
[![Documentation](https://docs.rs/pkgsrc/badge.svg)](https://docs.rs/pkgsrc)

A Rust interface to pkgsrc packages and the pkg\_install pkgdb.

This is being developed alongside [pm](https://github.com/jperkin/pm), a Rust
implementation of a pkgsrc package manager.  Anything that handles lower level
pkg\_install routines will be placed here.

## Status

* pkg\_match() is implemented and verified to be correct against a large input
  of matches.
* Metadata handles "+\*" files contained in an archive and is able to verify
  that the archive contains a valid package.
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
