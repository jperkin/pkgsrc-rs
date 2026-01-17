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
 *
 * An example pkg_add(1) utility
 *
 * This example demonstrates the major features of the native pkg_install
 * pkg_add command, showcasing the pkgsrc-rs library's capabilities for
 * package installation and management.
 */

use anyhow::{Context, Result, bail};
use pkgsrc::archive::{BinaryPackage, ExtractOptions};
use pkgsrc::pkgdb::PkgDB;
use pkgsrc::{Depend, Pattern};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "pkg_add", about = "An example pkg_add(1) command")]
pub struct OptArgs {
    /// Set PKG_DBDIR for installed packages
    #[structopt(short = "K", long = "pkg-dbdir")]
    pkg_dbdir: Option<String>,

    /// Mark package as automatically installed (dependency)
    #[structopt(short = "A", long = "automatic")]
    automatic: bool,

    /// Force installation despite failures (OS/arch mismatch, conflicts)
    #[structopt(short = "f", long = "force")]
    force: bool,

    /// Disable execution of install scripts
    #[structopt(short = "I", long = "no-scripts")]
    no_scripts: bool,

    /// Dry-run: show what would be done without actually installing
    #[structopt(short = "n", long = "dry-run")]
    dry_run: bool,

    /// Override installation prefix
    #[structopt(short = "p", long = "prefix")]
    prefix: Option<String>,

    /// Prefix all paths with destdir
    #[structopt(short = "P", long = "destdir")]
    destdir: Option<String>,

    /// Update existing packages (replace with newer version)
    #[structopt(short = "U", long = "update")]
    update: bool,

    /// Update and recursively update dependent packages
    #[structopt(short = "u", long = "recursive-update")]
    recursive_update: bool,

    /// Verbose output
    #[structopt(short = "v", long = "verbose")]
    verbose: bool,

    /// Package files to install (.tgz, .tzst, etc.)
    #[structopt(parse(from_os_str))]
    packages: Vec<PathBuf>,
}

/// Information about a package pending installation
#[derive(Debug)]
struct PackageInfo {
    /// Path to the package file
    #[allow(dead_code)]
    path: PathBuf,
    /// Parsed package
    package: BinaryPackage,
    /// Package name with version
    pkgname: String,
    /// Is this an automatic (dependency) installation?
    automatic: bool,
}

/// The package installation context
struct InstallContext {
    args: OptArgs,
    pkg_dbdir: PathBuf,
    install_prefix: PathBuf,
    destdir: Option<PathBuf>,
    installed: HashSet<String>,
    pending: Vec<PackageInfo>,
}

impl InstallContext {
    fn new(args: OptArgs) -> Result<Self> {
        let pkg_dbdir = PathBuf::from(
            args.pkg_dbdir
                .clone()
                .unwrap_or_else(|| "/var/db/pkg".to_string()),
        );

        let install_prefix = args
            .prefix
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/pkg"));

        let destdir = args.destdir.as_ref().map(PathBuf::from);

        // Load currently installed packages
        let mut installed = HashSet::new();
        if pkg_dbdir.exists() {
            if let Ok(pkgdb) = PkgDB::open(&pkg_dbdir) {
                for pkg_result in pkgdb {
                    if let Ok(pkg) = pkg_result {
                        installed.insert(pkg.pkgname().to_string());
                    }
                }
            }
        }

        Ok(Self {
            args,
            pkg_dbdir,
            install_prefix,
            destdir,
            installed,
            pending: Vec::new(),
        })
    }

    fn verbose(&self, msg: impl AsRef<str>) {
        if self.args.verbose {
            println!("{}", msg.as_ref());
        }
    }

    fn info(&self, msg: impl AsRef<str>) {
        println!("{}", msg.as_ref());
    }

    /// Check if a package matching the pattern is already installed
    fn is_satisfied(&self, pattern: &Pattern) -> Option<String> {
        for pkgname in &self.installed {
            if pattern.matches(pkgname) {
                return Some(pkgname.clone());
            }
        }
        None
    }

    /// Add a package to the pending installation queue
    fn add_package(&mut self, path: PathBuf, automatic: bool) -> Result<()> {
        let package = BinaryPackage::open(&path)
            .with_context(|| format!("Failed to open package: {}", path.display()))?;

        let pkgname = package
            .pkgname()
            .ok_or_else(|| anyhow::anyhow!("Package has no name"))?
            .to_string();

        self.verbose(format!("Adding {} to installation queue", pkgname));

        self.pending.push(PackageInfo {
            path,
            package,
            pkgname,
            automatic,
        });

        Ok(())
    }

    /// Check package dependencies
    fn check_dependencies(&mut self, pkg_info: &PackageInfo) -> Result<Vec<String>> {
        let mut missing = Vec::new();

        for dep_str in pkg_info.package.plist().depends() {
            let depend = Depend::new(dep_str)
                .with_context(|| format!("Invalid dependency: {}", dep_str))?;

            if let Some(satisfied) = self.is_satisfied(depend.pattern()) {
                self.verbose(format!(
                    "  Dependency {} satisfied by {}",
                    dep_str, satisfied
                ));
            } else {
                self.verbose(format!("  Missing dependency: {}", dep_str));
                missing.push(dep_str.to_string());
            }
        }

        Ok(missing)
    }

    /// Check package conflicts
    fn check_conflicts(&self, pkg_info: &PackageInfo) -> Result<Vec<String>> {
        let mut conflicts = Vec::new();

        for cfl_str in pkg_info.package.plist().conflicts() {
            let pattern = Pattern::new(cfl_str)
                .with_context(|| format!("Invalid conflict pattern: {}", cfl_str))?;

            if let Some(conflicting) = self.is_satisfied(&pattern) {
                conflicts.push(conflicting);
            }
        }

        Ok(conflicts)
    }

    /// Extract package files to the destination
    fn extract_package(&self, pkg_info: &PackageInfo) -> Result<()> {
        let dest = if let Some(destdir) = &self.destdir {
            destdir.join(self.install_prefix.strip_prefix("/").unwrap_or(&self.install_prefix))
        } else {
            self.install_prefix.clone()
        };

        if self.args.dry_run {
            self.info(format!(
                "Would extract {} to {}",
                pkg_info.pkgname,
                dest.display()
            ));
            return Ok(());
        }

        self.verbose(format!("Extracting {} to {}", pkg_info.pkgname, dest.display()));

        // Create destination directory if it doesn't exist
        fs::create_dir_all(&dest)
            .with_context(|| format!("Failed to create directory: {}", dest.display()))?;

        // Extract with plist-based permissions
        let options = ExtractOptions::new().with_mode();
        let extracted = pkg_info
            .package
            .extract_with_plist(&dest, options)
            .with_context(|| format!("Failed to extract package: {}", pkg_info.pkgname))?;

        self.verbose(format!("Extracted {} files", extracted.len()));

        Ok(())
    }

    /// Register package in the package database
    fn register_package(&mut self, pkg_info: &PackageInfo) -> Result<()> {
        if self.args.dry_run {
            self.info(format!("Would register {} in {}", pkg_info.pkgname, self.pkg_dbdir.display()));
            self.installed.insert(pkg_info.pkgname.clone());
            return Ok(());
        }

        let pkg_dir = self.pkg_dbdir.join(&pkg_info.pkgname);

        self.verbose(format!(
            "Registering {} in {}",
            pkg_info.pkgname,
            pkg_dir.display()
        ));

        // Create package directory
        fs::create_dir_all(&pkg_dir)
            .with_context(|| format!("Failed to create package directory: {}", pkg_dir.display()))?;

        // Write required metadata files
        fs::write(
            pkg_dir.join("+CONTENTS"),
            pkg_info.package.metadata().contents(),
        )?;
        fs::write(
            pkg_dir.join("+COMMENT"),
            pkg_info.package.metadata().comment(),
        )?;
        fs::write(pkg_dir.join("+DESC"), pkg_info.package.metadata().desc())?;

        // Write optional metadata files
        if let Some(build_info) = pkg_info.package.metadata().build_info() {
            fs::write(pkg_dir.join("+BUILD_INFO"), build_info.join("\n"))?;
        }

        if let Some(build_version) = pkg_info.package.metadata().build_version() {
            fs::write(pkg_dir.join("+BUILD_VERSION"), build_version.join("\n"))?;
        }

        if let Some(install) = pkg_info.package.metadata().install() {
            fs::write(pkg_dir.join("+INSTALL"), install)?;
        }

        if let Some(deinstall) = pkg_info.package.metadata().deinstall() {
            fs::write(pkg_dir.join("+DEINSTALL"), deinstall)?;
        }

        if let Some(display) = pkg_info.package.metadata().display() {
            fs::write(pkg_dir.join("+DISPLAY"), display)?;
        }

        // Write size information
        if let Some(size_pkg) = pkg_info.package.metadata().size_pkg() {
            fs::write(pkg_dir.join("+SIZE_PKG"), size_pkg.to_string())?;
        }

        if let Some(size_all) = pkg_info.package.metadata().size_all() {
            fs::write(pkg_dir.join("+SIZE_ALL"), size_all.to_string())?;
        }

        // Mark as automatic if requested
        if pkg_info.automatic {
            fs::write(pkg_dir.join("+AUTOMATIC"), "")?;
        }

        self.installed.insert(pkg_info.pkgname.clone());
        self.verbose(format!("Registered {}", pkg_info.pkgname));

        Ok(())
    }

    /// Execute install script if present and not disabled
    fn run_install_script(&self, pkg_info: &PackageInfo, phase: &str) -> Result<()> {
        if self.args.no_scripts {
            self.verbose(format!("Skipping install script for {}", pkg_info.pkgname));
            return Ok(());
        }

        if let Some(script) = pkg_info.package.metadata().install() {
            if self.args.dry_run {
                self.info(format!(
                    "Would execute install script for {} ({})",
                    pkg_info.pkgname, phase
                ));
                return Ok(());
            }

            self.verbose(format!(
                "Executing install script for {} ({})",
                pkg_info.pkgname, phase
            ));

            // In a real implementation, we would:
            // 1. Write script to a temporary file
            // 2. Make it executable
            // 3. Execute it with appropriate environment variables
            // 4. Handle the exit status
            //
            // For this example, we just note that it would be executed
            self.verbose(format!("Install script has {} bytes", script.len()));
        }

        Ok(())
    }

    /// Display package message if present
    fn show_display_file(&self, pkg_info: &PackageInfo) -> Result<()> {
        if let Some(display) = pkg_info.package.metadata().display() {
            println!("\n{}", "=".repeat(70));
            println!("{}", display);
            println!("{}", "=".repeat(70));
        }
        Ok(())
    }

    /// Install a single package
    fn install_package(&mut self, pkg_info: &PackageInfo) -> Result<()> {
        self.info(format!("Installing {}...", pkg_info.pkgname));

        // Check if already installed (unless updating)
        if !self.args.update && !self.args.recursive_update {
            if self.installed.contains(&pkg_info.pkgname) {
                self.info(format!("{} is already installed", pkg_info.pkgname));
                return Ok(());
            }
        }

        // Check OS/architecture compatibility
        if !self.args.force {
            if let Some(opsys) = pkg_info.package.build_info_value("OPSYS") {
                self.verbose(format!("Package built for: {}", opsys));
                // In a real implementation, we would check against current OS
            }

            if let Some(machine_arch) = pkg_info.package.build_info_value("MACHINE_ARCH") {
                self.verbose(format!("Package architecture: {}", machine_arch));
                // In a real implementation, we would check against current architecture
            }
        }

        // Check dependencies
        let missing_deps = self.check_dependencies(pkg_info)?;
        if !missing_deps.is_empty() && !self.args.force {
            bail!(
                "Missing dependencies for {}: {}",
                pkg_info.pkgname,
                missing_deps.join(", ")
            );
        }

        // Check conflicts
        let conflicts = self.check_conflicts(pkg_info)?;
        if !conflicts.is_empty() && !self.args.force {
            bail!(
                "Package {} conflicts with: {}",
                pkg_info.pkgname,
                conflicts.join(", ")
            );
        }

        // Run PRE-INSTALL script
        self.run_install_script(pkg_info, "PRE-INSTALL")?;

        // Extract package files
        self.extract_package(pkg_info)?;

        // Run POST-INSTALL script
        self.run_install_script(pkg_info, "POST-INSTALL")?;

        // Register in package database
        self.register_package(pkg_info)?;

        // Show display file if present
        self.show_display_file(pkg_info)?;

        self.info(format!("Successfully installed {}", pkg_info.pkgname));

        Ok(())
    }

    /// Process all packages in the installation queue
    fn install_all(&mut self) -> Result<()> {
        // Take ownership of pending packages
        let packages: Vec<_> = self.pending.drain(..).collect();

        if packages.is_empty() {
            bail!("No packages to install");
        }

        for pkg_info in packages {
            self.install_package(&pkg_info)?;
        }

        Ok(())
    }
}

fn main() -> Result<()> {
    let args = OptArgs::from_args();

    if args.packages.is_empty() {
        bail!("No packages specified");
    }

    let mut ctx = InstallContext::new(args)?;

    // Add all specified packages to the installation queue
    for path in ctx.args.packages.clone() {
        if !path.exists() {
            bail!("Package not found: {}", path.display());
        }

        ctx.add_package(path, ctx.args.automatic)?;
    }

    // Install all packages
    ctx.install_all()?;

    Ok(())
}
