## pkgsrc-rs - A Rust interface to pkg_install packages and database

This is being developed alongside [pm](https://github.com/jperkin/pm), a Rust
implementation of a pkgsrc package manager.  Anything that handles lower level
pkg\_install routines will be placed here.

### Status

* pkg\_match() is implemented and verified to be correct against a large input
  of matches.
* MetaData handles "+\*" files contained in an archive and is able to verify
  that the archive contains a valid package.
* Summary handles pkg\_summary(5) parsing and generation.

### Testing notes

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
