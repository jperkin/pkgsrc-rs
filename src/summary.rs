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
 */

/*!
 * [`pkg_summary(5)`] parsing and generation.
 *
 * A pkg_summary file contains a selection of useful package metadata, and is
 * primarily used by binary package managers to configure package repositories.
 *
 * A package entry in pkg_summary contains a list of `VARIABLE=VALUE` pairs,
 * and a complete pkg_summary file consists of multiple package entries
 * separated by a single blank line.
 *
 * This module supports parsing a pkg_summary file, for example if configuring
 * a remote repository, as well as generating pkg_summary output from a binary
 * package or local repository.
 *
 * A [`Summary`] is a complete entry for a package.  It implements the
 * [`FromStr`] trait which is the primary method of parsing an existing
 * pkg_summary entry.  A new [`Summary`] can also be created from package
 * metadata, using various functions that are provided.
 *
 * A collection of [`Summary`] entries can be stored in a [`Summaries`] struct,
 * thus containing a complete pkg_summary file for a package repository.
 * [`Summaries`] implements both [`FromStr`] and [`Write`], so can easily be
 * populated by streaming in a pkg_summary file.
 *
 * ## Examples
 *
 * ### Read pkg_summary
 *
 * Read the generated pkg_summary output from `pkg_info -Xa` and parse into a
 * new [`Summaries`] containing a [`Summary`] for each entry.
 *
 * ```
 * use pkgsrc::summary;
 * use std::io::BufReader;
 * use std::process::{Command, Stdio};
 *
 * fn main() -> summary::Result<()> {
 *     let mut pkgsum = summary::Summaries::new();
 *
 *     /*
 *      * Read "pkg_info -Xa" output into a buffer.
 *      */
 *     let pkg_info = Command::new("/opt/pkg/sbin/pkg_info")
 *         .args(&["-X", "-a"])
 *         .stdout(Stdio::piped())
 *         .spawn()
 *         .expect("could not spawn pkg_info");
 *     let mut pkg_info = BufReader::new(pkg_info.stdout.expect("failed"));
 *
 *     /*
 *      * Summaries implements the Write trait, so copying the data in will
 *      * parse it into separate Summary entries and return a Result.
 *      */
 *     std::io::copy(&mut pkg_info, &mut pkgsum)?;
 *
 *     /*
 *      * We have a complete pkg_summary, let's emulate "pkg_info".  Note that
 *      * each Summary entry will have been validated to ensure all required
 *      * entries exist, so it's safe to unwrap those.
 *      */
 *     for pkg in pkgsum.entries() {
 *         println!("{:20} {}", pkg.pkgname().unwrap(), pkg.comment().unwrap());
 *     }
 *
 *     Ok(())
 * }
 * ```
 *
 * ### Generate a pkg_summary entry
 *
 * Create a [`Summary`] entry from package metadata.  Here we only set the
 * minimum required fields, using `is_completed()` to check for validity.
 *
 * ```
 * use pkgsrc::summary::Summary;
 *
 * let mut sum = Summary::new();
 *
 * assert_eq!(sum.is_completed(), false);
 *
 * sum.set_build_date("2019-08-12 15:58:02 +0100");
 * sum.set_categories("devel pkgtools");
 * sum.set_comment("This is a test");
 * sum.set_description(&["A test description".to_string(),
 *                       "".to_string(),
 *                       "This is a multi-line variable".to_string()]);
 * sum.set_machine_arch("x86_64");
 * sum.set_opsys("Darwin");
 * sum.set_os_version("18.7.0");
 * sum.set_pkgname("testpkg-1.0");
 * sum.set_pkgpath("pkgtools/testpkg");
 * sum.set_pkgtools_version("20091115");
 * sum.set_size_pkg(4321);
 *
 * assert_eq!(sum.is_completed(), true);
 *
 * /*
 *  * With the Display trait implemented we can simply print the Summary and
 *  * it will be output in the correct format, i.e.
 *  *
 *  * BUILD_DATE=2019-08-12 15:58:02 +0100
 *  * CATEGORIES=devel pkgtools
 *  * COMMENT=This is a test
 *  * DESCRIPTION=A test description
 *  * DESCRIPTION=
 *  * DESCRIPTION=This is a multi-line variable
 *  * ...
 *  */
 * println!("{}", sum);
 * ```
 *
 * [`Entry`]: ../summary/enum.Entry.html
 * [`Summary`]: ../summary/struct.Summary.html
 * [`Summaries`]: ../summary/struct.Summaries.html
 * [`Display`]: https://doc.rust-lang.org/std/fmt/trait.Display.html
 * [`FromStr`]: https://doc.rust-lang.org/std/str/trait.FromStr.html
 * [`Write`]: https://doc.rust-lang.org/std/io/trait.Write.html
 * [`pkg_summary(5)`]: https://netbsd.gw.com/cgi-bin/man-cgi?pkg_summary+5
 */
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt;
use std::io;
use std::io::Write;
use std::num::ParseIntError;
use std::str::FromStr;

#[cfg(test)]
use unindent::unindent;

/**
 * A type alias for the result from the creation of either a [`Summary`] or a
 * [`Entry`], with [`Error`] returned in [`Err`] variants.
 *
 * [`Summary`]: ../summary/struct.Summary.html
 * [`Entry`]: ../summary/enum.Entry.html
 * [`Error`]: ../summary/enum.Error.html
 * [`Err`]: https://doc.rust-lang.org/std/result/enum.Result.html#variant.Err
 */
pub type Result<T> = std::result::Result<T, SummaryError>;

/**
 * Supported [`pkg_summary(5)`] variables.
 *
 * The descriptions here are taken straight from the manual page.
 *
 * [`pkg_summary(5)`]: https://netbsd.gw.com/cgi-bin/man-cgi?pkg_summary+5
 */
#[derive(Debug, Ord, PartialOrd, PartialEq, Eq, Hash)]
pub enum SummaryVariable {
    /**
     * `BUILD_DATE` (required).  The date and time when the package was built.
     */
    BuildDate,
    /**
     * `CATEGORIES` (required).  A list of categories which this package fits
     * in, separated by space.
     */
    Categories,
    /**
     * `COMMENT` (required).  A one-line description of the package.
     */
    Comment,
    /**
     * `CONFLICTS` (optional).  A list of dewey patterns of packages the
     * package conflicts with, one per line.  If missing, this package has no
     * conflicts.
     */
    Conflicts,
    /**
     * `DEPENDS` (optional).  A list of dewey patterns of packages the package
     * depends on, one per line.  If missing, this package has no dependencies.
     */
    Depends,
    /**
     * `DESCRIPTION` (required).  A more detailed description of the package.
     */
    Description,
    /**
     * `FILE_CKSUM` (optional).  A checksum type supported by digest(1) and
     * checksum separated by space character.
     */
    FileCksum,
    /**
     * `FILE_NAME` (optional).  The name of the binary package file.  If not
     * given, PKGNAME.tgz can be assumed.
     */
    FileName,
    /**
     * `FILE_SIZE` (optional).  The size of the binary package file, in bytes.
     */
    FileSize,
    /**
     * `HOMEPAGE` (optional).  A URL where more information about the package
     * can be found.
     */
    Homepage,
    /**
     * `LICENSE` (optional).  The type of license this package is distributed
     * under.  If empty or missing, it is OSI-approved.
     */
    License,
    /**
     * `MACHINE_ARCH` (required).  The architecture on which the package was
     * compiled.
     */
    MachineArch,
    /**
     * `OPSYS` (required).  The operating system on which the package was
     * compiled
     */
    Opsys,
    /**
     * `OS_VERSION` (required).  The version of the operating system on which
     * the package was compiled.
     */
    OsVersion,
    /**
     * `PKG_OPTIONS` (optional).  Any options selected to compile this package.
     * If missing, the package does not support options.
     */
    PkgOptions,
    /**
     * `PKGNAME` (required).  The name of the package.
     */
    Pkgname,
    /**
     * `PKGPATH` (required).  The path of the package directory within pkgsrc.
     */
    Pkgpath,
    /**
     * `PKGTOOLS_VERSION` (required).  The version of the package tools used to
     * create the package.
     */
    PkgtoolsVersion,
    /**
     * `PREV_PKGPATH` (optional).  The previous path of the package directory
     * within pkgsrc when a package was moved.  (See SUPERSEDES below for a
     * renamed package.)
     */
    PrevPkgpath,
    /**
     * `PROVIDES` (optional).  A list of shared libraries provided by the
     * package, including major version number, one per line.  If missing, this
     * package does not provide shared libraries.
     */
    Provides,
    /**
     * `REQUIRES` (optional).  A list of shared libraries needed by the
     * package, including major version number, one per line.  If missing, this
     * package does not require shared libraries.
     */
    Requires,
    /**
     * `SIZE_PKG` (required).  The size of the package when installed, in
     * bytes.
     */
    SizePkg,
    /**
     * `SUPERSEDES` (optional).  A list of dewey patterns of previous packages
     * this package replaces, one per line.  This is used for package renaming.
     */
    Supersedes,
}

/*
 * Convert from pkg_summary variables to their SummaryVariable equivalents.
 */
impl FromStr for SummaryVariable {
    type Err = SummaryError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "BUILD_DATE" => Ok(SummaryVariable::BuildDate),
            "FILE_SIZE" => Ok(SummaryVariable::FileSize),
            "CATEGORIES" => Ok(SummaryVariable::Categories),
            "COMMENT" => Ok(SummaryVariable::Comment),
            "CONFLICTS" => Ok(SummaryVariable::Conflicts),
            "DEPENDS" => Ok(SummaryVariable::Depends),
            "DESCRIPTION" => Ok(SummaryVariable::Description),
            "FILE_CKSUM" => Ok(SummaryVariable::FileCksum),
            "FILE_NAME" => Ok(SummaryVariable::FileName),
            "HOMEPAGE" => Ok(SummaryVariable::Homepage),
            "LICENSE" => Ok(SummaryVariable::License),
            "MACHINE_ARCH" => Ok(SummaryVariable::MachineArch),
            "OPSYS" => Ok(SummaryVariable::Opsys),
            "OS_VERSION" => Ok(SummaryVariable::OsVersion),
            "PKG_OPTIONS" => Ok(SummaryVariable::PkgOptions),
            "PKGNAME" => Ok(SummaryVariable::Pkgname),
            "PKGPATH" => Ok(SummaryVariable::Pkgpath),
            "PKGTOOLS_VERSION" => Ok(SummaryVariable::PkgtoolsVersion),
            "PREV_PKGPATH" => Ok(SummaryVariable::PrevPkgpath),
            "PROVIDES" => Ok(SummaryVariable::Provides),
            "REQUIRES" => Ok(SummaryVariable::Requires),
            "SIZE_PKG" => Ok(SummaryVariable::SizePkg),
            "SUPERSEDES" => Ok(SummaryVariable::Supersedes),
            _ => Err(SummaryError::ParseVariable(s.to_string())),
        }
    }
}

/**
 * Valid pkg_summary(5) value types.
 */
#[derive(Debug, PartialEq, Eq, Hash)]
enum SummaryValue {
    /**
     * A single string.
     */
    S(String),
    /**
     * A single integer.
     */
    I(u64),
    /**
     * An array of strings.
     */
    A(Vec<String>),
}

impl SummaryValue {
    /*
     * Push a new value onto an existing A().
     */
    fn push(&mut self, val: &SummaryValue) {
        let v = match val {
            SummaryValue::A(s) => s,
            _ => panic!("pushing only supported on A()"),
        };

        match self {
            SummaryValue::A(s) => s.extend_from_slice(&v),
            _ => panic!("pushing only supported on A()"),
        }
    }
}

impl fmt::Display for SummaryValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SummaryValue::S(s) => write!(f, "{}", s),
            SummaryValue::I(i) => write!(f, "{}", i),
            SummaryValue::A(s) => write!(f, "{}", s.join("\n")),
        }
    }
}

/*
 * Note that (as far as my reading of it suggests) we cannot return an error
 * via fmt::Result if there are any issues with missing fields, so we can only
 * print what we have and validation will have to occur elsewhere.
 */
impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        /*
         * HashMaps are stored in arbitrary order so we first copy the data
         * into a BTreeMap which preserves the SummaryVariable ordering ready
         * for printing.
         */
        let mut bmap = BTreeMap::new();
        for (key, val) in &self.entries {
            bmap.insert(key, val);
        }
        for (key, val) in bmap {
            match val {
                SummaryValue::S(s) => writeln!(f, "{}={}", key, s),
                SummaryValue::I(i) => writeln!(f, "{}={}", key, i),
                SummaryValue::A(a) => {
                    for s in a.iter() {
                        writeln!(f, "{}={}", key, s)?;
                    }
                    Ok(())
                }
            }?;
        }
        Ok(())
    }
}

/**
 * A complete [`pkg_summary(5)`] entry.
 *
 * ## Example
 *
 * ```
 * use pkgsrc::summary::Summary;
 *
 * let mut sum = Summary::new();
 *
 * assert_eq!(sum.is_completed(), false);
 *
 * sum.set_build_date("2019-08-12 15:58:02 +0100");
 * sum.set_categories("devel pkgtools");
 * sum.set_comment("This is a test");
 * sum.set_description(&["A test description".to_string(),
 *                       "".to_string(),
 *                       "This is a multi-line variable".to_string()]);
 * sum.set_machine_arch("x86_64");
 * sum.set_opsys("Darwin");
 * sum.set_os_version("18.7.0");
 * sum.set_pkgname("testpkg-1.0");
 * sum.set_pkgpath("pkgtools/testpkg");
 * sum.set_pkgtools_version("20091115");
 * sum.set_size_pkg(4321);
 *
 * assert_eq!(sum.is_completed(), true);
 *
 * /*
 *  * With the Display trait implemented we can simply print the Summary and
 *  * it will be output in the correct format, i.e.
 *  *
 *  * BUILD_DATE=2019-08-12 15:58:02 +0100
 *  * CATEGORIES=devel pkgtools
 *  * COMMENT=This is a test
 *  * DESCRIPTION=A test description
 *  * DESCRIPTION=
 *  * DESCRIPTION=This is a multi-line variable
 *  * ...
 *  */
 * println!("{}", sum);
 * ```
 *
 * [`pkg_summary(5)`]: https://netbsd.gw.com/cgi-bin/man-cgi?pkg_summary+5
 */
#[derive(Debug, Default)]
pub struct Summary {
    entries: HashMap<SummaryVariable, SummaryValue>,
}

impl Summary {
    /**
     * Create a new empty Summary.
     */
    pub fn new() -> Summary {
        let s: Summary = Default::default();
        s
    }

    /**
     * Indicate whether all of the required variables have been set for this
     * entry.
     */
    pub fn is_completed(&self) -> bool {
        if self.build_date().is_none()
            || self.categories().is_none()
            || self.comment().is_none()
            || self.description().is_none()
            || self.machine_arch().is_none()
            || self.opsys().is_none()
            || self.os_version().is_none()
            || self.pkgname().is_none()
            || self.pkgpath().is_none()
            || self.pkgtools_version().is_none()
            || self.size_pkg().is_none()
        {
            return false;
        }
        true
    }

    /*
     * Get S() I() and A() entries out of their SummaryValue wrappers.  Any
     * bad matches are incorrect internal usage issues and must panic.
     */
    fn get_s(&self, var: SummaryVariable) -> Option<&str> {
        match &self.entries.get(&var) {
            Some(entry) => match entry {
                SummaryValue::S(s) => Some(&s),
                _ => panic!("internal error"),
            },
            None => None,
        }
    }
    fn get_i(&self, var: SummaryVariable) -> Option<u64> {
        match &self.entries.get(&var) {
            Some(entry) => match entry {
                SummaryValue::I(i) => Some(*i),
                _ => panic!("internal error"),
            },
            None => None,
        }
    }
    fn get_a(&self, var: SummaryVariable) -> Option<&[String]> {
        match &self.entries.get(&var) {
            Some(entry) => match entry {
                SummaryValue::A(a) => Some(a),
                _ => panic!("internal error"),
            },
            None => None,
        }
    }

    /*
     * There's probably a fancy way to do these all in one operation, but I
     * couldn't yet figure it out (e.g. adding .or_insert() fails the borrow
     * checker as we use val twice, even though logically we aren't).
     */
    fn insert_or_update(&mut self, var: SummaryVariable, val: SummaryValue) {
        match self.entries.entry(var) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                *entry.get_mut() = val;
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(val);
            }
        }
        //self.entries.entry(var).and_modify(|e| *e = val).or_insert(val);
        /*
        if self.entries.contains_key(&var) {
            self.entries.entry(var).and_modify(|e| *e = val);
        } else {
            self.entries.insert(var, val);
        }
        */
    }

    fn insert_or_push(&mut self, var: SummaryVariable, val: SummaryValue) {
        self.entries
            .entry(var)
            .and_modify(|e| e.push(&val))
            .or_insert(val);
    }

    /**
     * Returns the [`BuildDate`] value, if set.  This is a required field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let build_date = String::from("2019-08-12 15:58:02 +0100");
     *
     * sum.set_build_date(build_date.as_str());
     *
     * assert_eq!(Some(build_date.as_str()), sum.build_date());
     * ```
     *
     * [`BuildDate`]: ../summary/enum.SummaryVariable.html#variant.BuildDate
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn build_date(&self) -> Option<&str> {
        self.get_s(SummaryVariable::BuildDate)
    }

    /**
     * Returns the [`Categories`] value, if set.  This is a required field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let categories = String::from("devel pkgtools");
     *
     * sum.set_categories(categories.as_str());
     *
     * assert_eq!(Some(categories.as_str()), sum.categories());
     * ```
     *
     * [`Categories`]: ../summary/enum.SummaryVariable.html#variant.Categories
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn categories(&self) -> Option<&str> {
        self.get_s(SummaryVariable::Categories)
    }

    /**
     * Returns the [`Comment`] value, if set.  This is a required field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let comment = String::from("This is a test");
     *
     * sum.set_comment(comment.as_str());
     *
     * assert_eq!(Some(comment.as_str()), sum.comment());
     * ```
     *
     * [`Comment`]: ../summary/enum.SummaryVariable.html#variant.Comment
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn comment(&self) -> Option<&str> {
        self.get_s(SummaryVariable::Comment)
    }

    /**
     * Returns the [`Conflicts`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let conflicts = vec![
     *                     String::from("cfl-pkg1-[0-9]*"),
     *                     String::from("cfl-pkg2>=2.0"),
     *                 ];
     *
     * sum.set_conflicts(conflicts.as_slice());
     *
     * assert_eq!(Some(conflicts.as_slice()), sum.conflicts());
     * ```
     *
     * [`Conflicts`]: ../summary/enum.SummaryVariable.html#variant.Conflicts
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn conflicts(&self) -> Option<&[String]> {
        self.get_a(SummaryVariable::Conflicts)
    }

    /**
     * Returns the [`Depends`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let depends = vec![
     *                     String::from("dep-pkg1-[0-9]*"),
     *                     String::from("dep-pkg2>=2.0"),
     *                 ];
     *
     * sum.set_depends(depends.as_slice());
     *
     * assert_eq!(Some(depends.as_slice()), sum.depends());
     * ```
     *
     * [`Depends`]: ../summary/enum.SummaryVariable.html#variant.Depends
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn depends(&self) -> Option<&[String]> {
        self.get_a(SummaryVariable::Depends)
    }

    /**
     * Returns the [`Description`] value, if set.  This is a required field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let description = vec![
     *                       String::from("This is a test"),
     *                       String::from(""),
     *                       String::from("This is a multi-line variable"),
     *                   ];
     *
     * sum.set_description(description.as_slice());
     *
     * assert_eq!(Some(description.as_slice()), sum.description());
     * ```
     *
     * [`Description`]: ../summary/enum.SummaryVariable.html#variant.Description
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn description(&self) -> Option<&[String]> {
        self.get_a(SummaryVariable::Description)
    }

    /**
     * Returns the [`FileCksum`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let cksum = String::from("SHA1 a4801e9b26eeb5b8bd1f54bac1c8e89dec67786a");
     *
     * sum.set_file_cksum(cksum.as_str());
     *
     * assert_eq!(Some(cksum.as_str()), sum.file_cksum());
     * ```
     *
     * [`FileCksum`]: ../summary/enum.SummaryVariable.html#variant.FileCksum
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn file_cksum(&self) -> Option<&str> {
        self.get_s(SummaryVariable::FileCksum)
    }

    /**
     * Returns the [`FileName`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let filename = String::from("testpkg-1.0.tgz");
     *
     * sum.set_file_name(filename.as_str());
     *
     * assert_eq!(Some(filename.as_str()), sum.file_name());
     * ```
     *
     * [`FileName`]: ../summary/enum.SummaryVariable.html#variant.FileName
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn file_name(&self) -> Option<&str> {
        self.get_s(SummaryVariable::FileName)
    }

    /**
     * Returns the [`FileSize`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let filesize = 1234;
     *
     * sum.set_file_size(filesize);
     *
     * assert_eq!(Some(filesize), sum.file_size());
     * ```
     *
     * [`FileSize`]: ../summary/enum.SummaryVariable.html#variant.FileSize
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn file_size(&self) -> Option<u64> {
        self.get_i(SummaryVariable::FileSize)
    }

    /**
     * Returns the [`Homepage`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let homepage = String::from("https://docs.rs/pkgsrc/");
     *
     * sum.set_homepage(homepage.as_str());
     *
     * assert_eq!(Some(homepage.as_str()), sum.homepage());
     * ```
     *
     * [`Homepage`]: ../summary/enum.SummaryVariable.html#variant.Homepage
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn homepage(&self) -> Option<&str> {
        self.get_s(SummaryVariable::Homepage)
    }

    /**
     * Returns the [`License`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let license = String::from("apache-2.0 OR modified-bsd");
     *
     * sum.set_license(license.as_str());
     *
     * assert_eq!(Some(license.as_str()), sum.license());
     * ```
     *
     * [`License`]: ../summary/enum.SummaryVariable.html#variant.License
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn license(&self) -> Option<&str> {
        self.get_s(SummaryVariable::License)
    }

    /**
     * Returns the [`MachineArch`] value, if set.  This is a required field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let machine_arch = String::from("x86_64");
     *
     * sum.set_machine_arch(machine_arch.as_str());
     *
     * assert_eq!(Some(machine_arch.as_str()), sum.machine_arch());
     * ```
     *
     * [`MachineArch`]: ../summary/enum.SummaryVariable.html#variant.MachineArch
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn machine_arch(&self) -> Option<&str> {
        self.get_s(SummaryVariable::MachineArch)
    }

    /**
     * Returns the [`Opsys`] value, if set.  This is a required field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let opsys = String::from("Darwin");
     *
     * sum.set_opsys(opsys.as_str());
     *
     * assert_eq!(Some(opsys.as_str()), sum.opsys());
     * ```
     *
     * [`Opsys`]: ../summary/enum.SummaryVariable.html#variant.Opsys
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn opsys(&self) -> Option<&str> {
        self.get_s(SummaryVariable::Opsys)
    }

    /**
     * Returns the [`OsVersion`] value, if set.  This is a required field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let os_version = String::from("18.7.0");
     *
     * sum.set_os_version(os_version.as_str());
     *
     * assert_eq!(Some(os_version.as_str()), sum.os_version());
     * ```
     *
     * [`OsVersion`]: ../summary/enum.SummaryVariable.html#variant.OsVersion
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn os_version(&self) -> Option<&str> {
        self.get_s(SummaryVariable::OsVersion)
    }

    /**
     * Returns the [`PkgOptions`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let pkg_options = String::from("http2 idn inet6 ldap libssh2");
     *
     * sum.set_pkg_options(pkg_options.as_str());
     *
     * assert_eq!(Some(pkg_options.as_str()), sum.pkg_options());
     * ```
     *
     * [`PkgOptions`]: ../summary/enum.SummaryVariable.html#variant.PkgOptions
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn pkg_options(&self) -> Option<&str> {
        self.get_s(SummaryVariable::PkgOptions)
    }

    /**
     * Returns the [`Pkgname`] value, if set.  This is a required field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let pkgname = String::from("testpkg-1.0");
     *
     * sum.set_pkgname(pkgname.as_str());
     *
     * assert_eq!(Some(pkgname.as_str()), sum.pkgname());
     * ```
     *
     * [`Pkgname`]: ../summary/enum.SummaryVariable.html#variant.Pkgname
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn pkgname(&self) -> Option<&str> {
        self.get_s(SummaryVariable::Pkgname)
    }

    /**
     * Returns the [`Pkgpath`] value, if set.  This is a required field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let pkgpath = String::from("pkgtools/testpkg");
     *
     * sum.set_pkgpath(pkgpath.as_str());
     *
     * assert_eq!(Some(pkgpath.as_str()), sum.pkgpath());
     * ```
     *
     * [`Pkgpath`]: ../summary/enum.SummaryVariable.html#variant.Pkgpath
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn pkgpath(&self) -> Option<&str> {
        self.get_s(SummaryVariable::Pkgpath)
    }

    /**
     * Returns the [`PkgtoolsVersion`] value, if set.  This is a required field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let pkgtools_version = String::from("20091115");
     *
     * sum.set_pkgtools_version(pkgtools_version.as_str());
     *
     * assert_eq!(Some(pkgtools_version.as_str()), sum.pkgtools_version());
     * ```
     *
     * [`PkgtoolsVersion`]: ../summary/enum.SummaryVariable.html#variant.PkgtoolsVersion
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn pkgtools_version(&self) -> Option<&str> {
        self.get_s(SummaryVariable::PkgtoolsVersion)
    }

    /**
     * Returns the [`PrevPkgpath`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let prev_pkgpath = String::from("obsolete/testpkg");
     *
     * sum.set_prev_pkgpath(prev_pkgpath.as_str());
     *
     * assert_eq!(Some(prev_pkgpath.as_str()), sum.prev_pkgpath());
     * ```
     *
     * [`PrevPkgpath`]: ../summary/enum.SummaryVariable.html#variant.PrevPkgpath
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn prev_pkgpath(&self) -> Option<&str> {
        self.get_s(SummaryVariable::PrevPkgpath)
    }

    /**
     * Returns the [`Provides`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let provides = vec![
     *                    String::from("/opt/pkg/lib/libfoo.dylib"),
     *                    String::from("/opt/pkg/lib/libbar.dylib"),
     *                 ];
     *
     * sum.set_provides(provides.as_slice());
     *
     * assert_eq!(Some(provides.as_slice()), sum.provides());
     * ```
     *
     * [`Provides`]: ../summary/enum.SummaryVariable.html#variant.Provides
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn provides(&self) -> Option<&[String]> {
        self.get_a(SummaryVariable::Provides)
    }

    /**
     * Returns the [`Requires`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let requires = vec![
     *                    String::from("/usr/lib/libSystem.B.dylib"),
     *                    String::from("/usr/lib/libiconv.2.dylib"),
     *                 ];
     *
     * sum.set_requires(requires.as_slice());
     *
     * assert_eq!(Some(requires.as_slice()), sum.requires());
     * ```
     *
     * [`Requires`]: ../summary/enum.SummaryVariable.html#variant.Requires
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn requires(&self) -> Option<&[String]> {
        self.get_a(SummaryVariable::Requires)
    }

    /**
     * Returns the [`SizePkg`] value, if set.  This is a required field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let size_pkg = 4321;
     *
     * sum.set_size_pkg(size_pkg);
     *
     * assert_eq!(Some(size_pkg), sum.size_pkg());
     * ```
     *
     * [`SizePkg`]: ../summary/enum.SummaryVariable.html#variant.SizePkg
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn size_pkg(&self) -> Option<u64> {
        self.get_i(SummaryVariable::SizePkg)
    }

    /**
     * Returns the [`Supersedes`] value, if set.  This is an optional field.
     *
     * Returns [`None`] if unset.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let supersedes = vec![
     *                      String::from("oldpkg-[0-9]*"),
     *                      String::from("badpkg>=2.0"),
     *                  ];
     *
     * sum.set_supersedes(supersedes.as_slice());
     *
     * assert_eq!(Some(supersedes.as_slice()), sum.supersedes());
     * ```
     *
     * [`Supersedes`]: ../summary/enum.SummaryVariable.html#variant.Supersedes
     * [`None`]: https://doc.rust-lang.org/std/option/enum.Option.html#variant.None
     */
    pub fn supersedes(&self) -> Option<&[String]> {
        self.get_a(SummaryVariable::Supersedes)
    }

    // Setters

    /**
     * Set or update the [`BuildDate`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let build_date = String::from("2019-08-12 15:58:02 +0100");
     *
     * sum.set_build_date(build_date.as_str());
     *
     * assert_eq!(Some(build_date.as_str()), sum.build_date());
     * ```
     *
     * [`BuildDate`]: ../summary/enum.SummaryVariable.html#variant.BuildDate
     */
    pub fn set_build_date(&mut self, build_date: &str) {
        self.insert_or_update(
            SummaryVariable::BuildDate,
            SummaryValue::S(build_date.to_string()),
        );
    }

    /**
     * Set or update the [`Categories`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let categories = String::from("devel pkgtools");
     *
     * sum.set_categories(categories.as_str());
     *
     * assert_eq!(Some(categories.as_str()), sum.categories());
     * ```
     *
     * [`Categories`]: ../summary/enum.SummaryVariable.html#variant.Categories
     */
    pub fn set_categories(&mut self, categories: &str) {
        self.insert_or_update(
            SummaryVariable::Categories,
            SummaryValue::S(categories.to_string()),
        );
    }

    /**
     * Set or update the [`Comment`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let comment = String::from("This is a test");
     *
     * sum.set_comment(comment.as_str());
     *
     * assert_eq!(Some(comment.as_str()), sum.comment());
     * ```
     *
     * [`Comment`]: ../summary/enum.SummaryVariable.html#variant.Comment
     */
    pub fn set_comment(&mut self, comment: &str) {
        self.insert_or_update(
            SummaryVariable::Comment,
            SummaryValue::S(comment.to_string()),
        );
    }

    /**
     * Set or update the [`Conflicts`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let conflicts = vec![
     *                     String::from("cfl-pkg1-[0-9]*"),
     *                     String::from("cfl-pkg2>=2.0"),
     *                 ];
     *
     * sum.set_conflicts(conflicts.as_slice());
     *
     * assert_eq!(Some(conflicts.as_slice()), sum.conflicts());
     * ```
     *
     * [`Conflicts`]: ../summary/enum.SummaryVariable.html#variant.Conflicts
     */
    pub fn set_conflicts(&mut self, conflicts: &[String]) {
        self.insert_or_update(
            SummaryVariable::Conflicts,
            SummaryValue::A(conflicts.to_vec()),
        );
    }

    /**
     * Set or update the [`Depends`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let depends = vec![
     *                     String::from("dep-pkg1-[0-9]*"),
     *                     String::from("dep-pkg2>=2.0"),
     *                 ];
     *
     * sum.set_depends(depends.as_slice());
     *
     * assert_eq!(Some(depends.as_slice()), sum.depends());
     * ```
     *
     * [`Depends`]: ../summary/enum.SummaryVariable.html#variant.Depends
     */
    pub fn set_depends(&mut self, depends: &[String]) {
        self.insert_or_update(
            SummaryVariable::Depends,
            SummaryValue::A(depends.to_vec()),
        );
    }

    /**
     * Set or update the [`Description`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let description = vec![
     *                       String::from("This is a test"),
     *                       String::from(""),
     *                       String::from("This is a multi-line variable"),
     *                   ];
     *
     * sum.set_description(description.as_slice());
     *
     * assert_eq!(Some(description.as_slice()), sum.description());
     * ```
     *
     * [`Description`]: ../summary/enum.SummaryVariable.html#variant.Description
     */
    pub fn set_description(&mut self, description: &[String]) {
        self.insert_or_update(
            SummaryVariable::Description,
            SummaryValue::A(description.to_vec()),
        );
    }

    /**
     * Set or update the [`FileCksum`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let cksum = String::from("SHA1 a4801e9b26eeb5b8bd1f54bac1c8e89dec67786a");
     *
     * sum.set_file_cksum(cksum.as_str());
     *
     * assert_eq!(Some(cksum.as_str()), sum.file_cksum());
     * ```
     *
     * [`FileCksum`]: ../summary/enum.SummaryVariable.html#variant.FileCksum
     */
    pub fn set_file_cksum(&mut self, file_cksum: &str) {
        self.insert_or_update(
            SummaryVariable::FileCksum,
            SummaryValue::S(file_cksum.to_string()),
        );
    }

    /**
     * Set or update the [`FileName`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let filename = String::from("testpkg-1.0.tgz");
     *
     * sum.set_file_name(filename.as_str());
     *
     * assert_eq!(Some(filename.as_str()), sum.file_name());
     * ```
     *
     * [`FileName`]: ../summary/enum.SummaryVariable.html#variant.FileName
     */
    pub fn set_file_name(&mut self, file_name: &str) {
        self.insert_or_update(
            SummaryVariable::FileName,
            SummaryValue::S(file_name.to_string()),
        );
    }

    /**
     * Set or update the [`FileSize`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let filesize = 1234;
     *
     * sum.set_file_size(filesize);
     *
     * assert_eq!(Some(filesize), sum.file_size());
     * ```
     *
     * [`FileSize`]: ../summary/enum.SummaryVariable.html#variant.FileSize
     */
    pub fn set_file_size(&mut self, file_size: u64) {
        self.insert_or_update(
            SummaryVariable::FileSize,
            SummaryValue::I(file_size),
        );
    }

    /**
     * Set or update the [`Homepage`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let homepage = String::from("https://docs.rs/pkgsrc/");
     *
     * sum.set_homepage(homepage.as_str());
     *
     * assert_eq!(Some(homepage.as_str()), sum.homepage());
     * ```
     *
     * [`Homepage`]: ../summary/enum.SummaryVariable.html#variant.Homepage
     */
    pub fn set_homepage(&mut self, homepage: &str) {
        self.insert_or_update(
            SummaryVariable::Homepage,
            SummaryValue::S(homepage.to_string()),
        );
    }

    /**
     * Set or update the [`License`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let license = String::from("apache-2.0 OR modified-bsd");
     *
     * sum.set_license(license.as_str());
     *
     * assert_eq!(Some(license.as_str()), sum.license());
     * ```
     *
     * [`License`]: ../summary/enum.SummaryVariable.html#variant.License
     */
    pub fn set_license(&mut self, license: &str) {
        self.insert_or_update(
            SummaryVariable::License,
            SummaryValue::S(license.to_string()),
        );
    }

    /**
     * Set or update the [`MachineArch`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let machine_arch = String::from("x86_64");
     *
     * sum.set_machine_arch(machine_arch.as_str());
     *
     * assert_eq!(Some(machine_arch.as_str()), sum.machine_arch());
     * ```
     *
     * [`MachineArch`]: ../summary/enum.SummaryVariable.html#variant.MachineArch
     */
    pub fn set_machine_arch(&mut self, machine_arch: &str) {
        self.insert_or_update(
            SummaryVariable::MachineArch,
            SummaryValue::S(machine_arch.to_string()),
        );
    }

    /**
     * Set or update the [`Opsys`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let opsys = String::from("Darwin");
     *
     * sum.set_opsys(opsys.as_str());
     *
     * assert_eq!(Some(opsys.as_str()), sum.opsys());
     * ```
     *
     * [`Opsys`]: ../summary/enum.SummaryVariable.html#variant.Opsys
     */
    pub fn set_opsys(&mut self, opsys: &str) {
        self.insert_or_update(
            SummaryVariable::Opsys,
            SummaryValue::S(opsys.to_string()),
        );
    }

    /**
     * Set or update the [`OsVersion`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let os_version = String::from("18.7.0");
     *
     * sum.set_os_version(os_version.as_str());
     *
     * assert_eq!(Some(os_version.as_str()), sum.os_version());
     * ```
     *
     * [`OsVersion`]: ../summary/enum.SummaryVariable.html#variant.OsVersion
     */
    pub fn set_os_version(&mut self, os_version: &str) {
        self.insert_or_update(
            SummaryVariable::OsVersion,
            SummaryValue::S(os_version.to_string()),
        );
    }

    /**
     * Set or update the [`PkgOptions`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let pkg_options = String::from("http2 idn inet6 ldap libssh2");
     *
     * sum.set_pkg_options(pkg_options.as_str());
     *
     * assert_eq!(Some(pkg_options.as_str()), sum.pkg_options());
     * ```
     *
     * [`PkgOptions`]: ../summary/enum.SummaryVariable.html#variant.PkgOptions
     */
    pub fn set_pkg_options(&mut self, pkg_options: &str) {
        self.insert_or_update(
            SummaryVariable::PkgOptions,
            SummaryValue::S(pkg_options.to_string()),
        );
    }

    /**
     * Set or update the [`Pkgname`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let pkgname = String::from("testpkg-1.0");
     *
     * sum.set_pkgname(pkgname.as_str());
     *
     * assert_eq!(Some(pkgname.as_str()), sum.pkgname());
     * ```
     *
     * [`Pkgname`]: ../summary/enum.SummaryVariable.html#variant.Pkgname
     */
    pub fn set_pkgname(&mut self, pkgname: &str) {
        self.insert_or_update(
            SummaryVariable::Pkgname,
            SummaryValue::S(pkgname.to_string()),
        );
    }

    /**
     * Set or update the [`Pkgpath`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let pkgpath = String::from("pkgtools/testpkg");
     *
     * sum.set_pkgpath(pkgpath.as_str());
     *
     * assert_eq!(Some(pkgpath.as_str()), sum.pkgpath());
     * ```
     *
     * [`Pkgpath`]: ../summary/enum.SummaryVariable.html#variant.Pkgpath
     */
    pub fn set_pkgpath(&mut self, pkgpath: &str) {
        self.insert_or_update(
            SummaryVariable::Pkgpath,
            SummaryValue::S(pkgpath.to_string()),
        );
    }

    /**
     * Set or update the [`PkgtoolsVersion`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let pkgtools_version = String::from("20091115");
     *
     * sum.set_pkgtools_version(pkgtools_version.as_str());
     *
     * assert_eq!(Some(pkgtools_version.as_str()), sum.pkgtools_version());
     * ```
     *
     * [`PkgtoolsVersion`]: ../summary/enum.SummaryVariable.html#variant.PkgtoolsVersion
     */
    pub fn set_pkgtools_version(&mut self, pkgtools_version: &str) {
        self.insert_or_update(
            SummaryVariable::PkgtoolsVersion,
            SummaryValue::S(pkgtools_version.to_string()),
        );
    }

    /**
     * Set or update the [`PrevPkgpath`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let prev_pkgpath = String::from("obsolete/testpkg");
     *
     * sum.set_prev_pkgpath(prev_pkgpath.as_str());
     *
     * assert_eq!(Some(prev_pkgpath.as_str()), sum.prev_pkgpath());
     * ```
     *
     * [`PrevPkgpath`]: ../summary/enum.SummaryVariable.html#variant.PrevPkgpath
     */
    pub fn set_prev_pkgpath(&mut self, prev_pkgpath: &str) {
        self.insert_or_update(
            SummaryVariable::PrevPkgpath,
            SummaryValue::S(prev_pkgpath.to_string()),
        );
    }

    /**
     * Set or update the [`Provides`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let provides = vec![
     *                    String::from("/opt/pkg/lib/libfoo.dylib"),
     *                    String::from("/opt/pkg/lib/libbar.dylib"),
     *                 ];
     *
     * sum.set_provides(provides.as_slice());
     *
     * assert_eq!(Some(provides.as_slice()), sum.provides());
     * ```
     *
     * [`Provides`]: ../summary/enum.SummaryVariable.html#variant.Provides
     */
    pub fn set_provides(&mut self, provides: &[String]) {
        self.insert_or_update(
            SummaryVariable::Provides,
            SummaryValue::A(provides.to_vec()),
        );
    }

    /**
     * Set or update the [`Requires`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let requires = vec![
     *                    String::from("/usr/lib/libSystem.B.dylib"),
     *                    String::from("/usr/lib/libiconv.2.dylib"),
     *                 ];
     *
     * sum.set_requires(requires.as_slice());
     *
     * assert_eq!(Some(requires.as_slice()), sum.requires());
     * ```
     *
     * [`Requires`]: ../summary/enum.SummaryVariable.html#variant.Requires
     */
    pub fn set_requires(&mut self, requires: &[String]) {
        self.insert_or_update(
            SummaryVariable::Requires,
            SummaryValue::A(requires.to_vec()),
        );
    }

    /**
     * Set or update the [`SizePkg`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let size_pkg = 4321;
     *
     * sum.set_size_pkg(size_pkg);
     *
     * assert_eq!(Some(size_pkg), sum.size_pkg());
     * ```
     *
     * [`SizePkg`]: ../summary/enum.SummaryVariable.html#variant.SizePkg
     */
    pub fn set_size_pkg(&mut self, size_pkg: u64) {
        self.insert_or_update(
            SummaryVariable::SizePkg,
            SummaryValue::I(size_pkg),
        );
    }

    /**
     * Set or update the [`Supersedes`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let supersedes = vec![
     *                      String::from("oldpkg-[0-9]*"),
     *                      String::from("badpkg>=2.0"),
     *                  ];
     *
     * sum.set_supersedes(supersedes.as_slice());
     *
     * assert_eq!(Some(supersedes.as_slice()), sum.supersedes());
     * ```
     *
     * [`Supersedes`]: ../summary/enum.SummaryVariable.html#variant.Supersedes
     */
    pub fn set_supersedes(&mut self, supersedes: &[String]) {
        self.insert_or_update(
            SummaryVariable::Supersedes,
            SummaryValue::A(supersedes.to_vec()),
        );
    }

    /**
     * Set or append to the [`Conflicts`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let conflicts = vec![
     *                     String::from("cfl-pkg1-[0-9]*"),
     *                     String::from("cfl-pkg2>=2.0"),
     *                 ];
     *
     * for conflict in &conflicts {
     *     sum.push_conflicts(&conflict);
     * }
     *
     * assert_eq!(Some(conflicts.as_slice()), sum.conflicts());
     * ```
     *
     * [`Conflicts`]: ../summary/enum.SummaryVariable.html#variant.Conflicts
     */
    pub fn push_conflicts(&mut self, conflicts: &str) {
        self.insert_or_push(
            SummaryVariable::Conflicts,
            SummaryValue::A(vec![conflicts.to_string()]),
        );
    }

    /**
     * Set or append to the [`Depends`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     * let depends = vec![
     *                     String::from("dep-pkg1-[0-9]*"),
     *                     String::from("dep-pkg2>=2.0"),
     *                 ];
     *
     * for depend in &depends {
     *     sum.push_depends(&depend);
     * }
     *
     * assert_eq!(Some(depends.as_slice()), sum.depends());
     * ```
     *
     * [`Depends`]: ../summary/enum.SummaryVariable.html#variant.Depends
     */
    pub fn push_depends(&mut self, depends: &str) {
        self.insert_or_push(
            SummaryVariable::Depends,
            SummaryValue::A(vec![depends.to_string()]),
        );
    }

    /**
     * Set or append to the [`Description`] value.  This is a required field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     *
     * let description = vec![
     *                       String::from("This is a test"),
     *                       String::from(""),
     *                       String::from("This is a multi-line variable"),
     *                   ];
     *
     * for line in &description {
     *     sum.push_description(&line);
     * }
     *
     * assert_eq!(Some(description.as_slice()), sum.description());
     * ```
     *
     * [`Description`]: ../summary/enum.SummaryVariable.html#variant.Description
     */
    pub fn push_description(&mut self, description: &str) {
        self.insert_or_push(
            SummaryVariable::Description,
            SummaryValue::A(vec![description.to_string()]),
        );
    }

    /**
     * Set or append to the [`Provides`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     *
     * let provides = vec![
     *                    String::from("/opt/pkg/lib/libfoo.dylib"),
     *                    String::from("/opt/pkg/lib/libbar.dylib"),
     *                 ];
     *
     * for prov in &provides {
     *     sum.push_provides(&prov);
     * }
     *
     * assert_eq!(Some(provides.as_slice()), sum.provides());
     * ```
     *
     * [`Provides`]: ../summary/enum.SummaryVariable.html#variant.Provides
     */
    pub fn push_provides(&mut self, provides: &str) {
        self.insert_or_push(
            SummaryVariable::Provides,
            SummaryValue::A(vec![provides.to_string()]),
        );
    }

    /**
     * Set or append to the [`Requires`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     *
     * let requires = vec![
     *                    String::from("/usr/lib/libSystem.B.dylib"),
     *                    String::from("/usr/lib/libiconv.2.dylib"),
     *                 ];
     *
     * for r in &requires {
     *     sum.push_requires(&r);
     * }
     *
     * assert_eq!(Some(requires.as_slice()), sum.requires());
     * ```
     *
     * [`Requires`]: ../summary/enum.SummaryVariable.html#variant.Requires
     */
    pub fn push_requires(&mut self, requires: &str) {
        self.insert_or_push(
            SummaryVariable::Requires,
            SummaryValue::A(vec![requires.to_string()]),
        );
    }

    /**
     * Set or append to the [`Supersedes`] value.  This is an optional field.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::Summary;
     *
     * let mut sum = Summary::new();
     *
     * let supersedes = vec![
     *                      String::from("oldpkg-[0-9]*"),
     *                      String::from("badpkg>=2.0"),
     *                  ];
     *
     * for s in &supersedes {
     *     sum.push_supersedes(&s);
     * }
     *
     * assert_eq!(Some(supersedes.as_slice()), sum.supersedes());
     * ```
     *
     * [`Supersedes`]: ../summary/enum.SummaryVariable.html#variant.Supersedes
     */
    pub fn push_supersedes(&mut self, supersedes: &str) {
        self.insert_or_push(
            SummaryVariable::Supersedes,
            SummaryValue::A(vec![supersedes.to_string()]),
        );
    }
}

impl FromStr for Summary {
    type Err = SummaryError;

    fn from_str(s: &str) -> Result<Self> {
        let mut sum = Summary::new();
        for line in s.lines() {
            let v: Vec<&str> = line.splitn(2, '=').collect();
            if v.len() != 2 {
                return Err(SummaryError::ParseLine(line.to_string()));
            }
            let key = SummaryVariable::from_str(v[0])?;
            match key {
                SummaryVariable::BuildDate => sum.set_build_date(v[1]),
                SummaryVariable::Categories => sum.set_categories(v[1]),
                SummaryVariable::Comment => sum.set_comment(v[1]),
                SummaryVariable::Conflicts => sum.push_conflicts(v[1]),
                SummaryVariable::Depends => sum.push_depends(v[1]),
                SummaryVariable::Description => sum.push_description(v[1]),
                SummaryVariable::FileCksum => sum.set_file_cksum(v[1]),
                SummaryVariable::FileName => sum.set_file_name(v[1]),
                SummaryVariable::FileSize => {
                    sum.set_file_size(v[1].parse::<u64>()?)
                }
                SummaryVariable::Homepage => sum.set_homepage(v[1]),
                SummaryVariable::License => sum.set_license(v[1]),
                SummaryVariable::MachineArch => sum.set_machine_arch(v[1]),
                SummaryVariable::Opsys => sum.set_opsys(v[1]),
                SummaryVariable::OsVersion => sum.set_os_version(v[1]),
                SummaryVariable::PkgOptions => sum.set_pkg_options(v[1]),
                SummaryVariable::Pkgname => sum.set_pkgname(v[1]),
                SummaryVariable::Pkgpath => sum.set_pkgpath(v[1]),
                SummaryVariable::PkgtoolsVersion => {
                    sum.set_pkgtools_version(v[1])
                }
                SummaryVariable::PrevPkgpath => sum.set_prev_pkgpath(v[1]),
                SummaryVariable::Provides => sum.push_provides(v[1]),
                SummaryVariable::Requires => sum.push_requires(v[1]),
                SummaryVariable::SizePkg => {
                    sum.set_size_pkg(v[1].parse::<u64>()?)
                }
                SummaryVariable::Supersedes => sum.push_supersedes(v[1]),
            }
        }

        /*
         * Validate complete entry.
         */
        if sum.build_date().is_none() {
            return Err(SummaryError::Incomplete(MissingVariable::BuildDate));
        }
        if sum.categories().is_none() {
            return Err(SummaryError::Incomplete(MissingVariable::Categories));
        }
        if sum.comment().is_none() {
            return Err(SummaryError::Incomplete(MissingVariable::Comment));
        }
        if sum.description().is_none() {
            return Err(SummaryError::Incomplete(MissingVariable::Description));
        }
        if sum.machine_arch().is_none() {
            return Err(SummaryError::Incomplete(MissingVariable::MachineArch));
        }
        if sum.opsys().is_none() {
            return Err(SummaryError::Incomplete(MissingVariable::Opsys));
        }
        if sum.os_version().is_none() {
            return Err(SummaryError::Incomplete(MissingVariable::OsVersion));
        }
        if sum.pkgname().is_none() {
            return Err(SummaryError::Incomplete(MissingVariable::Pkgname));
        }
        if sum.pkgpath().is_none() {
            return Err(SummaryError::Incomplete(MissingVariable::Pkgpath));
        }
        if sum.pkgtools_version().is_none() {
            return Err(SummaryError::Incomplete(
                MissingVariable::PkgtoolsVersion,
            ));
        }
        if sum.size_pkg().is_none() {
            return Err(SummaryError::Incomplete(MissingVariable::SizePkg));
        }

        Ok(sum)
    }
}

/**
 * Enum containing possible reasons that parsing [`pkg_summary(5)`] failed
 *
 * [`pkg_summary(5)`]: https://netbsd.gw.com/cgi-bin/man-cgi?pkg_summary+5
 */
#[derive(Debug)]
pub enum SummaryError {
    /**
     * The summary is incomplete due to a missing required variable.
     */
    Incomplete(MissingVariable),
    /**
     * An underlying `io::Error`.
     */
    Io(io::Error),
    /**
     * The supplied line is not in the correct VARIABLE=VALUE format.
     */
    ParseLine(String),
    /**
     * The supplied variable is not a valid pkg_summary(5) variable.
     */
    ParseVariable(String),
    /**
     * Parsing a supplied value as an Integer type (required for `FILE_SIZE`
     * and `SIZE_PKG`) failed.
     */
    ParseInt(ParseIntError),
}

/**
 * Missing variables that are required for a valid [`pkg_summary(5)`] entity.
 *
 * [`pkg_summary(5)`]: https://netbsd.gw.com/cgi-bin/man-cgi?pkg_summary+5
 */
#[derive(Debug)]
pub enum MissingVariable {
    /**
     * Missing required BUILD_DATE variable.
     */
    BuildDate,
    /**
     * Missing required CATEGORIES variable.
     */
    Categories,
    /**
     * Missing required COMMENT variable.
     */
    Comment,
    /**
     * Missing required DESCRIPTION variable.
     */
    Description,
    /**
     * Missing required MACHINE_ARCH variable.
     */
    MachineArch,
    /**
     * Missing required OPSYS variable.
     */
    Opsys,
    /**
     * Missing required OS_VERSION variable.
     */
    OsVersion,
    /**
     * Missing required PKGNAME variable.
     */
    Pkgname,
    /**
     * Missing required PKGPATH variable.
     */
    Pkgpath,
    /**
     * Missing required PKGTOOLS_VERSION variable.
     */
    PkgtoolsVersion,
    /**
     * Missing required SIZE_PKG variable.
     */
    SizePkg,
}

impl fmt::Display for SummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SummaryError::ParseLine(s) => {
                write!(f, "not correctly formatted (VARIABLE=VALUE): {}", s)
            }
            SummaryError::ParseVariable(s) => {
                write!(f, "'{}' is not a supported pkg_summary variable", s)
            }
            SummaryError::ParseInt(s) => {
                /* Defer to ParseIntError formatting */
                write!(f, "{}", s)
            }
            SummaryError::Io(s) => {
                /* Defer to io::Error formatting */
                write!(f, "{}", s)
            }
            SummaryError::Incomplete(s) => {
                /* Defer to MissingVariable formatting */
                write!(f, "{}", s)
            }
        }
    }
}

impl Error for SummaryError {}

impl fmt::Display for MissingVariable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "missing required variable ")?;
        match self {
            MissingVariable::BuildDate => write!(f, "BUILD_DATE"),
            MissingVariable::Categories => write!(f, "CATEGORIES"),
            MissingVariable::Comment => write!(f, "COMMENT"),
            MissingVariable::Description => write!(f, "DESCRIPTION"),
            MissingVariable::MachineArch => write!(f, "MACHINE_ARCH"),
            MissingVariable::Opsys => write!(f, "OPSYS"),
            MissingVariable::OsVersion => write!(f, "OS_VERSION"),
            MissingVariable::Pkgname => write!(f, "PKGNAME"),
            MissingVariable::Pkgpath => write!(f, "PKGPATH"),
            MissingVariable::PkgtoolsVersion => write!(f, "PKGTOOLS_VERSION"),
            MissingVariable::SizePkg => write!(f, "SIZE_PKG"),
        }
    }
}

impl fmt::Display for SummaryVariable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SummaryVariable::BuildDate => write!(f, "BUILD_DATE"),
            SummaryVariable::Categories => write!(f, "CATEGORIES"),
            SummaryVariable::Comment => write!(f, "COMMENT"),
            SummaryVariable::Conflicts => write!(f, "CONFLICTS"),
            SummaryVariable::Depends => write!(f, "DEPENDS"),
            SummaryVariable::Description => write!(f, "DESCRIPTION"),
            SummaryVariable::FileCksum => write!(f, "FILE_CKSUM"),
            SummaryVariable::FileName => write!(f, "FILE_NAME"),
            SummaryVariable::FileSize => write!(f, "FILE_SIZE"),
            SummaryVariable::Homepage => write!(f, "HOMEPAGE"),
            SummaryVariable::License => write!(f, "LICENSE"),
            SummaryVariable::MachineArch => write!(f, "MACHINE_ARCH"),
            SummaryVariable::Opsys => write!(f, "OPSYS"),
            SummaryVariable::OsVersion => write!(f, "OS_VERSION"),
            SummaryVariable::PkgOptions => write!(f, "PKG_OPTIONS"),
            SummaryVariable::Pkgname => write!(f, "PKGNAME"),
            SummaryVariable::Pkgpath => write!(f, "PKGPATH"),
            SummaryVariable::PkgtoolsVersion => write!(f, "PKGTOOLS_VERSION"),
            SummaryVariable::PrevPkgpath => write!(f, "PREV_PKGPATH"),
            SummaryVariable::Provides => write!(f, "PROVIDES"),
            SummaryVariable::Requires => write!(f, "REQUIRES"),
            SummaryVariable::SizePkg => write!(f, "SIZE_PKG"),
            SummaryVariable::Supersedes => write!(f, "SUPERSEDES"),
        }
    }
}

impl From<io::Error> for SummaryError {
    fn from(err: io::Error) -> Self {
        SummaryError::Io(err)
    }
}

impl From<ParseIntError> for SummaryError {
    fn from(err: ParseIntError) -> Self {
        SummaryError::ParseInt(err)
    }
}

/**
 * A collection of [`pkg_summary(5)`] entries.
 *
 * Each pkg_summary entry should be separated by a single blank line.
 *
 * The [`Write`] trait is implemented, and is the method by which an existing
 * pkg_summary file can be parsed into a new [`Summaries`].
 *
 * [`Display`] is also implemented so printing the newly created collection
 * will result in a correctly formed pkg_summary file.
 *
 * ## Example
 *
 * ```
 * use pkgsrc::summary::Summaries;
 * use unindent::unindent;
 *
 * let mut pkgsummary = Summaries::new();
 * let pkginfo = unindent(r#"
 *     BUILD_DATE=2019-08-12 15:58:02 +0100
 *     CATEGORIES=devel pkgtools
 *     COMMENT=This is a test
 *     DESCRIPTION=A test description
 *     DESCRIPTION=
 *     DESCRIPTION=This is a multi-line variable
 *     MACHINE_ARCH=x86_64
 *     OPSYS=Darwin
 *     OS_VERSION=18.7.0
 *     PKGNAME=testpkg-1.0
 *     PKGPATH=pkgtools/testpkg
 *     PKGTOOLS_VERSION=20091115
 *     SIZE_PKG=4321
 *     "#);
 * /*
 *  * Obviously 3 identical entries is useless, but serves as an example.
 *  */
 * let input = format!("{}\n{}\n{}\n", pkginfo, pkginfo, pkginfo);
 * std::io::copy(&mut input.as_bytes(), &mut pkgsummary);
 *
 * /*
 *  * Output should match what we received.
 *  */
 * let output = format!("{}", pkgsummary);
 * assert_eq!(input.as_bytes(), output.as_bytes());
 * assert_eq!(pkgsummary.entries().len(), 3);
 *
 * /*
 *  * Use each Summary entry to emulate pkg_info output.  This will hopefully
 *  * be implemented as a proper Iterator at some point.  Note that these
 *  * values are safe to unwrap as they are required variables and have been
 *  * checked to exist when creating the entries.
 *  */
 * for sum in pkgsummary.entries() {
 *     println!("{:20} {}", sum.pkgname().unwrap(), sum.comment().unwrap());
 * }
 * ```
 *
 * [`pkg_summary(5)`]: https://netbsd.gw.com/cgi-bin/man-cgi?pkg_summary+5
 * [`Summaries`]: ../summary/struct.Summaries.html
 * [`Display`]: https://doc.rust-lang.org/std/fmt/trait.Display.html
 * [`Write`]: https://doc.rust-lang.org/std/io/trait.Write.html
 */
#[derive(Debug, Default)]
pub struct Summaries {
    buf: Vec<u8>,
    entries: Vec<Summary>,
}

impl Summaries {
    /**
     * Return a new Summaries with default values.
     */
    pub fn new() -> Summaries {
        Summaries {
            buf: vec![],
            entries: vec![],
        }
    }

    /**
     * Return vector of parsed Summary records.
     */
    pub fn entries(&self) -> &Vec<Summary> {
        &self.entries
    }

    /**
     * Return mutable vector of parsed Summary records.
     */
    pub fn entries_mut(&mut self) -> &mut Vec<Summary> {
        &mut self.entries
    }
}

impl fmt::Display for Summaries {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for summary in self.entries() {
            writeln!(f, "{}", summary)?;
        }
        Ok(())
    }
}

impl Write for Summaries {
    /*
     * Stream from our input buffer into Summary records.
     *
     * There is probably a better way to handle this buffer, there's quite a
     * bit of copying/draining going on.  Some kind of circular buffer might be
     * a better option.
     */
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        /*
         * Save the incoming buffer on to the end of any buffer we may already
         * be processing.
         */
        self.buf.extend_from_slice(input);

        /*
         * Look for the last complete pkg_summary(5) record, if there are none
         * then go to the next input.
         */
        let input_string = match std::str::from_utf8(&self.buf) {
            Ok(s) => {
                if let Some(last) = s.rfind("\n\n") {
                    s.get(0..last + 2).unwrap()
                } else {
                    return Ok(input.len());
                }
            }
            Err(e) => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, e))
            }
        };

        /*
         * We have at least one complete record, parse it and add to the vector
         * of summary entries.
         */
        for sum_entry in input_string.split_terminator("\n\n") {
            let sum = match Summary::from_str(&sum_entry) {
                Ok(s) => s,
                Err(e) => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, e))
                }
            };
            self.entries.push(sum);
        }

        /*
         * What we really want is some way to just move forward the beginning
         * of the vector, but there appears to be no way to do that, so we end
         * up having to do something with the existing data.  This seems to be
         * the best way to do it for now?
         */
        let slen = input_string.len();
        self.buf = self.buf.split_off(slen);

        Ok(input.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /*
     * Check we return the correct error types.  There are probably simpler
     * ways to do this.
     */
    #[test]
    fn test_err() {
        match Summary::from_str("BUILD_DATE") {
            Ok(_) => panic!("should return ParseLine failure"),
            Err(e) => match e {
                SummaryError::ParseLine(_) => {}
                _ => panic!("should return ParseLine failure"),
            },
        }
        match Summary::from_str("BILD_DATE=") {
            Ok(_) => panic!("should return ParseVariable failure"),
            Err(e) => match e {
                SummaryError::ParseVariable(_) => {}
                _ => panic!("should return ParseVariable failure"),
            },
        }
        match Summary::from_str("FILE_SIZE=NaN") {
            Ok(_) => panic!("should return ParseInt failure"),
            Err(e) => match e {
                SummaryError::ParseInt(_) => {}
                _ => panic!("should return ParseInt failure"),
            },
        }

        match Summary::from_str("FILE_SIZE=1234") {
            Ok(_) => panic!("should return Incomplete failure"),
            Err(e) => match e {
                SummaryError::Incomplete(_) => {}
                _ => panic!("should return Incomplete failure"),
            },
        }
    }

    /*
     * Test a complete pkg_summary entry, this will go through all of the
     * various functions and ensure everything is working correctly.
     */
    #[test]
    fn test_fromstr() -> Result<()> {
        let pkginfo = unindent(
            r#"
            BUILD_DATE=2019-08-12 15:58:02 +0100
            CATEGORIES=devel pkgtools
            COMMENT=This is a test
            CONFLICTS=cfl-pkg1-[0-9]*
            CONFLICTS=cfl-pkg2>=2.0
            DEPENDS=dep-pkg1-[0-9]*
            DEPENDS=dep-pkg2>=2.0
            DESCRIPTION=A test description
            DESCRIPTION=
            DESCRIPTION=This is a multi-line variable
            FILE_CKSUM=SHA1 a4801e9b26eeb5b8bd1f54bac1c8e89dec67786a
            FILE_NAME=testpkg-1.0.tgz
            FILE_SIZE=1234
            HOMEPAGE=https://docs.rs/pkgsrc/
            LICENSE=apache-2.0 OR modified-bsd
            MACHINE_ARCH=x86_64
            OPSYS=Darwin
            OS_VERSION=18.7.0
            PKG_OPTIONS=http2 idn inet6 ldap libssh2
            PKGNAME=testpkg-1.0
            PKGPATH=pkgtools/testpkg
            PKGTOOLS_VERSION=20091115
            PREV_PKGPATH=obsolete/testpkg
            PROVIDES=/opt/pkg/lib/libfoo.dylib
            PROVIDES=/opt/pkg/lib/libbar.dylib
            REQUIRES=/usr/lib/libSystem.B.dylib
            REQUIRES=/usr/lib/libiconv.2.dylib
            SIZE_PKG=4321
            SUPERSEDES=oldpkg-[0-9]*
            SUPERSEDES=badpkg>=2.0
        "#,
        );
        let sum = Summary::from_str(&pkginfo)?;
        assert_eq!(sum.build_date(), Some("2019-08-12 15:58:02 +0100"));
        assert_eq!(sum.categories(), Some("devel pkgtools"));
        assert_eq!(sum.comment(), Some("This is a test"));
        assert_eq!(sum.conflicts().unwrap()[1], "cfl-pkg2>=2.0");
        assert_eq!(sum.depends().unwrap()[1], "dep-pkg2>=2.0");
        assert_eq!(sum.description().unwrap()[0], "A test description");
        assert_eq!(sum.description().unwrap()[1], "");
        assert_eq!(
            sum.file_cksum(),
            Some("SHA1 a4801e9b26eeb5b8bd1f54bac1c8e89dec67786a")
        );
        assert_eq!(sum.file_name(), Some("testpkg-1.0.tgz"));
        assert_eq!(sum.file_size(), Some(1234));
        assert_eq!(sum.homepage(), Some("https://docs.rs/pkgsrc/"));
        assert_eq!(sum.license(), Some("apache-2.0 OR modified-bsd"));
        assert_eq!(sum.machine_arch(), Some("x86_64"));
        assert_eq!(sum.opsys(), Some("Darwin"));
        assert_eq!(sum.os_version(), Some("18.7.0"));
        assert_eq!(sum.pkg_options(), Some("http2 idn inet6 ldap libssh2"));
        assert_eq!(sum.pkgname(), Some("testpkg-1.0"));
        assert_eq!(sum.pkgpath(), Some("pkgtools/testpkg"));
        assert_eq!(sum.pkgtools_version(), Some("20091115"));
        assert_eq!(sum.prev_pkgpath(), Some("obsolete/testpkg"));
        assert_eq!(sum.provides().unwrap()[1], "/opt/pkg/lib/libbar.dylib");
        assert_eq!(sum.requires().unwrap()[1], "/usr/lib/libiconv.2.dylib");
        assert_eq!(sum.size_pkg(), Some(4321));
        assert_eq!(sum.supersedes().unwrap()[1], "badpkg>=2.0");

        /*
         * Output of our generated Summary should be identical with what we
         * received, at least until we perform any ordering of output.
         */
        let sumout = format!("{}", sum);
        assert_eq!(&pkginfo.as_bytes(), &sumout.as_bytes());

        /*
         * Ensure Summaries works by faking up multiple entries that happen to
         * be identical - it doesn't matter for the purpose of this test.
         */
        let mut sums = Summaries::new();
        let input = format!("{}\n{}\n{}\n", pkginfo, pkginfo, pkginfo);
        std::io::copy(&mut input.as_bytes(), &mut sums)?;
        assert_eq!(sums.entries().len(), 3);

        Ok(())
    }
}
