## pkgsrc-rs - A Rust interface to pkg_install packages and database

This is being developed alongside [pm](https://github.com/jperkin/pm), a Rust
implementation of a pkgsrc package manager.  Anything that handles lower level
pkg\_install routines will be placed here.

### Usage

```rust
use pkgsrc::pmatch::pkg_match;

// simple match
assert_eq!(pkg_match("foobar-1.0", "foobar-1.0"), true);
assert_eq!(pkg_match("foobar-1.0", "foobar-1.1"), false);

// dewey comparisons
assert_eq!(pkg_match("foobar>=1.0", "foobar-1.1"), true);
assert_eq!(pkg_match("foobar>=1.1", "foobar-1.0"), false);

// alternate matches
assert_eq!(pkg_match("{foo,bar}>=1.0", "foo-1.1"), true);
assert_eq!(pkg_match("{foo,bar}>=1.0", "bar-1.1"), true);
assert_eq!(pkg_match("{foo,bar}>=1.0", "moo-1.1"), false);

// globs
assert_eq!(pkg_match("foo-[0-9]*", "foo-1.0"), true);
assert_eq!(pkg_match("fo?-[0-9]*", "foo-1.0"), true);
assert_eq!(pkg_match("fo*-[0-9]*", "foobar-1.0"), true);
```

### Status

* pkg\_match() is implemented and verified to be correct against a large input
  of matches using the following procedure:

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
