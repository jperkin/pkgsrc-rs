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
 * Packing list parsing and generation.
 *
 * Packing lists, commonly referred to as plists and named `PLIST` in pkgsrc
 * package directories, contain a list of files contents that are installed by
 * a package.  They also support a limited number of commands that configure
 * additional package metadata, as well as setting file permissions and
 * performing install and deinstall commands for extracted files.
 *
 * As plists can contain data that is not UTF-8 clean (for example ISO-8859
 * filenames), the primary interfaces for parsing input are the `from_bytes()`
 * functions for both [`PlistEntry`] and [`Plist`].
 *
 * Where possible, [`PlistEntry`] types are represented by [`String`] for
 * simpler handling (and enforced UTF-8 correctness), otherwise [`OsString`]
 * is used.
 *
 * A [`PlistEntry`] is an enum representing a single line in a plist, and a
 * [`Plist`] is a collection of [`PlistEntry`] making up a complete plist.
 *
 * Once a [`Plist`] has been configured, various functions allow examination of
 * the parsed data.
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
 *      assert_eq!(pkglist.depends().len(), 2);
 *      assert_eq!(pkglist.build_depends().len(), 2);
 *      assert_eq!(pkglist.conflicts().len(), 1);
 *      assert_eq!(pkglist.pkgdirs().len(), 1);
 *      assert_eq!(pkglist.pkgrmdirs().len(), 1);
 *
 *      Ok(())
 * }
 * ```
 */
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::string::FromUtf8Error;

#[cfg(test)]
use indoc::indoc;

/**
 * A type alias for the result from the creation of either a [`PlistEntry`] or
 * a [`Plist`], with [`Error`] returned in [`Err`] variants.
 */
pub type Result<T> = std::result::Result<T, PlistError>;

/**
 * Error type containing possible parse failures.
 */
#[derive(Debug)]
pub enum PlistError {
    /**
     * An unsupported `@command` string, or an unsupported argument to a command
     * that requires specific values (for example `@option preserve`).
     */
    UnsupportedCommand(OsString),
    /**
     * Incorrect number of arguments, or incorrect argument passed to a command
     * that requires a specific format.
     */
    IncorrectArguments(OsString),
    /**
     * Wrapped [`FromUtf8Error`] error when failing to parse valid UTF-8.
     */
    Utf8(FromUtf8Error),
}

impl fmt::Display for PlistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlistError::UnsupportedCommand(s) => {
                write!(f, "unsupported plist command: {}", s.to_string_lossy())
            }
            PlistError::IncorrectArguments(s) => write!(
                f,
                "incorrect command arguments: {}",
                s.to_string_lossy()
            ),
            PlistError::Utf8(s) => {
                write!(f, "invalid UTF-8 sequence: {}", s.utf8_error())
            }
        }
    }
}

impl Error for PlistError {}

impl From<FromUtf8Error> for PlistError {
    fn from(err: FromUtf8Error) -> Self {
        PlistError::Utf8(err)
    }
}

/**
 * A single plist entry.
 *
 * Entries can be constructed either by using [`from_bytes()`] to parse an
 * array of bytes from a plist, or by constructing one of the entries manually.
 *
 * ## Examples
 *
 * ```
 * use pkgsrc::plist::{Result,PlistEntry};
 * use std::ffi::OsString;
 *
 * fn main() -> Result<()> {
 *     /*
 *      * Entries differ in whether they take String or OsString arguments and
 *      * whether or not they are wrapped in Option, check the documentation!
 *      */
 *     let p1 = PlistEntry::from_bytes(String::from("@comment hi").as_bytes())?;
 *     let p2 = PlistEntry::Comment(Some(OsString::from("hi")));
 *
 *     assert_eq!(p1, p2);
 *
 *     Ok(())
 * }
 * ```
 *
 * [`from_bytes()`]: PlistEntry::from_bytes
 */
#[derive(Debug, Eq, PartialEq)]
pub enum PlistEntry {
    /**
     * Filename to extract relative to the current working directory.
     */
    File(OsString),
    /**
     * Set the internal directory pointer.  All subsequent filenames will be
     * assumed relative to this directory.
     */
    Cwd(OsString),
    /**
     * Execute command as part of the unpacking process.
     */
    Exec(OsString),
    /**
     * Execute command as part of the deinstallation process.
     */
    UnExec(OsString),
    /**
     * Set default permission for all subsequently extracted files.
     */
    Mode(Option<String>),
    /**
     * Set internal package options.  Named Opt to avoid conflict with Rust
     * "Option".
     */
    PkgOpt(PlistOption),
    /**
     * Set default ownership for all subsequently extracted files to specified
     * user.
     */
    Owner(Option<String>),
    /**
     * Set default group ownership for all subsequently extracted files to
     * specified group.
     */
    Group(Option<String>),
    /**
     * Embed a comment in the packing list.  While specified as mandatory in
     * the manual page, in practise it is not (e.g. `print-PLIST`).
     */
    Comment(Option<OsString>),
    /**
     * Used internally to tell extraction to ignore the next file.
     */
    Ignore,
    /**
     * Set the name of the package.
     */
    Name(String),
    /**
     * Declare directory name as managed.
     */
    PkgDir(OsString),
    /**
     * If directory name exists, it will be deleted at deinstall time.
     */
    DirRm(OsString),
    /**
     * Declare name as the file to be displayed at install time.
     */
    Display(OsString),
    /**
     * Declare a dependency on the pkgname package.
     */
    PkgDep(String),
    /**
     * Declare that this package was built with the exact version of pkgname.
     */
    BldDep(String),
    /**
     * Declare a conflict with the pkgcflname package.
     */
    PkgCfl(String),
}

/**
 * List of valid arguments for the `@option` command.  Currently the only
 * supported argument is `preserve`.
 */
#[derive(Debug, Eq, PartialEq)]
pub enum PlistOption {
    /**
     * Indicates that any existing files should be moved out of the way before
     * the package contents are install (and subsequently restored when the
     * contents are uninstalled).
     */
    Preserve,
}

macro_rules! plist_args_str {
    ($s:ident, $p:path, $l:ident) => {
        match $s {
            Some(s) => Ok($p(String::from_utf8(s.as_bytes().to_vec())?)),
            None => Err(PlistError::IncorrectArguments(OsString::from($l))),
        }
    };
}

macro_rules! plist_args_osstr {
    ($s:ident, $p:path, $l:ident) => {
        match $s {
            Some(dir) => Ok($p(OsString::from(dir))),
            None => Err(PlistError::IncorrectArguments(OsString::from($l))),
        }
    };
}

macro_rules! plist_args_str_opt {
    ($s:ident, $p:path) => {
        match $s {
            Some(s) => Ok($p(Some(String::from_utf8(s.as_bytes().to_vec())?))),
            None => Ok($p(None)),
        }
    };
}

macro_rules! plist_args_osstr_opt {
    ($s:ident, $p:path) => {
        match $s {
            Some(s) => Ok($p(Some(OsString::from(s)))),
            None => Ok($p(None)),
        }
    };
}

impl PlistEntry {
    /**
     * Construct a new [`PlistEntry`] from a stream of bytes representing a
     * line from a package list.
     */
    pub fn from_bytes(bytes: &[u8]) -> Result<PlistEntry> {
        let line = OsStr::from_bytes(bytes);
        let end = bytes.len();

        /*
         * Look for the first space character to split on, then convert the
         * first part to UTF-8 to simplify processing.  We ensure non-UTF-8
         * characters are handled correctly later.  If there are no spaces then
         * use the entire line.
         */
        let bytes = &bytes[0..end];
        let (mut idx, cmd) = match bytes.iter().position(|&c| c == b' ') {
            Some(i) => (i, String::from_utf8_lossy(&bytes[0..i]).into_owned()),
            None => (0, String::from_utf8_lossy(bytes).into_owned()),
        };

        /*
         * Set optional arguments if anything exists after the first space,
         * after first removing any leading whitespace.
         */
        let args = if idx == 0 || idx + 1 >= end {
            None
        } else {
            for c in &bytes[idx..end] {
                if (*c as char).is_whitespace() {
                    idx += 1;
                    continue;
                }
                break;
            }
            if idx == end {
                None
            } else {
                Some(OsStr::from_bytes(&bytes[idx..end]))
            }
        };

        if cmd.starts_with('@') {
            match cmd.as_str() {
                /*
                 * @src and @cd are effectively aliases for @cwd.
                 */
                "@cwd" | "@src" | "@cd" => {
                    plist_args_osstr!(args, PlistEntry::Cwd, line)
                }
                "@exec" => plist_args_osstr!(args, PlistEntry::Exec, line),
                "@unexec" => plist_args_osstr!(args, PlistEntry::UnExec, line),

                /*
                 * Currently "preserve" is the only valid option.
                 */
                "@option" => match args.and_then(OsStr::to_str) {
                    Some("preserve") => {
                        Ok(PlistEntry::PkgOpt(PlistOption::Preserve))
                    }
                    Some(_) => {
                        Err(PlistError::UnsupportedCommand(OsString::from(cmd)))
                    }
                    None => Err(PlistError::IncorrectArguments(
                        OsString::from(line),
                    )),
                },

                /*
                 * File ownership and permissions are allowed to be unset,
                 * indicating that they return to their respective defaults.
                 */
                "@mode" => plist_args_str_opt!(args, PlistEntry::Mode),
                "@owner" => plist_args_str_opt!(args, PlistEntry::Owner),
                "@group" => plist_args_str_opt!(args, PlistEntry::Group),

                /*
                 * Whilst the manual page specifies that @comment takes an
                 * argument, it's too pedantic to insist that it must, so we
                 * handle it as an optional argument.
                 *
                 * Must be an OsString as often contains filenames.
                 */
                "@comment" => plist_args_osstr_opt!(args, PlistEntry::Comment),

                /*
                 * For now be strict that "@ignore" must not take arguments.
                 */
                "@ignore" => match args {
                    Some(_) => Err(PlistError::IncorrectArguments(
                        OsString::from(line),
                    )),
                    None => Ok(PlistEntry::Ignore),
                },

                /*
                 * Contain strict package names so must be UTF-8 clean.
                 */
                "@name" => plist_args_str!(args, PlistEntry::Name, line),
                "@pkgdep" => plist_args_str!(args, PlistEntry::PkgDep, line),
                "@blddep" => plist_args_str!(args, PlistEntry::BldDep, line),
                "@pkgcfl" => plist_args_str!(args, PlistEntry::PkgCfl, line),

                /*
                 * Contain files/directories so need to support OsString.
                 */
                "@pkgdir" => plist_args_osstr!(args, PlistEntry::PkgDir, line),
                "@dirrm" => plist_args_osstr!(args, PlistEntry::DirRm, line),
                "@display" => {
                    plist_args_osstr!(args, PlistEntry::Display, line)
                }

                _ => Err(PlistError::UnsupportedCommand(OsString::from(cmd))),
            }
        } else {
            Ok(PlistEntry::File(OsString::from(OsStr::from_bytes(bytes))))
        }
    }
}

/**
 * A complete list of [`PlistEntry`] entries.
 *
 * Entries are parsed using [`from_bytes()`].
 *
 * See the top for a full example.
 *
 * ## Examples
 *
 * ```
 * use pkgsrc::plist::{Result,Plist};
 * use std::ffi::OsString;
 *
 * fn main() -> Result<()> {
 *     let p1 = Plist::from_bytes(String::from("@name pkg-1.0").as_bytes())?;
 *     assert_eq!(p1.pkgname(), Some("pkg-1.0"));
 *     Ok(())
 * }
 * ```
 *
 * [`from_bytes()`]: Plist::from_bytes
 */
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Plist {
    entries: Vec<PlistEntry>,
}

macro_rules! plist_match_filter_str {
    ($s:ident, $p:path) => {
        $s.entries
            .iter()
            .filter_map(|entry| match entry {
                $p(s) => Some(s.as_str()),
                _ => None,
            })
            .collect()
    };
}

macro_rules! plist_match_filter_osstr {
    ($s:ident, $p:path) => {
        $s.entries
            .iter()
            .filter_map(|entry| match entry {
                $p(s) => Some(s.as_os_str()),
                _ => None,
            })
            .collect()
    };
}

macro_rules! plist_find_first_str {
    ($s:ident, $p:path) => {
        $s.entries.iter().find_map(|entry| match entry {
            $p(s) => Some(s.as_str()),
            _ => None,
        })
    };
}

macro_rules! plist_find_first_osstr {
    ($s:ident, $p:path) => {
        $s.entries.iter().find_map(|entry| match entry {
            $p(s) => Some(s.as_os_str()),
            _ => None,
        })
    };
}

impl Plist {
    /**
     * Return an empty new [`Plist`].
     */
    pub fn new() -> Plist {
        let plist: Plist = Default::default();
        plist
    }

    /**
     * Construct a new [`Plist`] from a stream of bytes representing lines
     * from a package list.
     */
    pub fn from_bytes(bytes: &[u8]) -> Result<Plist> {
        let mut plist = Plist::new();

        /*
         * Look through the byte stream, splitting entries on newlines, and
         * account for leading whitespace in order to skip any blank lines.
         */
        let mut lines: Vec<(usize, usize)> = Vec::new();
        let mut start = 0;
        let mut tstart = 0;
        let mut trim = true;
        let mut end = 0;
        for (idx, ch) in bytes.iter().enumerate() {
            if *ch == b'\n' {
                /*
                 * Valid line containing non-whitespace characters.
                 */
                if start < idx && tstart + 1 < idx {
                    lines.push((start, idx));
                }
                /*
                 * Reset for next line.
                 */
                start = idx + 1;
                end = start;
                tstart = start;
                trim = true;
            } else if trim && (*ch as char).is_whitespace() {
                /*
                 * Account for leading whitespace.
                 */
                tstart += 1;
            } else {
                /*
                 * Stop on first non-whitespace character.
                 */
                trim = false;
            }
        }
        /*
         * Handle any trailing lines that do not contain newlines.
         */
        if end < bytes.len() && tstart < bytes.len() {
            lines.push((start, bytes.len()));
        }

        /*
         * Parse all valid entries that we've found.
         */
        for (start, end) in lines {
            plist
                .entries
                .push(PlistEntry::from_bytes(&bytes[start..end])?);
        }

        Ok(plist)
    }

    /**
     * Return the package name as specified with `@name`.  If multiple entries
     * are found only the first is returned.  This is wrapped in [`Option`] as
     * while indicated as mandatory in the manual page it is often left out,
     * deferring to deriving the package name from the file name instead.
     */
    pub fn pkgname(&self) -> Option<&str> {
        plist_find_first_str!(self, PlistEntry::Name)
    }

    /**
     * Return the optional package display file (i.e. `MESSAGE`) as specified
     * with `@display`.  If multiple entries are found only the first is
     * returned.
     */
    pub fn display(&self) -> Option<&OsStr> {
        plist_find_first_osstr!(self, PlistEntry::Display)
    }

    /**
     * Return a vector containing `@pkgdep` entries as string slices.
     */
    pub fn depends(&self) -> Vec<&str> {
        plist_match_filter_str!(self, PlistEntry::PkgDep)
    }

    /**
     * Return a vector containing `@blddep` entries as string slices.
     */
    pub fn build_depends(&self) -> Vec<&str> {
        plist_match_filter_str!(self, PlistEntry::BldDep)
    }

    /**
     * Return a vector containing `@pkgcfl` entries as string slices.
     */
    pub fn conflicts(&self) -> Vec<&str> {
        plist_match_filter_str!(self, PlistEntry::PkgCfl)
    }

    /**
     * Return a vector containing `@pkgdir` entries as string slices.
     */
    pub fn pkgdirs(&self) -> Vec<&OsStr> {
        plist_match_filter_osstr!(self, PlistEntry::PkgDir)
    }

    /**
     * Return a vector containing `@dirrm` entries as string slices.
     */
    pub fn pkgrmdirs(&self) -> Vec<&OsStr> {
        plist_match_filter_osstr!(self, PlistEntry::DirRm)
    }

    /**
     * Return a vector containing a list of file entries as string slices.  Any
     * files that come after an "@ignore" command are not listed.
     */
    pub fn files(&self) -> Vec<&OsStr> {
        let mut ignore = false;
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                PlistEntry::Ignore => {
                    ignore = true;
                    None
                }
                PlistEntry::File(file) => {
                    if ignore {
                        ignore = false;
                        None
                    } else {
                        Some(file.as_os_str())
                    }
                }
                _ => None,
            })
            .collect()
    }

    /**
     * Return a vector containing a list of file entries including their prefix
     * (as set by `@cwd`) as OsStrings.  Any files that come after an "@ignore"
     * command are not listed.
     */
    pub fn files_prefixed(&self) -> Vec<OsString> {
        let mut ignore = false;
        let mut prefix: Option<OsString> = None;
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                PlistEntry::Cwd(dir) => {
                    prefix = Some(dir.to_os_string());
                    None
                }
                PlistEntry::Ignore => {
                    ignore = true;
                    None
                }
                PlistEntry::File(file) => {
                    if ignore {
                        ignore = false;
                        None
                    } else {
                        let mut path = OsString::new();
                        if let Some(pfx) = &prefix {
                            path.push(pfx);
                        }
                        if !path.to_string_lossy().ends_with('/') {
                            path.push("/");
                        }
                        path.push(file);
                        Some(path)
                    }
                }
                _ => None,
            })
            .collect()
    }

    /**
     * Return a vector containing a list of PlistEntry entries that are used
     * during an install procedure.  It is up to the caller to keep track of
     * file metadata.
     */
    pub fn install_cmds(&self) -> Vec<&PlistEntry> {
        let mut ignore = false;
        self.entries
            .iter()
            .filter(|entry| match entry {
                /*
                 * Ignore the next file, usually (always?) a +METADATA file.
                 */
                PlistEntry::Ignore => {
                    ignore = true;
                    false
                }
                PlistEntry::File(_) => {
                    if ignore {
                        ignore = false;
                        false
                    } else {
                        true
                    }
                }
                PlistEntry::Cwd(_)
                | PlistEntry::Exec(_)
                | PlistEntry::Mode(_)
                | PlistEntry::Owner(_)
                | PlistEntry::Group(_)
                | PlistEntry::PkgDir(_) => true,
                _ => false,
            })
            .collect()
    }

    /**
     * Return a vector containing a list of PlistEntry entries that are used
     * during an uninstall procedure.  It is up to the caller to keep track of
     * file metadata.
     */
    pub fn uninstall_cmds(&self) -> Vec<&PlistEntry> {
        let mut ignore = false;
        self.entries
            .iter()
            .filter(|entry| match entry {
                /*
                 * Ignore the next file, usually (always?) a +METADATA file.
                 */
                PlistEntry::Ignore => {
                    ignore = true;
                    false
                }
                PlistEntry::File(_) => {
                    if ignore {
                        ignore = false;
                        false
                    } else {
                        true
                    }
                }
                PlistEntry::Cwd(_)
                | PlistEntry::UnExec(_)
                | PlistEntry::Mode(_)
                | PlistEntry::Owner(_)
                | PlistEntry::Group(_)
                | PlistEntry::PkgDir(_)
                | PlistEntry::DirRm(_) => true,
                _ => false,
            })
            .collect()
    }

    /**
     * Return bool indicating whether `@option preserve` has been set or not.
     */
    pub fn is_preserve(&self) -> bool {
        self.entries
            .iter()
            .filter(|entry| {
                matches!(entry, PlistEntry::PkgOpt(PlistOption::Preserve))
            })
            .count()
            > 0
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
     * Plist commands that only accept strict UTF-8 Strings.
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
            assert_eq!(PlistEntry::from_bytes(&t)?, $p(String::from("💖")));

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
     * Plist commands that only accept optional strict UTF-8 Strings.
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
                $p(Some(String::from("💖")))
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
     * Plist commands that accept ISO-8859 input.
     */
    macro_rules! valid_8859 {
        ($s:expr, $p:path) => {
            /*
             * A UTF-8 sparkle heart as used in the Rust documentation, and a
             * Norwegian "ø" in non-UTF-8 compatible ISO-8859 format.
             */
            let heart = vec![240, 159, 146, 150];
            let oe = vec![0xf8];

            let mut t = String::from($s).into_bytes();
            t.extend_from_slice(&heart);
            assert_eq!(PlistEntry::from_bytes(&t)?, $p(OsString::from("💖")));
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
     * Plist commands that accept optional ISO-8859 input.
     */
    macro_rules! valid_8859_opt {
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
                $p(Some(OsString::from("💖")))
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
        assert_eq!(plist.depends().len(), 2);
        assert_eq!(plist.build_depends().len(), 2);
        assert_eq!(plist.conflicts().len(), 2);
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
        let p2 = PlistEntry::Comment(Some(OsString::from("hi")));
        assert_eq!(p1, p2);

        /*
         * Any leading whitespace means the line is treated as a filename.
         */
        let p1 = plist_entry!(" @comment ")?;
        let p2 = PlistEntry::File(OsString::from(" @comment "));
        assert_eq!(p1, p2);

        Ok(())
    }

    /*
     * Plist commands that only support strict UTF-8 input and are stored as
     * String types.
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
        valid_8859!("", PlistEntry::File);
        valid_8859!("@cwd ", PlistEntry::Cwd);
        valid_8859!("@exec ", PlistEntry::Exec);
        valid_8859!("@unexec ", PlistEntry::UnExec);
        valid_8859!("@pkgdir ", PlistEntry::PkgDir);
        valid_8859!("@dirrm ", PlistEntry::DirRm);
        valid_8859!("@display ", PlistEntry::Display);
        valid_8859_opt!("@comment ", PlistEntry::Comment);

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
        assert_eq!(plist.pkgdirs(), ["one", "two", "three"]);

        let plist = plist!("@dirrm one\n@dirrm two\n@dirrm three")?;
        assert_eq!(plist.pkgrmdirs(), ["one", "two", "three"]);

        let plist = plist!("@pkgdep one\n@pkgdep two\n@pkgdep three")?;
        assert_eq!(plist.depends(), ["one", "two", "three"]);

        let plist = plist!("@blddep one\n@blddep two\n@blddep three")?;
        assert_eq!(plist.build_depends(), ["one", "two", "three"]);

        let plist = plist!("@pkgcfl one\n@pkgcfl two\n@pkgcfl three")?;
        assert_eq!(plist.conflicts(), ["one", "two", "three"]);

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
        assert_eq!(plist.files(), ["bin/good", "bin/evil", "bin/ok"]);
        assert_eq!(
            plist.files_prefixed(),
            ["/opt/pkg/bin/good", "/bin/evil", "/opt/pkg/bin/ok"]
        );
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
        assert_eq!(plist.display(), Some(OsString::from("one").as_os_str()));

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
}
