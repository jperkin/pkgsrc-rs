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
 *
 * plist.rs - package packlist metadata
 */

/**
 * Command of packlist (file entry or a supported "@" command).
 */
#[derive(Debug)]
pub enum PlistCommand {
    File,
    Cwd,
    Exec, /* pkg_install calls this PLIST_CMD */
    Chmod,
    Chown,
    Chgrp,
    Comment,
    Ignore,
    Name,
    UnExec,
    Src,
    Display,
    PkgDep,
    DirRm,
    Option,
    PkgCfl,
    BldDep,
    PkgDir,
}

impl Default for PlistCommand {
    fn default() -> PlistCommand {
        PlistCommand::File
    }
}

/**
 * A single line entry in a PLIST.
 */
#[derive(Debug, Default)]
pub struct PlistEntry {
    command: PlistCommand,
    argv: Vec<String>,
}

impl PlistEntry {
    /**
     * Return a new empty `PlistEntry` container.
     */
    pub fn new() -> PlistEntry {
        let entry: PlistEntry = Default::default();
        entry
    }
}

/**
 * A vector of `PlistEntry` entries making up a complete PLIST.
 */
#[derive(Debug, Default)]
pub struct Plist {
    entries: Vec<PlistEntry>,
}

impl Plist {
    /**
     * Return a new empty `Plist` container.
     */
    pub fn new() -> Plist {
        let plist: Plist = Default::default();
        plist
    }

    fn parse_command(
        &mut self,
        command: &str,
    ) -> Result<PlistCommand, &'static str> {
        match command {
            "@cwd" => Ok(PlistCommand::Cwd),
            /*
             * Documented as an alias in pkg_create(1) but does not appear to
             * actually be supported by pkg_install.  We support it regardless.
             */
            "@cd" => Ok(PlistCommand::Cwd),
            "@exec" => Ok(PlistCommand::Exec),
            "@mode" => Ok(PlistCommand::Chmod),
            "@owner" => Ok(PlistCommand::Chown),
            "@group" => Ok(PlistCommand::Chgrp),
            "@comment" => Ok(PlistCommand::Comment),
            "@ignore" => Ok(PlistCommand::Ignore),
            "@name" => Ok(PlistCommand::Name),
            "@unexec" => Ok(PlistCommand::UnExec),
            "@src" => Ok(PlistCommand::Src),
            "@display" => Ok(PlistCommand::Display),
            "@pkgdep" => Ok(PlistCommand::PkgDep),
            "@dirrm" => Ok(PlistCommand::DirRm),
            "@option" => Ok(PlistCommand::Option),
            "@pkgcfl" => Ok(PlistCommand::PkgCfl),
            "@blddep" => Ok(PlistCommand::BldDep),
            "@pkgdir" => Ok(PlistCommand::PkgDir),
            _ => Err("Unknown PLIST command"),
        }
    }

    /**
     * Read in a packlist (also known as `+CONTENTS`) as a complete string and
     * parse each line into separate `PlistEntry` entries in our `Plist`.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::Plist;
     *
     * let mut plist = Plist::new();
     * plist.add_entries("@comment testing\nbin/file\n@exec rm -rf /\n");
     * ```
     */
    pub fn add_entries(&mut self, file: &str) -> Result<(), &'static str> {
        for line in file.lines() {
            self.add_entry(line)?;
        }
        Ok(())
    }

    /**
     * Read in a line from a plist, parse it into a new `PlistEntry`, and push
     * that onto the end of our `Plist`.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::Plist;
     *
     * let mut plist = Plist::new();
     * plist.add_entry("@cwd hello");
     * plist.add_entry("bin/file");
     * plist.add_entry("@comment This is a comment");
     * plist.add_entry("@owner user");
     * plist.add_entry("@group group");
     * plist.add_entry("@option preserve");
     * plist.add_entry("@exec rm -rf /");
     * ```
     */
    pub fn add_entry(&mut self, line: &str) -> Result<(), &'static str> {
        let mut entry = PlistEntry::new();
        let linevec: Vec<&str> = line.splitn(2, ' ').collect();

        /*
         * Convert "@command" to PlistCommand::* type and set entry.command
         * accordingly.  Any entry that isn't a "@command" is a filename to
         * extract.
         */
        if line.starts_with('@') {
            match self.parse_command(linevec[0]) {
                Ok(cmd) => entry.command = cmd,
                Err(e) => return Err(e),
            }
        } else {
            entry.command = PlistCommand::File;
        }

        /*
         * Validate that the correct number of arguments have been supplied,
         * and populate entry.argv.
         */
        match entry.command {
            /*
             * Files can contain spaces so we just push the entire line.
             */
            PlistCommand::File => {
                entry.argv.push(line.to_string());
            }

            /*
             * Commands that must have zero arguments.
             */
            PlistCommand::Ignore => {
                if !(linevec.get(1).is_none()) {
                    return Err("PLIST command requires zero arguments");
                }
            }

            /*
             * Commands that must have exactly one argument.
             *
             * XXX: Note that we do not actually validate that only one
             * argument was passed, and instead push the rest of the line, as
             * it is not clear how spaces should be handled here (e.g. user/
             * group names and paths on Windows).
             */
            PlistCommand::Cwd
            | PlistCommand::Chmod
            | PlistCommand::Chown
            | PlistCommand::Chgrp
            | PlistCommand::Name
            | PlistCommand::Src
            | PlistCommand::Display
            | PlistCommand::PkgDep
            | PlistCommand::DirRm
            | PlistCommand::Option
            | PlistCommand::PkgCfl
            | PlistCommand::BldDep
            | PlistCommand::PkgDir => {
                if let Some(arg) = linevec.get(1) {
                    entry.argv.push(arg.to_string())
                } else {
                    return Err("PLIST command requires exactly 1 argument");
                }
            }

            /*
             * Commands that must have at least one argument.
             */
            PlistCommand::Exec
            | PlistCommand::Comment
            | PlistCommand::UnExec => {
                if let Some(arg) = linevec.get(1) {
                    entry.argv.push(arg.to_string())
                } else {
                    return Err("PLIST command requires at least 1 argument");
                }
            }
        }

        self.entries.push(entry);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /*
     * Ensure all supported @commands are recognised and enforce the
     * correct number of arguments.
     */
    fn test_valid_plist_commands() {
        let mut plist = Plist::new();

        /*
         * Filenames can contain spaces.  XXX: Do we want to perform
         * any other validation?
         */
        assert!(plist.add_entry("bin/true").is_ok());
        assert!(plist.add_entry("bin/this is also true").is_ok());
        /*
         * Commands that take exactly zero arguments.
         */
        assert!(plist.add_entry("@ignore").is_ok());
        assert!(plist.add_entry("@ignore this").is_err());
        /*
         * Commands that take exactly one argument.
         */
        assert!(plist.add_entry("@cwd").is_err());
        assert!(plist.add_entry("@cwd /t1").is_ok());
        assert!(plist.add_entry("@src").is_err());
        assert!(plist.add_entry("@src /t1").is_ok());
        assert!(plist.add_entry("@mode").is_err());
        assert!(plist.add_entry("@mode 0644").is_ok());
        // XXX: We may want to validate that the only supported option
        // "preserve" is specified here.
        assert!(plist.add_entry("@option").is_err());
        assert!(plist.add_entry("@option preserve").is_ok());
        assert!(plist.add_entry("@owner").is_err());
        assert!(plist.add_entry("@owner ben").is_ok());
        assert!(plist.add_entry("@group").is_err());
        assert!(plist.add_entry("@group ben").is_ok());
        assert!(plist.add_entry("@name").is_err());
        assert!(plist.add_entry("@name pkgname").is_ok());
        assert!(plist.add_entry("@pkgdir").is_err());
        assert!(plist.add_entry("@pkgdir /t1").is_ok());
        assert!(plist.add_entry("@dirrm").is_err());
        assert!(plist.add_entry("@dirrm /t1").is_ok());
        assert!(plist.add_entry("@display").is_err());
        assert!(plist.add_entry("@display file1").is_ok());
        assert!(plist.add_entry("@pkgdep").is_err());
        assert!(plist.add_entry("@pkgdep pkg1-[0-9]*").is_ok());
        assert!(plist.add_entry("@blddep").is_err());
        assert!(plist.add_entry("@blddep pkg1-[0-9]*").is_ok());
        assert!(plist.add_entry("@pkgcfl").is_err());
        assert!(plist.add_entry("@pkgcfl pkg1-[0-9]*").is_ok());
        /*
         * For now we do not assert these tests as it's not clear how they
         * should be handled (e.g. legitimate spaces in arguments).
         */
        //assert!(plist.add_entry("@cwd /t1 /t2").is_err());
        //assert!(plist.add_entry("@src /t1 /t2").is_err());
        //assert!(plist.add_entry("@mode 0644 test").is_err());
        //assert!(plist.add_entry("@option nuke pkgdb").is_err());
        //assert!(plist.add_entry("@owner zak and sara").is_err());
        //assert!(plist.add_entry("@group ben folds five").is_err());
        //assert!(plist.add_entry("@name package name").is_err());
        //assert!(plist.add_entry("@pkgdir /t1 /t2").is_err());
        //assert!(plist.add_entry("@dirrm /t1 /t2").is_err());
        //assert!(plist.add_entry("@display file1 file2").is_err());
        //assert!(plist.add_entry("@pkgdep pkg1-[0-9]* pkg2-[0-9]*").is_err());
        //assert!(plist.add_entry("@blddep pkg1-[0-9]* pkg2-[0-9]*").is_err());
        //assert!(plist.add_entry("@pkgcfl pkg1-[0-9]* pkg2-[0-9]*").is_err());

        /*
         * Commands that take at least one argument.
         */
        assert!(plist.add_entry("@exec").is_err());
        assert!(plist.add_entry("@exec echo").is_ok());
        assert!(plist.add_entry("@exec echo test").is_ok());
        assert!(plist.add_entry("@unexec").is_err());
        assert!(plist.add_entry("@unexec echo").is_ok());
        assert!(plist.add_entry("@unexec echo test").is_ok());
        assert!(plist.add_entry("@comment").is_err());
        assert!(plist.add_entry("@comment hi").is_ok());
        assert!(plist.add_entry("@comment hello there").is_ok());
    }
}
