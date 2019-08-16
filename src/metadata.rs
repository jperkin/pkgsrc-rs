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
 * metadata.rs - parse package metadata from "+*" files
 */

/*!
 * Parse metadata contained in "+*" files in a package archive.
 *
 * ## Examples
 *
 * ```no_run
 * use flate2::read::GzDecoder;
 * use pkgsrc::metadata;
 * use std::fs::File;
 * use std::io::Read;
 * use tar::Archive;
 *
 * fn main() -> Result<(), std::io::Error> {
 *     let pkg = File::open("package-1.0.tgz")?;
 *     let mut archive = Archive::new(GzDecoder::new(pkg));
 *     let mut metadata = metadata::MetaData::new();
 *
 *     for file in archive.entries()? {
 *         let mut file = file?;
 *         let filename = String::from(file.header().path()?.to_str().unwrap());
 *         let mut s = String::new();
 *
 *         if filename.starts_with('+') {
 *             file.read_to_string(&mut s)?;
 *             if let Err(e) = metadata.read_metadata(&filename, &s) {
 *                 panic!("Bad metadata: {}", e);
 *             }
 *         }
 *     }
 *
 *     if let Err(e) = metadata.is_valid() {
 *         panic!("Bad metadata: {}", e);
 *     }
 *
 *     println!("Information for package-1.0");
 *     println!("Comment: {}", metadata.comment());
 *     println!("Files:");
 *     for file in metadata.contents() {
 *         if !file.starts_with('@') && !file.starts_with('+') {
 *             println!("{}", file);
 *         }
 *     }
 *
 *     Ok(())
 * }
 * ```
 */

#[derive(Debug, Default)]
pub struct MetaData {
    build_info: Option<Vec<String>>,
    build_version: Option<Vec<String>>,
    comment: String,
    contents: Vec<String>,
    deinstall: Option<Vec<String>>,
    desc: Vec<String>,
    display: Option<Vec<String>>,
    install: Option<Vec<String>>,
    installed_info: Option<Vec<String>>,
    mtree_dirs: Option<Vec<String>>,
    preserve: Option<Vec<String>>,
    required_by: Option<Vec<String>>,
    size_all: Option<i64>,
    size_pkg: Option<i64>,
}

impl MetaData {
    pub fn new() -> MetaData {
        let metadata: MetaData = Default::default();
        metadata
    }

    pub fn build_info(&self) -> &Option<Vec<String>> {
        &self.build_info
    }

    pub fn build_version(&self) -> &Option<Vec<String>> {
        &self.build_version
    }

    pub fn comment(&self) -> &String {
        &self.comment
    }

    pub fn contents(&self) -> &Vec<String> {
        &self.contents
    }

    pub fn deinstall(&self) -> &Option<Vec<String>> {
        &self.deinstall
    }

    pub fn desc(&self) -> &Vec<String> {
        &self.desc
    }

    pub fn display(&self) -> &Option<Vec<String>> {
        &self.display
    }

    pub fn install(&self) -> &Option<Vec<String>> {
        &self.install
    }

    pub fn installed_info(&self) -> &Option<Vec<String>> {
        &self.installed_info
    }

    pub fn mtree_dirs(&self) -> &Option<Vec<String>> {
        &self.mtree_dirs
    }

    pub fn preserve(&self) -> &Option<Vec<String>> {
        &self.preserve
    }

    pub fn required_by(&self) -> &Option<Vec<String>> {
        &self.required_by
    }

    pub fn size_all(&self) -> &Option<i64> {
        &self.size_all
    }

    pub fn size_pkg(&self) -> &Option<i64> {
        &self.size_pkg
    }

    pub fn read_metadata(
        &mut self,
        fname: &str,
        value: &str,
    ) -> Result<(), &'static str> {
        /*
         * Set up various variable types that may be used.
         *
         * XXX: I'm not 100% sure .trim() is correct here, it might need to be
         * modified to only strip newlines rather than all whitespace.
         */
        let val_string = value.trim().to_string();
        let val_i64 = val_string.parse::<i64>();
        let mut val_vec = vec![];
        for line in val_string.lines() {
            val_vec.push(line.to_string());
        }

        match fname {
            "+BUILD_INFO" => self.build_info = Some(val_vec),
            "+BUILD_VERSION" => self.build_version = Some(val_vec),
            "+COMMENT" => self.comment = val_string,
            "+CONTENTS" => self.contents = val_vec,
            "+DEINSTALL" => self.deinstall = Some(val_vec),
            "+DESC" => self.desc = val_vec,
            "+DISPLAY" => self.display = Some(val_vec),
            "+INSTALL" => self.install = Some(val_vec),
            "+INSTALLED_INFO" => self.installed_info = Some(val_vec),
            "+MTREE_DIRS" => self.mtree_dirs = Some(val_vec),
            "+PRESERVE" => self.preserve = Some(val_vec),
            "+REQUIRED_BY" => self.required_by = Some(val_vec),
            "+SIZE_ALL" => self.size_all = Some(val_i64.unwrap()),
            "+SIZE_PKG" => self.size_pkg = Some(val_i64.unwrap()),
            _ => return Err("Invalid metadata filename"),
        }

        Ok(())
    }

    /*
     * Ensure required files have been registered.
     */
    pub fn is_valid(&self) -> Result<(), &'static str> {
        if self.comment.is_empty() {
            return Err("Missing or empty +COMMENT");
        }
        if self.contents.is_empty() {
            return Err("Missing or empty +CONTENTS");
        }
        if self.desc.is_empty() {
            return Err("Missing or empty +DESC");
        }
        Ok(())
    }
}
