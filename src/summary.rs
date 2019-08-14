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
 * summary.rs - handle pkg_summary(5) parsing.
 */

#[cfg(test)]
use unindent::unindent;

use std::io::Write;

#[derive(Debug)]
pub struct SummaryStream {
    buf: Vec<u8>,
    entries: Vec<SummaryEntry>,
}

/*
 * i64 types are used even though the values cannot be expressed as negative
 * as it avoids having to convert to sqlite which does not support u64.
 */
#[derive(Debug, Default)]
pub struct SummaryEntry {
    automatic: Option<i64>, // Not part of pkg_summary(5)
    build_date: String,
    categories: Vec<String>,
    comment: String,
    conflicts: Vec<String>,
    depends: Vec<String>,
    description: Vec<String>,
    file_cksum: Option<String>,
    file_name: Option<String>,
    file_size: Option<i64>,
    homepage: Option<String>,
    license: Option<String>,
    machine_arch: String,
    opsys: String,
    os_version: String,
    pkg_options: Option<String>,
    pkgbase: String, // Non-standard, name part of pkgname
    pkgname: String, // Full package name including version
    pkgpath: String,
    pkgtools_version: String,
    pkgversion: String, // Non-standard, version part of pkgname
    prev_pkgpath: Option<String>,
    provides: Vec<String>,
    requires: Vec<String>,
    size_pkg: Option<i64>,
    supersedes: Vec<String>,
}

impl SummaryEntry {
    pub fn new() -> SummaryEntry {
        let sum: SummaryEntry = Default::default();
        sum
    }
    /*
     * XXX: Some are Strings, some are str due to unwrapping Option, I need to
     * figure out what's best here depending on how they will be used.
     */
    pub fn automatic(&self) -> i64 {
        self.automatic.unwrap_or(0)
    }
    pub fn build_date(&self) -> &String {
        &self.build_date
    }
    pub fn categories(&self) -> &Vec<String> {
        &self.categories
    }
    pub fn comment(&self) -> &String {
        &self.comment
    }
    #[allow(dead_code)]
    pub fn conflicts(&self) -> &Vec<String> {
        &self.conflicts
    }
    #[allow(dead_code)]
    pub fn depends(&self) -> &Vec<String> {
        &self.depends
    }
    pub fn description(&self) -> &Vec<String> {
        &self.description
    }
    #[allow(dead_code)]
    pub fn file_cksum(&self) -> &str {
        match &self.file_cksum {
            Some(s) => s.as_str(),
            None => "",
        }
    }
    pub fn file_name(&self) -> &str {
        match &self.file_name {
            Some(s) => s.as_str(),
            None => "",
        }
    }
    pub fn file_size(&self) -> i64 {
        self.file_size.unwrap_or(0)
    }
    pub fn homepage(&self) -> &str {
        match &self.homepage {
            Some(s) => s.as_str(),
            None => "",
        }
    }
    pub fn license(&self) -> &str {
        match &self.license {
            Some(s) => s.as_str(),
            None => "",
        }
    }
    #[allow(dead_code)]
    pub fn machine_arch(&self) -> &String {
        &self.machine_arch
    }
    pub fn opsys(&self) -> &String {
        &self.opsys
    }
    pub fn os_version(&self) -> &String {
        &self.os_version
    }
    pub fn pkg_options(&self) -> &str {
        match &self.pkg_options {
            Some(s) => s.as_str(),
            None => "",
        }
    }
    pub fn pkgbase(&self) -> &String {
        &self.pkgbase
    }
    pub fn pkgname(&self) -> &String {
        &self.pkgname
    }
    pub fn pkgpath(&self) -> &String {
        &self.pkgpath
    }
    pub fn pkgtools_version(&self) -> &String {
        &self.pkgtools_version
    }
    pub fn pkgversion(&self) -> &String {
        &self.pkgversion
    }
    #[allow(dead_code)]
    pub fn prev_pkgpath(&self) -> &str {
        match &self.prev_pkgpath {
            Some(s) => s.as_str(),
            None => "",
        }
    }
    #[allow(dead_code)]
    pub fn provides(&self) -> &Vec<String> {
        &self.provides
    }
    #[allow(dead_code)]
    pub fn requires(&self) -> &Vec<String> {
        &self.requires
    }
    pub fn size_pkg(&self) -> &Option<i64> {
        &self.size_pkg
    }
    #[allow(dead_code)]
    pub fn supersedes(&self) -> &Vec<String> {
        &self.supersedes
    }

    pub fn parse_entry(&mut self, key: &str, value: &str) -> Result<(), &'static str> {
        let valstring = value.to_string();
        let vali64 = value.parse::<i64>();
        /*
         * Ideally there'd be a way to automatically parse these based on
         * the struct member names and their types, but so far I haven't
         * found a way to do that.
         */
        match key {
            "BUILD_DATE" => self.build_date = valstring,
            "CATEGORIES" => self.categories.push(valstring),
            "COMMENT" => self.comment = valstring,
            "CONFLICTS" => self.conflicts.push(valstring),
            "DEPENDS" => self.depends.push(valstring),
            "DESCRIPTION" => self.description.push(valstring),
            "FILE_CKSUM" => self.file_cksum = Some(valstring),
            "FILE_NAME" => self.file_name = Some(valstring),
            "FILE_SIZE" => self.file_size = Some(vali64.unwrap()),
            "HOMEPAGE" => self.homepage = Some(valstring),
            "LICENSE" => self.license = Some(valstring),
            "MACHINE_ARCH" => self.machine_arch = valstring,
            "OPSYS" => self.opsys = valstring,
            "OS_VERSION" => self.os_version = valstring,
            "PKG_OPTIONS" => self.pkg_options = Some(valstring),
            /* Split PKGNAME into constituent parts */
            "PKGNAME" => {
                let splitstring = valstring.clone();
                self.pkgname = valstring;
                let v: Vec<&str> = splitstring.rsplitn(2, '-').collect();
                self.pkgbase = v[1].to_string();
                self.pkgversion = v[0].to_string();
            }
            "PKGPATH" => self.pkgpath = valstring,
            "PKGTOOLS_VERSION" => self.pkgtools_version = valstring,
            "PREV_PKGPATH" => self.prev_pkgpath = Some(valstring),
            "PROVIDES" => self.provides.push(valstring),
            "REQUIRES" => self.requires.push(valstring),
            "SIZE_PKG" => self.size_pkg = Some(vali64.unwrap()),
            "SUPERSEDES" => self.supersedes.push(valstring),
            _ => return Err("Unhandled key"),
        }
        Ok(())
    }

    /*
     * Not a member of pkg_summary(5) but this is the best place to store this
     * information at present.
     */
    pub fn set_automatic(&mut self) {
        self.automatic = Some(1);
    }

    /*
     * Ensure required fields (as per pkg_summary(5)) are set
     */
    pub fn validate(&self) -> Result<(), &'static str> {
        /*
         * Again, there's probably a fancy way to match these.
         */
        if self.build_date.is_empty() {
            return Err("Missing BUILD_DATE");
        }
        if self.categories.is_empty() {
            return Err("Missing CATEGORIES");
        }
        if self.comment.is_empty() {
            return Err("Missing COMMENT");
        }
        if self.description.is_empty() {
            return Err("Missing DESCRIPTION");
        }
        if self.machine_arch.is_empty() {
            return Err("Missing MACHINE_ARCH");
        }
        if self.opsys.is_empty() {
            return Err("Missing OPSYS");
        }
        if self.os_version.is_empty() {
            return Err("Missing OS_VERSION");
        }
        if self.pkgname.is_empty() {
            return Err("Missing PKGNAME");
        }
        if self.pkgpath.is_empty() {
            return Err("Missing PKGPATH");
        }
        if self.pkgtools_version.is_empty() {
            return Err("Missing PKGTOOLS_VERSION");
        }
        /*
         * SIZE_PKG is a required field but a size of 0 is valid (meta-pkgs)
         * so it needs to be an Option().
         */
        if self.size_pkg.is_none() {
            return Err("Missing SIZE_PKG");
        }
        Ok(())
    }
}

impl SummaryStream {
    pub fn new() -> SummaryStream {
        SummaryStream {
            buf: vec![],
            entries: vec![],
        }
    }

    pub fn entries(&self) -> &Vec<SummaryEntry> {
        &self.entries
    }

    pub fn entries_mut(&mut self) -> &mut Vec<SummaryEntry> {
        &mut self.entries
    }
}

impl Write for SummaryStream {
    /*
     * Stream from our input buffer into SummaryEntry records.
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
            _ => panic!("ERROR: Invalid pkg_summary(5) stream"),
        };

        /*
         * We have at least one complete record, parse it and add to the vector
         * of summary entries.
         */
        for sum_entry in input_string.split_terminator("\n\n") {
            let mut sum = SummaryEntry::new();
            for line in sum_entry.lines() {
                let v: Vec<&str> = line.splitn(2, '=').collect();
                let key = v.get(0);
                let val = v.get(1);
                if key.is_none() || val.is_none() {
                    panic!("ERROR: Invalid pkg_summary(5) line");
                }
                match sum.parse_entry(key.unwrap(), val.unwrap()) {
                    Ok(_) => {}
                    Err(err) => {
                        println!("PARSE ERROR: {}", err);
                        println!("{:#?}", sum);
                    }
                }
            }
            match sum.validate() {
                Ok(_) => {
                    self.entries.push(sum);
                }
                Err(err) => {
                    println!("VALIDATE ERROR: {}", err);
                    println!("{:#?}", sum);
                }
            }
        }

        /*
         * What we really want is some way to just move forward the beginning
         * of the vector, but there appears to be no way to do that, so we end
         * up having to do something with the existing data.  This seems to be
         * the best way to do it for now?
         */
        self.buf = self.buf.split_off(input_string.len());

        Ok(input.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_summary() {
        let mut pkgsummary = SummaryStream::new();
        let pkginfo = unindent(
            r#"
        BUILD_DATE=2019-08-14 00:00:00 +0000
        CATEGORIES=test
        COMMENT=This is a test
        DESCRIPTION=A test description
        DESCRIPTION=This is a multi-line field
        MACHINE_ARCH=x86_64
        OPSYS=Darwin
        OS_VERSION=18.7.0
        PKGNAME=pkgtest-1.0
        PKGPATH=category/pkgtest
        PKGTOOLS_VERSION=20190405
        SIZE_PKG=1234

        "#,
        );
        std::io::copy(&mut pkginfo.as_bytes(), &mut pkgsummary);
        assert_eq!(pkgsummary.entries().len(), 1);

        let mut pkgsum = SummaryEntry::new();
        pkgsum = pkgsummary.entries_mut().pop().expect("invalid");
        assert_eq!(pkgsum.description().len(), 2);
    }
}
