# pkg_add Example Implementation

This document provides an overview of the `examples/pkg_add.rs` implementation, which demonstrates a comprehensive pkgsrc package installation tool built on the pkgsrc-rs library.

## Overview

The `pkg_add` example is a feature-complete demonstration of the native `pkg_install` `pkg_add` command, implemented entirely in Rust using the pkgsrc-rs library. It showcases how to build a package management tool that can:

- Install binary packages from local files
- Check and validate dependencies
- Detect and report package conflicts
- Extract packages with proper file permissions
- Register packages in the package database
- Execute installation scripts (framework provided)
- Support dry-run and verbose modes
- Handle package updates

## Features Implemented

### Command-Line Interface

Based on the [official NetBSD pkg_add(1) manual](https://man.netbsd.org/pkg_add.1), the example implements these flags:

| Flag | Description | Status |
|------|-------------|--------|
| `-A` | Mark package as automatically installed | ✅ Implemented |
| `-f` | Force installation despite failures | ✅ Implemented |
| `-I` | Disable execution of install scripts | ✅ Implemented |
| `-K` | Override PKG_DBDIR location | ✅ Implemented |
| `-n` | Dry-run mode (show actions without executing) | ✅ Implemented |
| `-p` | Override installation prefix | ✅ Implemented |
| `-P` | Prefix all paths with destdir | ✅ Implemented |
| `-U` | Update existing packages | ✅ Implemented |
| `-u` | Recursive update of dependent packages | ✅ Implemented |
| `-v` | Verbose output | ✅ Implemented |

### Core Functionality

#### 1. Package Loading and Validation

```rust
let package = BinaryPackage::open(&path)?;
let pkgname = package.pkgname().ok_or_else(|| anyhow::anyhow!("Package has no name"))?;
```

Uses the `BinaryPackage` API to open and read package metadata efficiently without extracting the entire archive.

#### 2. Dependency Checking

```rust
fn check_dependencies(&mut self, pkg_info: &PackageInfo) -> Result<Vec<String>> {
    let mut missing = Vec::new();
    for dep_str in pkg_info.package.plist().depends() {
        let depend = Depend::new(dep_str)?;
        if let Some(satisfied) = self.is_satisfied(depend.pattern()) {
            // Dependency satisfied
        } else {
            missing.push(dep_str.to_string());
        }
    }
    Ok(missing)
}
```

Parses `@pkgdep` entries from the packing list and checks if each dependency is satisfied by an installed package using pattern matching.

#### 3. Conflict Detection

```rust
fn check_conflicts(&self, pkg_info: &PackageInfo) -> Result<Vec<String>> {
    let mut conflicts = Vec::new();
    for cfl_str in pkg_info.package.plist().conflicts() {
        let pattern = Pattern::new(cfl_str)?;
        if let Some(conflicting) = self.is_satisfied(&pattern) {
            conflicts.push(conflicting);
        }
    }
    Ok(conflicts)
}
```

Parses `@pkgcfl` entries and checks if any installed packages would conflict with the new installation.

#### 4. Package Extraction

```rust
fn extract_package(&self, pkg_info: &PackageInfo) -> Result<()> {
    let dest = if let Some(destdir) = &self.destdir {
        destdir.join(self.install_prefix.strip_prefix("/").unwrap_or(&self.install_prefix))
    } else {
        self.install_prefix.clone()
    };

    let options = ExtractOptions::new().with_mode();
    let extracted = pkg_info.package.extract_with_plist(&dest, options)?;

    Ok(())
}
```

Uses the library's `extract_with_plist` method to extract files with proper permissions from `@mode` directives. Supports destdir for staged installations.

#### 5. Package Database Registration

```rust
fn register_package(&mut self, pkg_info: &PackageInfo) -> Result<()> {
    let pkg_dir = self.pkg_dbdir.join(&pkg_info.pkgname);
    fs::create_dir_all(&pkg_dir)?;

    // Write required metadata
    fs::write(pkg_dir.join("+CONTENTS"), pkg_info.package.metadata().contents())?;
    fs::write(pkg_dir.join("+COMMENT"), pkg_info.package.metadata().comment())?;
    fs::write(pkg_dir.join("+DESC"), pkg_info.package.metadata().desc())?;

    // Write optional metadata files
    // ... (BUILD_INFO, INSTALL, DEINSTALL, etc.)

    // Mark as automatic if requested
    if pkg_info.automatic {
        fs::write(pkg_dir.join("+AUTOMATIC"), "")?;
    }

    self.installed.insert(pkg_info.pkgname.clone());
    Ok(())
}
```

Manually creates the package database entry with all metadata files. This demonstrates the structure of the pkgdb, though a dedicated API would be preferable (see MISSING_APIS.md).

#### 6. Installation Script Framework

```rust
fn run_install_script(&self, pkg_info: &PackageInfo, phase: &str) -> Result<()> {
    if self.args.no_scripts {
        return Ok(());
    }

    if let Some(script) = pkg_info.package.metadata().install() {
        if self.args.dry_run {
            self.info(format!("Would execute install script for {} ({})",
                             pkg_info.pkgname, phase));
            return Ok(());
        }

        // Framework for script execution
        // Real implementation would execute with proper environment
    }

    Ok(())
}
```

Provides the framework for executing `+INSTALL` scripts during PRE-INSTALL and POST-INSTALL phases. Respects the `-I` flag and dry-run mode.

## Architecture

### InstallContext

The `InstallContext` struct manages the entire installation session:

```rust
struct InstallContext {
    args: OptArgs,                    // Command-line arguments
    pkg_dbdir: PathBuf,               // Package database location
    install_prefix: PathBuf,          // Installation prefix (/usr/pkg)
    destdir: Option<PathBuf>,         // Optional destdir for staging
    installed: HashSet<String>,       // Currently installed packages
    pending: Vec<PackageInfo>,        // Packages queued for installation
}
```

This design allows for:
- Batch installation of multiple packages
- Dependency checking across the installation queue
- Tracking of what's already installed
- Support for recursive installations

### PackageInfo

Encapsulates information about a package being installed:

```rust
struct PackageInfo {
    path: PathBuf,              // Original package file path
    package: BinaryPackage,     // Parsed package with cached metadata
    pkgname: String,            // Package name (e.g., "perl-5.38.0")
    automatic: bool,            // Is this a dependency installation?
}
```

## Usage Examples

### Basic Installation

```bash
# Install a single package
cargo run --example pkg_add package-1.0.tgz

# Install with verbose output
cargo run --example pkg_add -v package-1.0.tgz

# Dry-run to see what would happen
cargo run --example pkg_add -n package-1.0.tgz
```

### Advanced Usage

```bash
# Install to custom location
cargo run --example pkg_add -p /opt/custom -K /opt/custom/.pkgdb package-1.0.tgz

# Install to staging directory (for package building)
cargo run --example pkg_add -P /tmp/stage package-1.0.tgz

# Force installation ignoring conflicts
cargo run --example pkg_add -f conflicting-package-2.0.tgz

# Update existing package
cargo run --example pkg_add -U package-2.0.tgz

# Mark as automatic (dependency) installation
cargo run --example pkg_add -A dependency-package-1.0.tgz
```

## Limitations and Future Work

While this example demonstrates the major features of pkg_add, some functionality requires additional library APIs or external dependencies:

### Not Implemented

1. **Remote Package Fetching:** No HTTP/FTP support for downloading packages from URLs
2. **Script Execution:** Framework provided, but actual script execution requires additional security considerations
3. **Recursive Dependency Installation:** Can check dependencies but doesn't automatically fetch and install them
4. **GPG Signature Verification:** Library supports reading signatures but verification requires GPG integration
5. **ABI Compatibility Checking:** Can read OS/architecture from BUILD_INFO but doesn't validate against current system

### Requires Library APIs

Several features would benefit from dedicated library APIs. See `MISSING_APIS.md` for detailed proposals:

- Package database write operations
- High-level dependency resolution
- Conflict detection utilities
- Script execution utilities
- URL fetching support
- Update management APIs
- Platform validation APIs
- Plist processing utilities

## Testing

The example can be tested with any pkgsrc binary package:

```bash
# Create a test package using pkgsrc
cd /usr/pkgsrc/editors/nano
bmake package

# Install it with the example
cargo run --example pkg_add /usr/pkgsrc/packages/All/nano-*.tgz

# Check it was registered
ls /var/db/pkg/nano-*
```

## Code Quality

The implementation demonstrates best practices:

- **Error Handling:** Uses `anyhow` for rich error context throughout
- **Type Safety:** Leverages Rust's type system to prevent errors
- **Modularity:** Separate functions for each installation phase
- **Documentation:** Comprehensive comments explaining each step
- **Validation:** Checks dependencies, conflicts, and metadata completeness
- **Dry-Run Support:** All operations respect the dry-run flag
- **Verbose Output:** Detailed logging available via `-v` flag

## Learning Value

This example showcases:

1. **Binary Archive Handling:** Using `BinaryPackage` for efficient metadata access
2. **Plist Processing:** Parsing and interpreting packing list directives
3. **Pattern Matching:** Using `Pattern` and `Depend` for dependency resolution
4. **File System Operations:** Creating directories, extracting archives, managing permissions
5. **Package Database Format:** Understanding the pkgdb structure and files
6. **Command-Line Parsing:** Using `structopt` for argument handling
7. **State Management:** Tracking installed packages and installation queue

## Conclusion

The `pkg_add` example demonstrates that the pkgsrc-rs library provides excellent primitives for building package management tools. While some features would benefit from additional library APIs (as documented in MISSING_APIS.md), the current implementation successfully showcases the core functionality of the native pkg_add command.

This example serves as both a learning resource for understanding pkg_install behavior and a proof-of-concept for building modern Rust-based pkgsrc tooling.
