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

/**
 * Parse metadata contained in `+*` files in a package archive.
 *
 * ## Examples
 *
 * ```no_run
 * use flate2::read::GzDecoder;
 * use pkgsrc::Metadata;
 * use std::fs::File;
 * use std::io::Read;
 * use tar::Archive;
 *
 * fn main() -> Result<(), std::io::Error> {
 *     let pkg = File::open("package-1.0.tgz")?;
 *     let mut archive = Archive::new(GzDecoder::new(pkg));
 *     let mut metadata = Metadata::new();
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
 *     for line in metadata.contents().lines() {
 *         if !line.starts_with('@') && !line.starts_with('+') {
 *             println!("{}", line);
 *         }
 *     }
 *
 *     Ok(())
 * }
 * ```
 */
#[derive(Debug, Default)]
pub struct Metadata {
    build_info: Option<Vec<String>>,
    build_version: Option<Vec<String>>,
    comment: String,
    contents: String,
    deinstall: Option<String>,
    desc: String,
    display: Option<String>,
    install: Option<String>,
    installed_info: Option<Vec<String>>,
    mtree_dirs: Option<Vec<String>>,
    preserve: Option<Vec<String>>,
    required_by: Option<Vec<String>>,
    size_all: Option<i64>,
    size_pkg: Option<i64>,
}

impl Metadata {
    /**
     * Return a new empty `Metadata` container.
     */
    pub fn new() -> Metadata {
        let metadata: Metadata = Default::default();
        metadata
    }

    /**
     * Return the optional `+BUILD_INFO` file as a vector of strings.
     */
    pub fn build_info(&self) -> &Option<Vec<String>> {
        &self.build_info
    }

    /**
     * Return the optional `+BUILD_VERSION` file as a vector of strings.
     */
    pub fn build_version(&self) -> &Option<Vec<String>> {
        &self.build_version
    }

    /**
     * Return the mandatory `+COMMENT` file as a string.  This should be a
     * single line.
     */
    pub fn comment(&self) -> &String {
        &self.comment
    }

    /**
     * Return the mandatory `+CONTENTS` (i.e. packlist or PLIST) file as a
     * complete string.
     */
    pub fn contents(&self) -> &String {
        &self.contents
    }

    /**
     * Return the optional `+DEINSTALL` script as complete string.
     */
    pub fn deinstall(&self) -> &Option<String> {
        &self.deinstall
    }

    /**
     * Return the mandatory `+DESC` file as a complete string.
     */
    pub fn desc(&self) -> &String {
        &self.desc
    }

    /**
     * Return the optional `+DISPLAY` (i.e. MESSAGE) file as a complete string.
     */
    pub fn display(&self) -> &Option<String> {
        &self.display
    }

    /**
     * Return the optional `+INSTALL` script as a complete string.
     */
    pub fn install(&self) -> &Option<String> {
        &self.install
    }

    /**
     * Return the optional `+INSTALLED_INFO` file as a vector of strings.
     */
    pub fn installed_info(&self) -> &Option<Vec<String>> {
        &self.installed_info
    }

    /**
     * Return the optional `+MTREE_DIRS` file (obsolete) as a vector of strings.
     */
    pub fn mtree_dirs(&self) -> &Option<Vec<String>> {
        &self.mtree_dirs
    }

    /**
     * Return the optional `+PRESERVE` file as a vector of strings.
     */
    pub fn preserve(&self) -> &Option<Vec<String>> {
        &self.preserve
    }

    /**
     * Return the optional `+REQUIRED_BY` file as a vector of strings.
     */
    pub fn required_by(&self) -> &Option<Vec<String>> {
        &self.required_by
    }

    /**
     * Return the optional `+SIZE_ALL` file as an i64.
     */
    pub fn size_all(&self) -> &Option<i64> {
        &self.size_all
    }

    /**
     * Return the optional `+SIZE_PKG` file as an i64.
     */
    pub fn size_pkg(&self) -> &Option<i64> {
        &self.size_pkg
    }

    /**
     * Read in a metadata file `fname` and its `value` as strings, populating
     * the associated Metadata struct.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::Metadata;
     *
     * let mut m = Metadata::new();
     * m.read_metadata("+COMMENT", "This is a package comment");
     * ```
     */
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
            "+COMMENT" => self.comment.push_str(&val_string),
            "+CONTENTS" => self.contents.push_str(&val_string),
            "+DEINSTALL" => self.deinstall = Some(val_string),
            "+DESC" => self.desc.push_str(&val_string),
            "+DISPLAY" => self.display = Some(val_string),
            "+INSTALL" => self.install = Some(val_string),
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

    /**
     * Ensure the required files (`+COMMENT`, `+CONTENTS`, and `+DESC`) have
     * been registered, indicating that this is a valid package.
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
