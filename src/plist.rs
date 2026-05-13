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
 * Packing list parsing and generation.
 *
 * Packing lists, commonly referred to as plists and named `PLIST` in pkgsrc
 * package directories, contain a list of files installed by a package.  They
 * also support a limited number of commands that configure additional package
 * metadata, as well as setting file permissions and performing install and
 * deinstall commands for extracted files.
 *
 * A [`PlistEntry`] is an enum representing a single line in a plist, and a
 * [`Plist`] is a collection of [`PlistEntry`] making up a complete plist.
 * Once a [`Plist`] has been parsed, various functions allow examination of
 * the parsed data.
 *
 * As plists can contain data that is not UTF-8 clean (for example ISO-8859
 * filenames), the primary interfaces for parsing input are byte oriented.
 *
 * Two parser styles are available:
 *
 * * [`Plist::from_bytes`] parses the entire byte slice eagerly into an
 *   owning [`Plist`], a `Vec<PlistEntry<'static>>` with named query methods.
 *   Use this when you want to interrogate the plist multiple times.
 *
 * * [`parse`] returns a lazy iterator of [`PlistEntry<'_>`] borrowing
 *   directly from the source bytes.  Use this when you only need to walk
 *   the plist once: it avoids per-entry allocation.
 *
 * ## Examples
 *
 * Initialize a basic PLIST.  Blank lines are ignored, and only used here for
 * clarity.
 *
 * ```
 * use pkgsrc::plist::{Plist, Result};
 * use indoc::indoc;
 *
 * fn main() -> Result<()> {
 *     let input = indoc! {"
 *         @comment $NetBSD$
 *
 *         @name pkgtest-1.0
 *         @pkgdep dep-pkg1-[0-9]*
 *         @pkgdep dep-pkg2>=2.0
 *         @blddep dep-pkg1-1.0nb2
 *         @blddep dep-pkg2-2.0nb4
 *         @pkgcfl cfl-pkg1<2.0
 *
 *         @display MESSAGE
 *
 *         @cwd /opt/pkg
 *
 *         @comment bin/foo installed with specific permissions, preserved
 *         @comment on uninstall (obsolete feature?), and commands are executed
 *         @comment after it is installed and deleted.
 *
 *         @option preserve
 *         @mode 0644
 *         @owner root
 *         @group wheel
 *         bin/foo
 *         @exec echo \"I just installed F=%F D=%D B=%B f=%f\"
 *         @unexec echo \"I just deleted F=%F D=%D B=%B f=%f\"
 *
 *         @comment bin/bar just installed with default permissions
 *
 *         @mode
 *         @owner
 *         @group
 *         bin/bar
 *
 *         @pkgdir /opt/pkg/share/junk
 *         @dirrm /opt/pkg/share/obsolete-option
 *
 *         @ignore
 *         +BUILD_INFO
 *     "};
 *
 *      let pkglist = Plist::from_bytes(input.as_bytes())?;
 *
 *      assert_eq!(pkglist.pkgname(), Some("pkgtest-1.0"));
 *      assert_eq!(pkglist.depends().count(), 2);
 *      assert_eq!(pkglist.build_depends().count(), 2);
 *      assert_eq!(pkglist.conflicts().count(), 1);
 *      assert_eq!(pkglist.pkgdirs().count(), 1);
 *      assert_eq!(pkglist.pkgrmdirs().count(), 1);
 *
 *      Ok(())
 * }
 * ```
 *
 * [`Plist`] implements [`IntoIterator`], allowing direct iteration over entries:
 *
 * ```
 * use pkgsrc::plist::{Plist, PlistEntry, Result};
 *
 * fn main() -> Result<()> {
 *     let plist = Plist::from_bytes(b"@name pkg-1.0\nbin/foo\nbin/bar")?;
 *
 *     for entry in &plist {
 *         if let PlistEntry::File(path) = entry {
 *             println!("File: {}", path.display());
 *         }
 *     }
 *
 *     Ok(())
 * }
 * ```
 */
use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::str::Utf8Error;
use thiserror::Error;

#[cfg(test)]
use indoc::indoc;

/**
 * A type alias for the result from the creation of either a [`PlistEntry`] or
 * a [`Plist`], with [`PlistError`] returned in [`Err`] variants.
 */
pub type Result<T> = std::result::Result<T, PlistError>;

/**
 * Error type containing possible parse failures.
 */
#[derive(Debug, Error)]
pub enum PlistError {
    /**
     * An unsupported `@command` string, or an unsupported argument to a command
     * that requires specific values (for example `@option preserve`).
     */
    #[error("unsupported plist command: {cmd}", cmd = .0.to_string_lossy())]
    UnsupportedCommand(OsString),
    /**
     * Incorrect number of arguments, or incorrect argument passed to a command
     * that requires a specific format.
     */
    #[error("incorrect command arguments: {args}", args = .0.to_string_lossy())]
    IncorrectArguments(OsString),
    /**
     * Wrapped [`Utf8Error`] when failing to parse valid UTF-8.
     */
    #[error("invalid UTF-8 sequence: {0}")]
    Utf8(#[from] Utf8Error),
}

/**
 * A single plist entry.
 *
 * Entries can be constructed either by using [`PlistEntry::from_bytes`] to
 * parse an array of bytes from a plist, or by constructing one of the
 * variants manually.
 *
 * ## Examples
 *
 * ```
 * use pkgsrc::plist::{PlistEntry, Result};
 * use std::borrow::Cow;
 * use std::ffi::OsStr;
 *
 * fn main() -> Result<()> {
 *     let p1 = PlistEntry::from_bytes(b"@comment hi")?;
 *     let p2 = PlistEntry::Comment(Some(Cow::Borrowed(OsStr::new("hi"))));
 *     assert_eq!(p1, p2);
 *     Ok(())
 * }
 * ```
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlistEntry<'a> {
    /**
     * Filename to extract relative to the current working directory.
     */
    File(Cow<'a, Path>),
    /**
     * Set the internal directory pointer.  All subsequent filenames will be
     * assumed relative to this directory.
     */
    Cwd(Cow<'a, Path>),
    /**
     * Execute command as part of the unpacking process.
     */
    Exec(Cow<'a, OsStr>),
    /**
     * Execute command as part of the deinstallation process.
     */
    UnExec(Cow<'a, OsStr>),
    /**
     * Set default permission for all subsequently extracted files.
     */
    Mode(Option<Cow<'a, str>>),
    /**
     * Set internal package options.  Named PkgOpt to avoid conflict with
     * Rust "Option".
     */
    PkgOpt(PlistOption),
    /**
     * Set default ownership for all subsequently extracted files to specified
     * user.
     */
    Owner(Option<Cow<'a, str>>),
    /**
     * Set default group ownership for all subsequently extracted files to
     * specified group.
     */
    Group(Option<Cow<'a, str>>),
    /**
     * Embed a comment in the packing list.  While specified as mandatory in
     * the manual page, in practise it is not (e.g. `print-PLIST`).
     */
    Comment(Option<Cow<'a, OsStr>>),
    /**
     * Used internally to tell extraction to ignore the next file.
     */
    Ignore,
    /**
     * Set the name of the package.
     */
    Name(Cow<'a, str>),
    /**
     * Declare directory name as managed.
     */
    PkgDir(Cow<'a, Path>),
    /**
     * If directory name exists, it will be deleted at deinstall time.
     */
    DirRm(Cow<'a, Path>),
    /**
     * Declare name as the file to be displayed at install time.
     */
    Display(Cow<'a, Path>),
    /**
     * Declare a dependency on the pkgname package.
     */
    PkgDep(Cow<'a, str>),
    /**
     * Declare that this package was built with the exact version of pkgname.
     */
    BldDep(Cow<'a, str>),
    /**
     * Declare a conflict with the pkgcflname package.
     */
    PkgCfl(Cow<'a, str>),
    /**
     * MD5 checksum of the preceding file entry.
     * Parsed from `@comment MD5:<32-char-hex>`.
     */
    FileChecksum(Cow<'a, str>),
    /**
     * Symlink target for the preceding file entry.
     * Parsed from `@comment Symlink:<target>`.
     */
    SymlinkTarget(Cow<'a, Path>),
}

/**
 * List of valid arguments for the `@option` command.  Currently the only
 * supported argument is `preserve`.
 */
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PlistOption {
    /**
     * Indicates that any existing files should be moved out of the way before
     * the package contents are install (and subsequently restored when the
     * contents are uninstalled).
     */
    Preserve,
}

impl<'a> PlistEntry<'a> {
    /**
     * Construct a new [`PlistEntry`] from a stream of bytes representing a
     * line from a package list.  Validates UTF-8 for variants that are
     * semantically `String`.
     */
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
        parse_line(bytes)
    }

    /**
     * Convert into a `'static` (fully owned) entry by cloning any borrowed
     * payloads.
     */
    #[must_use]
    pub fn into_owned(self) -> PlistEntry<'static> {
        use PlistEntry as P;
        match self {
            P::File(p) => P::File(own_path(p)),
            P::Cwd(p) => P::Cwd(own_path(p)),
            P::Exec(o) => P::Exec(own_osstr(o)),
            P::UnExec(o) => P::UnExec(own_osstr(o)),
            P::Mode(s) => P::Mode(own_opt_str(s)),
            P::PkgOpt(o) => P::PkgOpt(o),
            P::Owner(s) => P::Owner(own_opt_str(s)),
            P::Group(s) => P::Group(own_opt_str(s)),
            P::Comment(o) => P::Comment(own_opt_osstr(o)),
            P::Ignore => P::Ignore,
            P::Name(s) => P::Name(own_str(s)),
            P::PkgDir(p) => P::PkgDir(own_path(p)),
            P::DirRm(p) => P::DirRm(own_path(p)),
            P::Display(p) => P::Display(own_path(p)),
            P::PkgDep(s) => P::PkgDep(own_str(s)),
            P::BldDep(s) => P::BldDep(own_str(s)),
            P::PkgCfl(s) => P::PkgCfl(own_str(s)),
            P::FileChecksum(s) => P::FileChecksum(own_str(s)),
            P::SymlinkTarget(p) => P::SymlinkTarget(own_path(p)),
        }
    }
}

/**
 * A lazy iterator over a plist's entries, borrowing from the source bytes.
 *
 * Returned by [`parse`].  Yields `Result<PlistEntry<'_>>`: each item is a
 * parsed entry or a [`PlistError`] for that line.  Blank and
 * whitespace-only lines are skipped.  UTF-8 validation is performed
 * inline for variants whose payloads are typed as [`Cow<str>`] (see
 * [`PlistEntry`]); a bad-UTF-8 payload on those variants yields a
 * [`PlistError::Utf8`].
 */
pub struct Parser<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Parser<'a> {
    type Item = Result<PlistEntry<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let line = next_line(&mut self.rest)?;
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            return Some(parse_line(line));
        }
    }
}

/**
 * Lazily parse `bytes` into a stream of [`PlistEntry`] values.
 *
 * Payloads borrow directly from `bytes` and the call itself does no work.
 * Intended for one-pass walks over large plists where allocating owned
 * payloads per line is wasteful.
 *
 * UTF-8 is validated inline for variants whose payloads are typed as
 * [`Cow<str>`].  The cost is byte-level scanning over those payloads
 * only (typically a small fraction of plist content) and produces an
 * `Err` for malformed input rather than silently lossy data.
 */
#[must_use]
pub fn parse(bytes: &[u8]) -> Parser<'_> {
    Parser { rest: bytes }
}

fn next_line<'a>(rest: &mut &'a [u8]) -> Option<&'a [u8]> {
    if rest.is_empty() {
        return None;
    }
    match rest.iter().position(|&b| b == b'\n') {
        Some(i) => {
            let line = &rest[..i];
            *rest = &rest[i + 1..];
            Some(line)
        }
        None => {
            let line = *rest;
            *rest = &[];
            Some(line)
        }
    }
}

fn parse_line(line: &[u8]) -> Result<PlistEntry<'_>> {
    let (cmd, args) = split_cmd_args(line);

    if !cmd.starts_with(b"@") {
        return Ok(PlistEntry::File(borrow_path(line)));
    }

    match cmd {
        /*
         * @src and @cd are effectively aliases for @cwd.
         */
        b"@cwd" | b"@src" | b"@cd" => {
            required_path(args, line, PlistEntry::Cwd)
        }
        b"@exec" => required_osstr(args, line, PlistEntry::Exec),
        b"@unexec" => required_osstr(args, line, PlistEntry::UnExec),

        /*
         * File ownership and permissions are allowed to be unset,
         * indicating that they return to their respective defaults.
         */
        b"@mode" => Ok(PlistEntry::Mode(optional_str(args)?)),
        b"@owner" => Ok(PlistEntry::Owner(optional_str(args)?)),
        b"@group" => Ok(PlistEntry::Group(optional_str(args)?)),

        /*
         * Currently "preserve" is the only valid option.
         */
        b"@option" => match args {
            Some(b"preserve") => Ok(PlistEntry::PkgOpt(PlistOption::Preserve)),
            Some(_) => Err(PlistError::UnsupportedCommand(os(cmd))),
            None => Err(PlistError::IncorrectArguments(os(line))),
        },

        /*
         * Whilst the manual page specifies that @comment takes an
         * argument, it's too pedantic to insist that it must, so we
         * handle it as an optional argument.  Comments often carry
         * non-UTF-8 filenames so the payload is typed as OsStr.
         *
         * Special cases:
         * - "@comment MD5:<hash>"      -> FileChecksum (32-char hex MD5)
         * - "@comment Symlink:<target>" -> SymlinkTarget
         */
        b"@comment" => parse_comment(args),

        /*
         * For now be strict that @ignore must not take arguments.
         */
        b"@ignore" => match args {
            None => Ok(PlistEntry::Ignore),
            Some(_) => Err(PlistError::IncorrectArguments(os(line))),
        },

        b"@name" => required_str(args, line, PlistEntry::Name),
        b"@pkgdep" => required_str(args, line, PlistEntry::PkgDep),
        b"@blddep" => required_str(args, line, PlistEntry::BldDep),
        b"@pkgcfl" => required_str(args, line, PlistEntry::PkgCfl),

        b"@pkgdir" => required_path(args, line, PlistEntry::PkgDir),
        b"@dirrm" => required_path(args, line, PlistEntry::DirRm),
        b"@display" => required_path(args, line, PlistEntry::Display),

        _ => Err(PlistError::UnsupportedCommand(os(cmd))),
    }
}

fn split_cmd_args(line: &[u8]) -> (&[u8], Option<&[u8]>) {
    let Some(i) = line.iter().position(|&b| b == b' ') else {
        return (line, None);
    };
    let cmd = &line[..i];
    let mut j = i + 1;
    while j < line.len() && line[j].is_ascii_whitespace() {
        j += 1;
    }
    if j >= line.len() {
        (cmd, None)
    } else {
        (cmd, Some(&line[j..]))
    }
}

fn required_path<'a>(
    args: Option<&'a [u8]>,
    line: &[u8],
    ctor: fn(Cow<'a, Path>) -> PlistEntry<'a>,
) -> Result<PlistEntry<'a>> {
    match args {
        Some(a) => Ok(ctor(borrow_path(a))),
        None => Err(PlistError::IncorrectArguments(os(line))),
    }
}

fn required_osstr<'a>(
    args: Option<&'a [u8]>,
    line: &[u8],
    ctor: fn(Cow<'a, OsStr>) -> PlistEntry<'a>,
) -> Result<PlistEntry<'a>> {
    match args {
        Some(a) => Ok(ctor(borrow_osstr(a))),
        None => Err(PlistError::IncorrectArguments(os(line))),
    }
}

fn required_str<'a>(
    args: Option<&'a [u8]>,
    line: &[u8],
    ctor: fn(Cow<'a, str>) -> PlistEntry<'a>,
) -> Result<PlistEntry<'a>> {
    match args {
        Some(a) => Ok(ctor(Cow::Borrowed(std::str::from_utf8(a)?))),
        None => Err(PlistError::IncorrectArguments(os(line))),
    }
}

fn optional_str(args: Option<&[u8]>) -> Result<Option<Cow<'_, str>>> {
    Ok(args
        .map(|a| std::str::from_utf8(a).map(Cow::Borrowed))
        .transpose()?)
}

fn parse_comment(args: Option<&[u8]>) -> Result<PlistEntry<'_>> {
    let Some(a) = args else {
        return Ok(PlistEntry::Comment(None));
    };
    if let Some(rest) = a.strip_prefix(b"MD5:") {
        if rest.len() == 32 && rest.iter().all(u8::is_ascii_hexdigit) {
            return Ok(PlistEntry::FileChecksum(Cow::Borrowed(
                std::str::from_utf8(rest)?,
            )));
        }
        return Ok(PlistEntry::Comment(Some(borrow_osstr(a))));
    }
    if let Some(rest) = a.strip_prefix(b"Symlink:") {
        return Ok(PlistEntry::SymlinkTarget(borrow_path(rest)));
    }
    Ok(PlistEntry::Comment(Some(borrow_osstr(a))))
}

#[inline]
fn borrow_osstr(bytes: &[u8]) -> Cow<'_, OsStr> {
    Cow::Borrowed(OsStr::from_bytes(bytes))
}

#[inline]
fn borrow_path(bytes: &[u8]) -> Cow<'_, Path> {
    Cow::Borrowed(Path::new(OsStr::from_bytes(bytes)))
}

#[inline]
fn os(bytes: &[u8]) -> OsString {
    OsStr::from_bytes(bytes).to_os_string()
}

#[inline]
fn own_path(c: Cow<'_, Path>) -> Cow<'static, Path> {
    Cow::Owned(c.into_owned())
}

#[inline]
fn own_osstr(c: Cow<'_, OsStr>) -> Cow<'static, OsStr> {
    Cow::Owned(c.into_owned())
}

#[inline]
fn own_str(c: Cow<'_, str>) -> Cow<'static, str> {
    Cow::Owned(c.into_owned())
}

#[inline]
fn own_opt_osstr(c: Option<Cow<'_, OsStr>>) -> Option<Cow<'static, OsStr>> {
    c.map(own_osstr)
}

#[inline]
fn own_opt_str(c: Option<Cow<'_, str>>) -> Option<Cow<'static, str>> {
    c.map(own_str)
}

/**
 * Information about a file in the packing list, including optional metadata.
 *
 * This struct combines a file path with its associated metadata from the
 * packing list, including:
 * - MD5 checksum (from `@comment MD5:...`)
 * - Symlink target (from `@comment Symlink:...`)
 * - File mode, owner, and group (from `@mode`, `@owner`, `@group`)
 */
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FileInfo {
    /// The file path relative to the current working directory.
    pub path: PathBuf,
    /// MD5 checksum as 32-character hex string, if present.
    pub checksum: Option<String>,
    /// Symlink target path, if this entry represents a symlink.
    pub symlink_target: Option<PathBuf>,
    /// File mode (e.g., "0644", "755"), if set.
    pub mode: Option<String>,
    /// File owner username, if set.
    pub owner: Option<String>,
    /// File group name, if set.
    pub group: Option<String>,
}

/**
 * A complete list of [`PlistEntry`] entries.
 *
 * Entries are parsed eagerly using [`Plist::from_bytes`].  For one-pass
 * streaming parsing without per-entry allocation use [`parse`], which
 * yields [`PlistEntry<'_>`] borrowed from the source.
 *
 * See the top of the module for a full example.
 *
 * ## Examples
 *
 * ```
 * use pkgsrc::plist::{Plist, Result};
 *
 * fn main() -> Result<()> {
 *     let plist = Plist::from_bytes(b"@name pkg-1.0")?;
 *     assert_eq!(plist.pkgname(), Some("pkg-1.0"));
 *     Ok(())
 * }
 * ```
 */
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Plist {
    entries: Vec<PlistEntry<'static>>,
}

impl Plist {
    /**
     * Return an empty new [`Plist`].
     */
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /**
     * Construct a new [`Plist`] from a stream of bytes representing lines
     * from a package list.
     */
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut entries = Vec::new();
        for r in parse(bytes) {
            entries.push(r?.into_owned());
        }
        Ok(Self { entries })
    }

    /**
     * Return the package name as specified with `@name`.  If multiple entries
     * are found only the first is returned.  This is wrapped in [`Option`] as
     * while indicated as mandatory in the manual page it is often left out,
     * deferring to deriving the package name from the file name instead.
     */
    #[must_use]
    pub fn pkgname(&self) -> Option<&str> {
        self.entries.iter().find_map(|e| match e {
            PlistEntry::Name(s) => Some(s.as_ref()),
            _ => None,
        })
    }

    /**
     * Return the optional package display file (i.e. `MESSAGE`) as specified
     * with `@display`.  If multiple entries are found only the first is
     * returned.
     */
    #[must_use]
    pub fn display(&self) -> Option<&Path> {
        self.entries.iter().find_map(|e| match e {
            PlistEntry::Display(p) => Some(p.as_ref()),
            _ => None,
        })
    }

    /**
     * Return an iterator over `@pkgdep` entries as string slices.
     */
    pub fn depends(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.iter().filter_map(|e| match e {
            PlistEntry::PkgDep(s) => Some(s.as_ref()),
            _ => None,
        })
    }

    /**
     * Return an iterator over `@blddep` entries as string slices.
     */
    pub fn build_depends(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.iter().filter_map(|e| match e {
            PlistEntry::BldDep(s) => Some(s.as_ref()),
            _ => None,
        })
    }

    /**
     * Return an iterator over `@pkgcfl` entries as string slices.
     */
    pub fn conflicts(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.iter().filter_map(|e| match e {
            PlistEntry::PkgCfl(s) => Some(s.as_ref()),
            _ => None,
        })
    }

    /**
     * Return an iterator over `@pkgdir` entries as path slices.
     */
    pub fn pkgdirs(&self) -> impl Iterator<Item = &Path> + '_ {
        self.entries.iter().filter_map(|e| match e {
            PlistEntry::PkgDir(p) => Some(p.as_ref()),
            _ => None,
        })
    }

    /**
     * Return an iterator over `@dirrm` entries as path slices.
     */
    pub fn pkgrmdirs(&self) -> impl Iterator<Item = &Path> + '_ {
        self.entries.iter().filter_map(|e| match e {
            PlistEntry::DirRm(p) => Some(p.as_ref()),
            _ => None,
        })
    }

    /**
     * Return an iterator over file entries as path slices.  Any files
     * that come after an `@ignore` command are not listed.
     */
    pub fn files(&self) -> impl Iterator<Item = &Path> + '_ {
        let mut ignore = false;
        self.entries.iter().filter_map(move |entry| match entry {
            PlistEntry::Ignore => {
                ignore = true;
                None
            }
            PlistEntry::File(file) => {
                if std::mem::take(&mut ignore) {
                    None
                } else {
                    Some(file.as_ref())
                }
            }
            _ => None,
        })
    }

    /**
     * Return an iterator over file entries joined with their preceding
     * `@cwd` prefix as owned [`PathBuf`] values.  Any files that come
     * after an `@ignore` command are not listed.
     */
    pub fn files_prefixed(&self) -> impl Iterator<Item = PathBuf> + '_ {
        let mut ignore = false;
        let mut prefix: Option<&Path> = None;
        self.entries.iter().filter_map(move |entry| match entry {
            PlistEntry::Cwd(dir) => {
                prefix = Some(dir.as_ref());
                None
            }
            PlistEntry::Ignore => {
                ignore = true;
                None
            }
            PlistEntry::File(file) => {
                if std::mem::take(&mut ignore) {
                    return None;
                }
                let file: &Path = file.as_ref();
                Some(match prefix {
                    Some(pfx) => pfx.join(file),
                    None => file.to_path_buf(),
                })
            }
            _ => None,
        })
    }

    /**
     * Return an iterator over file entries with their associated metadata.
     *
     * For each non-`@ignore`d file directive, the iterator yields a
     * [`FileInfo`] carrying:
     * - the file path,
     * - the most recent `@mode` / `@owner` / `@group` settings,
     * - an optional MD5 checksum (from a following `@comment MD5:...`),
     * - an optional symlink target (from a following `@comment Symlink:...`).
     *
     * Call `.collect()` if a `Vec<FileInfo>` is needed.
     */
    pub fn files_with_info(&self) -> impl Iterator<Item = FileInfo> + '_ {
        FilesWithInfo::new(&self.entries)
    }

    /**
     * Return an iterator over the [`PlistEntry`] entries used during an
     * install procedure.  It is up to the caller to keep track of file
     * metadata.
     */
    pub fn install_cmds(
        &self,
    ) -> impl Iterator<Item = &PlistEntry<'static>> + '_ {
        let mut ignore = false;
        self.entries.iter().filter(move |entry| match entry {
            /*
             * Ignore the next file, usually (always?) a +METADATA file.
             */
            PlistEntry::Ignore => {
                ignore = true;
                false
            }
            PlistEntry::File(_) => !std::mem::take(&mut ignore),
            PlistEntry::Cwd(_)
            | PlistEntry::Exec(_)
            | PlistEntry::Mode(_)
            | PlistEntry::Owner(_)
            | PlistEntry::Group(_)
            | PlistEntry::PkgDir(_) => true,
            _ => false,
        })
    }

    /**
     * Return an iterator over the [`PlistEntry`] entries used during an
     * uninstall procedure.  It is up to the caller to keep track of file
     * metadata.
     */
    pub fn uninstall_cmds(
        &self,
    ) -> impl Iterator<Item = &PlistEntry<'static>> + '_ {
        let mut ignore = false;
        self.entries.iter().filter(move |entry| match entry {
            /*
             * Ignore the next file, usually (always?) a +METADATA file.
             */
            PlistEntry::Ignore => {
                ignore = true;
                false
            }
            PlistEntry::File(_) => !std::mem::take(&mut ignore),
            PlistEntry::Cwd(_)
            | PlistEntry::UnExec(_)
            | PlistEntry::Mode(_)
            | PlistEntry::Owner(_)
            | PlistEntry::Group(_)
            | PlistEntry::PkgDir(_)
            | PlistEntry::DirRm(_) => true,
            _ => false,
        })
    }

    /**
     * Return bool indicating whether `@option preserve` has been set or not.
     */
    #[must_use]
    pub fn is_preserve(&self) -> bool {
        self.entries
            .iter()
            .any(|e| matches!(e, PlistEntry::PkgOpt(PlistOption::Preserve)))
    }
}

struct FilesWithInfo<'a> {
    entries: &'a [PlistEntry<'static>],
    i: usize,
    ignore: bool,
    mode: Option<String>,
    owner: Option<String>,
    group: Option<String>,
}

impl<'a> FilesWithInfo<'a> {
    fn new(entries: &'a [PlistEntry<'static>]) -> Self {
        Self {
            entries,
            i: 0,
            ignore: false,
            mode: None,
            owner: None,
            group: None,
        }
    }
}

impl Iterator for FilesWithInfo<'_> {
    type Item = FileInfo;

    fn next(&mut self) -> Option<FileInfo> {
        while self.i < self.entries.len() {
            match &self.entries[self.i] {
                PlistEntry::Mode(m) => {
                    self.mode = m.as_deref().map(str::to_owned);
                }
                PlistEntry::Owner(o) => {
                    self.owner = o.as_deref().map(str::to_owned);
                }
                PlistEntry::Group(g) => {
                    self.group = g.as_deref().map(str::to_owned);
                }
                PlistEntry::Ignore => self.ignore = true,
                PlistEntry::File(path) => {
                    self.i += 1;
                    if std::mem::take(&mut self.ignore) {
                        continue;
                    }
                    let mut info = FileInfo {
                        path: path.as_ref().to_path_buf(),
                        checksum: None,
                        symlink_target: None,
                        mode: self.mode.clone(),
                        owner: self.owner.clone(),
                        group: self.group.clone(),
                    };
                    while self.i < self.entries.len() {
                        match &self.entries[self.i] {
                            PlistEntry::FileChecksum(hash) => {
                                info.checksum = Some(hash.as_ref().to_owned());
                                self.i += 1;
                            }
                            PlistEntry::SymlinkTarget(target) => {
                                info.symlink_target =
                                    Some(target.as_ref().to_path_buf());
                                self.i += 1;
                            }
                            _ => break,
                        }
                    }
                    return Some(info);
                }
                _ => {}
            }
            self.i += 1;
        }
        None
    }
}

impl IntoIterator for Plist {
    type Item = PlistEntry<'static>;
    type IntoIter = std::vec::IntoIter<PlistEntry<'static>>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<'a> IntoIterator for &'a Plist {
    type Item = &'a PlistEntry<'static>;
    type IntoIter = std::slice::Iter<'a, PlistEntry<'static>>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /*
     * Set up some macros to simplify tests.
     */
    macro_rules! plist {
        ($s:expr) => {
            Plist::from_bytes(String::from($s).as_bytes())
        };
    }
    macro_rules! plist_entry {
        ($s:expr) => {
            PlistEntry::from_bytes(String::from($s).as_bytes())
                .map(PlistEntry::into_owned)
        };
    }
    macro_rules! plist_match_ok {
        ($s:expr, $p:path) => {
            let plist = plist_entry!($s)?;
            assert_eq!(plist, $p);
        };
    }
    macro_rules! plist_match_ok_arg {
        ($s:expr, $p:path) => {
            match plist_entry!($s) {
                Ok(e) => match e {
                    $p(_) => {}
                    _ => panic!("should be a valid {} entry", stringify!($p)),
                },
                Err(_) => panic!("should be a valid {} entry", stringify!($p)),
            }
        };
    }
    macro_rules! plist_match_error {
        ($s:expr, $p:path) => {
            match plist!($s) {
                Ok(_) => panic!("should return {} error", stringify!($p)),
                Err(e) => match e {
                    $p(_) => {}
                    _ => panic!("should return {} error", stringify!($p)),
                },
            }
        };
    }

    /*
     * Plist commands that only accept strict UTF-8 input.
     */
    macro_rules! valid_utf8 {
        ($s:expr, $p:path) => {
            /*
             * A UTF-8 sparkle heart as used in the Rust documentation, and a
             * Norwegian "ø" in non-UTF-8 compatible ISO-8859 format.
             */
            let heart = vec![240, 159, 146, 150];
            let oe = vec![0xf8];

            /*
             * Supported UTF-8 string.
             */
            let mut t = String::from($s).into_bytes();
            t.extend_from_slice(&heart);
            assert_eq!(PlistEntry::from_bytes(&t)?, $p(Cow::Borrowed("💖")));

            /*
             * Unsupported ISO-8859 byte sequence.
             */
            let mut t = String::from($s).into_bytes();
            t.extend_from_slice(&oe);
            match PlistEntry::from_bytes(&t) {
                Ok(p) => panic!(
                    "should be an invalid {} entry, not {:?}",
                    stringify!($p),
                    p
                ),
                Err(e) => match e {
                    PlistError::Utf8(_) => {}
                    _ => panic!(
                        "should be an invalid {} entry: {}",
                        stringify!($p),
                        e
                    ),
                },
            }
        };
    }

    /*
     * Plist commands that only accept optional strict UTF-8 input.
     */
    macro_rules! valid_utf8_opt {
        ($s:expr, $p:path) => {
            /*
             * A UTF-8 sparkle heart as used in the Rust documentation, and a
             * Norwegian "ø" in non-UTF-8 compatible ISO-8859 format.
             */
            let heart = vec![240, 159, 146, 150];
            let oe = vec![0xf8];

            /*
             * Supported UTF-8 string.
             */
            let mut t = String::from($s).into_bytes();
            t.extend_from_slice(&heart);
            assert_eq!(
                PlistEntry::from_bytes(&t)?,
                $p(Some(Cow::Borrowed("💖")))
            );

            /*
             * Unsupported ISO-8859 byte sequence.
             */
            let mut t = String::from($s).into_bytes();
            t.extend_from_slice(&oe);
            match PlistEntry::from_bytes(&t) {
                Ok(p) => panic!(
                    "should be an invalid {} entry, not {:?}",
                    stringify!($p),
                    p
                ),
                Err(e) => match e {
                    PlistError::Utf8(_) => {}
                    _ => panic!(
                        "should be an invalid {} entry: {}",
                        stringify!($p),
                        e
                    ),
                },
            }
        };
    }

    /*
     * Plist commands that accept ISO-8859 input as a Path payload.
     */
    macro_rules! valid_path {
        ($s:expr, $p:path) => {
            /*
             * A UTF-8 sparkle heart as used in the Rust documentation, and a
             * Norwegian "ø" in non-UTF-8 compatible ISO-8859 format.
             */
            let heart = vec![240, 159, 146, 150];
            let oe = vec![0xf8];

            let mut t = String::from($s).into_bytes();
            t.extend_from_slice(&heart);
            assert_eq!(
                PlistEntry::from_bytes(&t)?,
                $p(Cow::Borrowed(Path::new("💖")))
            );
            let mut t = String::from($s).into_bytes();
            t.extend_from_slice(&oe);
            match PlistEntry::from_bytes(&t) {
                Ok(e) => match e {
                    $p(_) => {}
                    _ => panic!("should be a valid {} entry", stringify!($p)),
                },
                Err(_) => panic!("should be a valid {} entry", stringify!($p)),
            }
        };
    }

    /*
     * Plist commands that accept ISO-8859 input as an OsStr payload.
     */
    macro_rules! valid_osstr {
        ($s:expr, $p:path) => {
            /*
             * A UTF-8 sparkle heart as used in the Rust documentation, and a
             * Norwegian "ø" in non-UTF-8 compatible ISO-8859 format.
             */
            let heart = vec![240, 159, 146, 150];
            let oe = vec![0xf8];

            let mut t = String::from($s).into_bytes();
            t.extend_from_slice(&heart);
            assert_eq!(
                PlistEntry::from_bytes(&t)?,
                $p(Cow::Borrowed(OsStr::new("💖")))
            );
            let mut t = String::from($s).into_bytes();
            t.extend_from_slice(&oe);
            match PlistEntry::from_bytes(&t) {
                Ok(e) => match e {
                    $p(_) => {}
                    _ => panic!("should be a valid {} entry", stringify!($p)),
                },
                Err(_) => panic!("should be a valid {} entry", stringify!($p)),
            }
        };
    }

    /*
     * Plist commands that accept optional ISO-8859 input as an OsStr.
     */
    macro_rules! valid_osstr_opt {
        ($s:expr, $p:path) => {
            /*
             * A UTF-8 sparkle heart as used in the Rust documentation, and a
             * Norwegian "ø" in non-UTF-8 compatible ISO-8859 format.
             */
            let heart = vec![240, 159, 146, 150];
            let oe = vec![0xf8];

            let mut t = String::from($s).into_bytes();
            t.extend_from_slice(&heart);
            assert_eq!(
                PlistEntry::from_bytes(&t)?,
                $p(Some(Cow::Borrowed(OsStr::new("💖"))))
            );
            let mut t = String::from($s).into_bytes();
            t.extend_from_slice(&oe);
            match PlistEntry::from_bytes(&t) {
                Ok(e) => match e {
                    $p(_) => {}
                    _ => panic!("should be a valid {} entry", stringify!($p)),
                },
                Err(_) => panic!("should be a valid {} entry", stringify!($p)),
            }
        };
    }

    /*
     * Test an example full plist for functionality.  Correctness tests are
     * elsewhere.
     */
    #[test]
    fn test_full_plist() -> Result<()> {
        let input = indoc! {"
            @comment $NetBSD$
            @name pkgtest-1.0
            @pkgdep dep-pkg1-[0-9]*
            @pkgdep dep-pkg2>=2.0
            @blddep dep-pkg1-1.0nb2
            @blddep dep-pkg2-2.1
            @pkgcfl cfl-pkg1-[0-9]*
            @pkgcfl cfl-pkg2>=2.0
            @display MESSAGE
            @option preserve
            @cwd /
            @src /
            @cd /
            @mode 0644
            @owner root
            @group wheel
            bin/foo
            @exec touch F=%F D=%D B=%B f=%f
            @unexec rm F=%F D=%D B=%B f=%f
            @mode
            @owner
            @group
            bin/bar
            @pkgdir /var/db/pkgsrc-rs
            @dirrm /var/db/pkgsrc-rs-legacy
            @ignore
            +BUILD_INFO
        "};
        let plist = Plist::from_bytes(input.as_bytes())?;
        assert_eq!(plist.depends().count(), 2);
        assert_eq!(plist.build_depends().count(), 2);
        assert_eq!(plist.conflicts().count(), 2);
        Ok(())
    }

    /*
     * Check parsing for lines and whitespace is as expected.  Notes:
     *
     *  - Trailing whitespace is always removed.
     *  - Leading whitespace of the command is never removed.
     *  - Leading whitespace of optional arguments is removed.
     *  - Entries containing only whitespace are skipped.
     *
     */
    #[test]
    fn test_line_input() -> Result<()> {
        /*
         * Stripping all trailing whitespace for commands that support
         * optional arguments when none is specified should return None.
         */
        assert_eq!(plist_entry!("@comment  \n")?, PlistEntry::Comment(None));
        assert_eq!(plist_entry!("@mode  ")?, PlistEntry::Mode(None));
        assert_eq!(plist_entry!("@owner \t ")?, PlistEntry::Owner(None));
        assert_eq!(plist_entry!("@group \t\n ")?, PlistEntry::Group(None));

        /*
         * Strip leading whitespace from a valid argument.
         */
        let p1 = plist_entry!("@comment  hi")?;
        let p2 = PlistEntry::Comment(Some(Cow::Borrowed(OsStr::new("hi"))));
        assert_eq!(p1, p2);

        /*
         * Any leading whitespace means the line is treated as a filename.
         */
        let p1 = plist_entry!(" @comment ")?;
        let p2 = PlistEntry::File(Cow::Borrowed(Path::new(" @comment ")));
        assert_eq!(p1, p2);

        Ok(())
    }

    /*
     * Plist commands that only support strict UTF-8 input.
     */
    #[test]
    fn test_utf8() -> Result<()> {
        valid_utf8_opt!("@mode ", PlistEntry::Mode);
        valid_utf8_opt!("@owner ", PlistEntry::Owner);
        valid_utf8_opt!("@group ", PlistEntry::Group);

        valid_utf8!("@name ", PlistEntry::Name);
        valid_utf8!("@pkgdep ", PlistEntry::PkgDep);
        valid_utf8!("@blddep ", PlistEntry::BldDep);
        valid_utf8!("@pkgcfl ", PlistEntry::PkgCfl);

        Ok(())
    }

    /*
     * Plist commands that must support ISO-8859 characters that are invalid
     * UTF-8 sequences.  This is mostly to support filenames and DESCR files
     * that still use (mostly European) ISO-8859 characters.
     */
    #[test]
    fn test_8859() -> Result<()> {
        valid_path!("", PlistEntry::File);
        valid_path!("@cwd ", PlistEntry::Cwd);
        valid_osstr!("@exec ", PlistEntry::Exec);
        valid_osstr!("@unexec ", PlistEntry::UnExec);
        valid_path!("@pkgdir ", PlistEntry::PkgDir);
        valid_path!("@dirrm ", PlistEntry::DirRm);
        valid_path!("@display ", PlistEntry::Display);
        valid_osstr_opt!("@comment ", PlistEntry::Comment);

        Ok(())
    }

    /*
     * Check for valid argument processing.
     */
    #[test]
    fn test_args() -> Result<()> {
        /*
         * Commands that must not contain arguments.
         */
        plist_match_ok!("@ignore", PlistEntry::Ignore);
        plist_match_error!("@ignore hi", PlistError::IncorrectArguments);

        /*
         * Commands that must contain an argument.
         */
        plist_match_ok_arg!("@cwd /cwd", PlistEntry::Cwd);
        plist_match_ok_arg!("@src /cwd", PlistEntry::Cwd);
        plist_match_ok_arg!("@cd /cwd", PlistEntry::Cwd);
        plist_match_ok_arg!("@exec echo hi", PlistEntry::Exec);
        plist_match_ok_arg!("@unexec echo lo", PlistEntry::UnExec);
        plist_match_ok_arg!("@name pkgname", PlistEntry::Name);
        plist_match_ok_arg!("@pkgdir /dirname", PlistEntry::PkgDir);
        plist_match_ok_arg!("@dirrm /dirname", PlistEntry::DirRm);
        plist_match_ok_arg!("@display MESSAGE", PlistEntry::Display);
        plist_match_ok_arg!("@pkgdep pkgname", PlistEntry::PkgDep);
        plist_match_ok_arg!("@blddep pkgname", PlistEntry::BldDep);
        plist_match_ok_arg!("@pkgcfl pkgname", PlistEntry::PkgCfl);
        plist_match_error!("@cwd", PlistError::IncorrectArguments);
        plist_match_error!("@src", PlistError::IncorrectArguments);
        plist_match_error!("@cd", PlistError::IncorrectArguments);
        plist_match_error!("@exec", PlistError::IncorrectArguments);
        plist_match_error!("@unexec", PlistError::IncorrectArguments);
        plist_match_error!("@name", PlistError::IncorrectArguments);
        plist_match_error!("@pkgdir", PlistError::IncorrectArguments);
        plist_match_error!("@dirrm", PlistError::IncorrectArguments);
        plist_match_error!("@display", PlistError::IncorrectArguments);
        plist_match_error!("@pkgdep", PlistError::IncorrectArguments);
        plist_match_error!("@blddep", PlistError::IncorrectArguments);
        plist_match_error!("@pkgcfl", PlistError::IncorrectArguments);

        /*
         * Commands where arguments are optional.
         */
        plist_match_ok_arg!("@comment", PlistEntry::Comment);
        plist_match_ok_arg!("@comment hi there", PlistEntry::Comment);
        plist_match_ok_arg!("@mode", PlistEntry::Mode);
        plist_match_ok_arg!("@mode 0644", PlistEntry::Mode);
        plist_match_ok_arg!("@owner", PlistEntry::Owner);
        plist_match_ok_arg!("@owner root", PlistEntry::Owner);
        plist_match_ok_arg!("@group", PlistEntry::Group);
        plist_match_ok_arg!("@group wheel", PlistEntry::Group);

        /*
         * Commands that require specific arguments.
         */
        plist_match_ok_arg!("@option preserve", PlistEntry::PkgOpt);
        plist_match_error!("@option", PlistError::IncorrectArguments);
        plist_match_error!("@option invalid", PlistError::UnsupportedCommand);

        Ok(())
    }

    /*
     * Test functions that return vectors.
     */
    #[test]
    fn test_vecs() -> Result<()> {
        let plist = plist!("@pkgdir one\n@pkgdir two\n@pkgdir three")?;
        assert_eq!(
            plist.pkgdirs().collect::<Vec<_>>(),
            ["one", "two", "three"]
        );

        let plist = plist!("@dirrm one\n@dirrm two\n@dirrm three")?;
        assert_eq!(
            plist.pkgrmdirs().collect::<Vec<_>>(),
            ["one", "two", "three"]
        );

        let plist = plist!("@pkgdep one\n@pkgdep two\n@pkgdep three")?;
        assert_eq!(
            plist.depends().collect::<Vec<_>>(),
            ["one", "two", "three"]
        );

        let plist = plist!("@blddep one\n@blddep two\n@blddep three")?;
        assert_eq!(
            plist.build_depends().collect::<Vec<_>>(),
            ["one", "two", "three"]
        );

        let plist = plist!("@pkgcfl one\n@pkgcfl two\n@pkgcfl three")?;
        assert_eq!(
            plist.conflicts().collect::<Vec<_>>(),
            ["one", "two", "three"]
        );

        Ok(())
    }

    /*
     * Test functions that return file matches.
     */
    #[test]
    fn test_files() -> Result<()> {
        let input = indoc! {"
            @cwd /opt/pkg
            bin/good
            @cwd /
            bin/evil
            @ignore
            @cwd /tmp
            +IGNORE_ME
            @cwd /opt/pkg
            bin/ok
        "};
        let plist = Plist::from_bytes(input.as_bytes())?;
        let files: Vec<&Path> = plist.files().collect();
        assert_eq!(
            files,
            [
                Path::new("bin/good"),
                Path::new("bin/evil"),
                Path::new("bin/ok")
            ]
        );
        let prefixed: Vec<PathBuf> = plist.files_prefixed().collect();
        assert_eq!(
            prefixed,
            [
                PathBuf::from("/opt/pkg/bin/good"),
                PathBuf::from("/bin/evil"),
                PathBuf::from("/opt/pkg/bin/ok")
            ]
        );

        let plist = Plist::from_bytes(b"bin/relative\n")?;
        let files: Vec<&Path> = plist.files().collect();
        assert_eq!(files, [Path::new("bin/relative")]);
        let prefixed: Vec<PathBuf> = plist.files_prefixed().collect();
        assert_eq!(prefixed, [PathBuf::from("bin/relative")]);
        Ok(())
    }

    /*
     * Test functions that return only the first match.
     */
    #[test]
    fn test_first_match() -> Result<()> {
        let plist = plist!("@comment not a pkgname")?;
        assert_eq!(plist.pkgname(), None);

        let plist = plist!("@name one\n@name two\n@name three")?;
        assert_eq!(plist.pkgname(), Some("one"));

        let plist = plist!("@comment not a display")?;
        assert_eq!(plist.display(), None);

        let plist = plist!("@display one\n@display two\n@display three")?;
        assert_eq!(plist.display(), Some(Path::new("one")));

        Ok(())
    }

    /*
     * Test that is_preserve() functions correctly.
     */
    #[test]
    fn test_preserve() -> Result<()> {
        assert!(!plist!("@comment not set")?.is_preserve());
        assert!(plist!("@option preserve")?.is_preserve());

        Ok(())
    }

    /*
     * Test MD5 checksum parsing from @comment MD5:...
     */
    #[test]
    fn test_file_checksum() -> Result<()> {
        // Valid MD5 checksum (32 hex chars)
        let entry =
            plist_entry!("@comment MD5:d41d8cd98f00b204e9800998ecf8427e")?;
        assert_eq!(
            entry,
            PlistEntry::FileChecksum(Cow::Borrowed(
                "d41d8cd98f00b204e9800998ecf8427e"
            ))
        );

        // Invalid MD5 (too short) - treated as regular comment
        let entry = plist_entry!("@comment MD5:abc123")?;
        assert!(matches!(entry, PlistEntry::Comment(_)));

        // Invalid MD5 (non-hex chars) - treated as regular comment
        let entry =
            plist_entry!("@comment MD5:d41d8cd98f00b204e9800998ecf8427g")?;
        assert!(matches!(entry, PlistEntry::Comment(_)));

        // Regular comment should still work
        let entry = plist_entry!("@comment This is a comment")?;
        assert!(matches!(entry, PlistEntry::Comment(_)));

        Ok(())
    }

    /*
     * Test symlink target parsing from @comment Symlink:...
     */
    #[test]
    fn test_symlink_target() -> Result<()> {
        let entry = plist_entry!("@comment Symlink:/usr/bin/target")?;
        assert_eq!(
            entry,
            PlistEntry::SymlinkTarget(Cow::Borrowed(Path::new(
                "/usr/bin/target"
            )))
        );

        // Empty symlink target
        let entry = plist_entry!("@comment Symlink:")?;
        assert_eq!(
            entry,
            PlistEntry::SymlinkTarget(Cow::Borrowed(Path::new("")))
        );

        Ok(())
    }

    /*
     * Test files_with_info() returns files with associated metadata.
     */
    #[test]
    fn test_files_with_info() -> Result<()> {
        let input = indoc! {"
            @mode 0755
            @owner root
            @group wheel
            bin/myapp
            @comment MD5:d41d8cd98f00b204e9800998ecf8427e
            @mode 0644
            etc/myapp.conf
            @comment MD5:098f6bcd4621d373cade4e832627b4f6
            lib/libfoo.so
            @comment Symlink:libfoo.so.1
            @ignore
            +BUILD_INFO
        "};

        let plist = Plist::from_bytes(input.as_bytes())?;
        let files: Vec<FileInfo> = plist.files_with_info().collect();

        assert_eq!(files.len(), 3);

        // First file: bin/myapp with mode 0755, owner root, group wheel
        assert_eq!(files[0].path, PathBuf::from("bin/myapp"));
        assert_eq!(
            files[0].checksum,
            Some("d41d8cd98f00b204e9800998ecf8427e".to_string())
        );
        assert_eq!(files[0].symlink_target, None);
        assert_eq!(files[0].mode, Some("0755".to_string()));
        assert_eq!(files[0].owner, Some("root".to_string()));
        assert_eq!(files[0].group, Some("wheel".to_string()));

        // Second file: etc/myapp.conf with mode 0644
        assert_eq!(files[1].path, PathBuf::from("etc/myapp.conf"));
        assert_eq!(
            files[1].checksum,
            Some("098f6bcd4621d373cade4e832627b4f6".to_string())
        );
        assert_eq!(files[1].mode, Some("0644".to_string()));

        // Third file: lib/libfoo.so is a symlink
        assert_eq!(files[2].path, PathBuf::from("lib/libfoo.so"));
        assert_eq!(files[2].checksum, None);
        assert_eq!(files[2].symlink_target, Some(PathBuf::from("libfoo.so.1")));

        Ok(())
    }

    #[test]
    fn test_into_iterator() -> Result<()> {
        let plist =
            plist!("@name pkg-1.0\nbin/foo\n@pkgdep dep-[0-9]*\nbin/bar")?;

        let entries: Vec<_> = plist.into_iter().collect();
        assert_eq!(entries.len(), 4);
        assert!(matches!(entries[0], PlistEntry::Name(_)));
        assert!(matches!(entries[1], PlistEntry::File(_)));
        assert!(matches!(entries[2], PlistEntry::PkgDep(_)));
        assert!(matches!(entries[3], PlistEntry::File(_)));

        Ok(())
    }

    #[test]
    fn test_iter_by_ref() -> Result<()> {
        let plist = plist!("@name pkg-1.0\nbin/foo\nbin/bar")?;

        let file_count = (&plist)
            .into_iter()
            .filter(|e| matches!(e, PlistEntry::File(_)))
            .count();
        assert_eq!(file_count, 2);

        // plist is still usable after iteration by reference
        assert_eq!(plist.pkgname(), Some("pkg-1.0"));

        Ok(())
    }

    /*
     * Lazy parser tests: parse() yields borrowed entries, validating
     * UTF-8 inline for variants whose payloads are typed as Cow<str>.
     */
    #[test]
    fn test_parse_iter() -> Result<()> {
        let input = b"@name pkg-1.0\nbin/foo\n@pkgdir /var/db/x\n";
        let entries: Vec<_> = parse(input).collect::<Result<Vec<_>>>()?;
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], PlistEntry::Name(Cow::Borrowed("pkg-1.0")));
        assert_eq!(
            entries[1],
            PlistEntry::File(Cow::Borrowed(Path::new("bin/foo")))
        );
        assert_eq!(
            entries[2],
            PlistEntry::PkgDir(Cow::Borrowed(Path::new("/var/db/x")))
        );
        Ok(())
    }

    #[test]
    fn test_parse_iter_no_trailing_newline() -> Result<()> {
        let input = b"@name pkg-1.0\nbin/foo";
        let entries: Vec<_> = parse(input).collect::<Result<Vec<_>>>()?;
        assert_eq!(entries.len(), 2);
        Ok(())
    }

    #[test]
    fn test_parse_iter_skips_blanks() -> Result<()> {
        let input = b"@name pkg-1.0\n\n   \nbin/foo\n";
        let entries: Vec<_> = parse(input).collect::<Result<Vec<_>>>()?;
        assert_eq!(entries.len(), 2);
        Ok(())
    }

    #[test]
    fn test_parse_comment_special_forms() -> Result<()> {
        let entries: Vec<_> = parse(
            b"@comment MD5:d41d8cd98f00b204e9800998ecf8427e\n\
              @comment Symlink:/usr/bin/target\n\
              @comment plain comment\n",
        )
        .collect::<Result<Vec<_>>>()?;
        assert_eq!(
            entries[0],
            PlistEntry::FileChecksum(Cow::Borrowed(
                "d41d8cd98f00b204e9800998ecf8427e"
            ))
        );
        assert_eq!(
            entries[1],
            PlistEntry::SymlinkTarget(Cow::Borrowed(Path::new(
                "/usr/bin/target"
            )))
        );
        assert!(matches!(entries[2], PlistEntry::Comment(Some(_))));
        Ok(())
    }

    /*
     * parse() validates UTF-8 inline for the Cow<str>-typed variants;
     * malformed input yields PlistError::Utf8 immediately rather than
     * propagating bad bytes downstream.
     */
    #[test]
    fn test_parse_validates_utf8() -> Result<()> {
        let input = b"@name \xff-bad\nbin/foo\n";
        match parse(input).next() {
            Some(Err(PlistError::Utf8(_))) => Ok(()),
            other => panic!("expected Utf8 error from parse(), got {other:?}"),
        }
    }

    /*
     * into_owned() turns a borrowed entry into a 'static one, suitable
     * for storing past the lifetime of the source bytes.
     */
    #[test]
    fn test_into_owned() -> Result<()> {
        let owned: PlistEntry<'static> = {
            let bytes: Vec<u8> = b"@name pkg-1.0".to_vec();
            PlistEntry::from_bytes(&bytes)?.into_owned()
        };
        assert_eq!(owned, PlistEntry::Name(Cow::Owned("pkg-1.0".to_owned())));
        Ok(())
    }
}
