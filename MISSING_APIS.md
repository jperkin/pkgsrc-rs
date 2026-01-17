# Missing Library APIs for Full pkg_add Implementation

This document identifies library interfaces that would enhance the pkgsrc-rs library to support complete implementations of package management tools like `pkg_add`.

## Executive Summary

The `examples/pkg_add.rs` example demonstrates the major features of the native `pkg_install` `pkg_add` command. While the current library provides excellent support for reading packages and metadata, several APIs would significantly improve package installation and management workflows.

## Proposed Library APIs

### 1. Package Database Write Operations

**Current State:** The library provides read-only access to the package database via `PkgDB` and `InstalledPackage`.

**Missing Functionality:**
- Writing new package entries to the database
- Updating existing package entries
- Removing package entries
- Managing `+REQUIRED_BY` relationships between packages

**Proposed API:**

```rust
// In pkgdb module
pub struct PkgDBWriter {
    path: PathBuf,
}

impl PkgDBWriter {
    /// Create a new package database writer
    pub fn open(path: impl AsRef<Path>) -> Result<Self, io::Error>;

    /// Register a new package in the database
    pub fn register_package(
        &mut self,
        pkgname: &str,
        metadata: &Metadata,
        automatic: bool,
    ) -> Result<(), io::Error>;

    /// Update an existing package entry
    pub fn update_package(
        &mut self,
        pkgname: &str,
        metadata: &Metadata,
    ) -> Result<(), io::Error>;

    /// Remove a package entry from the database
    pub fn unregister_package(&mut self, pkgname: &str) -> Result<(), io::Error>;

    /// Add a dependent package to +REQUIRED_BY
    pub fn add_required_by(
        &mut self,
        pkgname: &str,
        required_by: &str,
    ) -> Result<(), io::Error>;

    /// Remove a dependent package from +REQUIRED_BY
    pub fn remove_required_by(
        &mut self,
        pkgname: &str,
        required_by: &str,
    ) -> Result<(), io::Error>;
}
```

**Rationale:** Package installation requires writing metadata to the database. This is currently handled manually in the example using raw file I/O, but a dedicated API would:
- Ensure consistency across tools
- Handle database format evolution
- Provide atomic operations
- Validate metadata completeness

### 2. Dependency Resolution Utilities

**Current State:** The library provides `Depend` for parsing dependencies and `Pattern` for matching, but no high-level resolution logic.

**Missing Functionality:**
- Finding installed packages that satisfy a dependency pattern
- Resolving dependency chains
- Detecting circular dependencies
- Suggesting packages to install for unmet dependencies

**Proposed API:**

```rust
// In depend module or new resolver module
pub struct DependencyResolver<'a> {
    pkgdb: &'a PkgDB,
    available: Vec<Summary>,
}

impl<'a> DependencyResolver<'a> {
    /// Create a new resolver with installed packages and available packages
    pub fn new(pkgdb: &'a PkgDB, available: Vec<Summary>) -> Self;

    /// Check if a dependency is satisfied by any installed package
    pub fn is_satisfied(&self, depend: &Depend) -> Option<String>;

    /// Find all installed packages matching a pattern
    pub fn find_installed(&self, pattern: &Pattern) -> Vec<String>;

    /// Resolve all dependencies for a package
    pub fn resolve_dependencies(
        &self,
        pkg: &BinaryPackage,
    ) -> Result<Vec<Depend>, DependError>;

    /// Detect circular dependencies in a dependency chain
    pub fn detect_cycles(
        &self,
        pkgname: &str,
        chain: &[String],
    ) -> Option<Vec<String>>;

    /// Find available packages that could satisfy a dependency
    pub fn suggest_packages(&self, depend: &Depend) -> Vec<String>;
}
```

**Rationale:** Every package manager needs dependency resolution. The example implements basic checking, but a library API would:
- Provide tested, correct resolution logic
- Support recursive dependency installation
- Enable consistent behavior across tools
- Allow for advanced strategies (newest version, prefer installed, etc.)

### 3. Conflict Detection and Resolution

**Current State:** The library parses `@pkgcfl` from plists but provides no utilities for checking conflicts.

**Missing Functionality:**
- Checking if a package conflicts with installed packages
- Finding all packages that would conflict with a new installation
- Suggesting conflict resolutions

**Proposed API:**

```rust
// In pkgdb or conflict module
pub struct ConflictChecker<'a> {
    pkgdb: &'a PkgDB,
}

impl<'a> ConflictChecker<'a> {
    /// Create a new conflict checker
    pub fn new(pkgdb: &'a PkgDB) -> Self;

    /// Check if a package would conflict with any installed packages
    pub fn check_conflicts(&self, pkg: &BinaryPackage) -> Vec<String>;

    /// Check if installing pkgname would cause conflicts
    pub fn would_conflict(&self, pkgname: &str, conflicts: &[String]) -> Vec<String>;

    /// Find all installed packages that conflict with a pattern
    pub fn find_conflicting(&self, pattern: &Pattern) -> Vec<String>;
}
```

**Rationale:** Conflict detection prevents package corruption and system instability. A library API would:
- Ensure consistent conflict checking logic
- Support force-install options properly
- Enable conflict resolution suggestions
- Allow for conflict policy configuration

### 4. Installation Script Execution

**Current State:** The library reads `+INSTALL` and `+DEINSTALL` scripts from metadata but provides no execution utilities.

**Missing Functionality:**
- Safe execution of install/deinstall scripts
- Environment variable setup for scripts
- Variable substitution (F=%F, D=%D, B=%B, f=%f)
- Script phase management (PRE-INSTALL, POST-INSTALL, etc.)

**Proposed API:**

```rust
// In new scripts module
pub enum ScriptPhase {
    PreInstall,
    PostInstall,
    PreDeinstall,
    PostDeinstall,
}

pub struct ScriptExecutor {
    pkg_dbdir: PathBuf,
    prefix: PathBuf,
}

impl ScriptExecutor {
    /// Create a new script executor
    pub fn new(pkg_dbdir: impl AsRef<Path>, prefix: impl AsRef<Path>) -> Self;

    /// Execute an install/deinstall script
    pub fn execute_script(
        &self,
        pkgname: &str,
        script: &str,
        phase: ScriptPhase,
    ) -> Result<i32, io::Error>;

    /// Perform variable substitution in script content
    pub fn substitute_variables(
        &self,
        script: &str,
        pkgname: &str,
        cwd: &Path,
    ) -> String;
}
```

**Rationale:** Script execution is complex and security-sensitive. A library API would:
- Provide secure execution by default
- Handle variable substitution correctly
- Support all script phases
- Enable dry-run mode properly
- Log script output appropriately

### 5. URL Fetching and Remote Package Support

**Current State:** The library only supports local file paths.

**Missing Functionality:**
- HTTP/HTTPS package fetching
- FTP package fetching (with passive mode support)
- Progress reporting for downloads
- Package signature verification
- Caching of downloaded packages

**Proposed API:**

```rust
// In new fetch module
pub struct PackageFetcher {
    cache_dir: Option<PathBuf>,
    verify_signatures: bool,
}

impl PackageFetcher {
    /// Create a new package fetcher
    pub fn new() -> Self;

    /// Enable caching to a directory
    pub fn with_cache(mut self, cache_dir: impl AsRef<Path>) -> Self;

    /// Enable GPG signature verification
    pub fn with_signature_verification(mut self) -> Self;

    /// Fetch a package from a URL to a local file
    pub fn fetch(
        &self,
        url: &str,
        progress: Option<Box<dyn Fn(u64, u64)>>,
    ) -> Result<PathBuf, FetchError>;

    /// Verify a package signature
    pub fn verify_signature(&self, pkg_path: &Path) -> Result<bool, FetchError>;
}
```

**Rationale:** The native `pkg_add` supports URLs for remote package installation. A library API would:
- Abstract over different protocols
- Support both HTTP and FTP
- Enable progress reporting for large downloads
- Handle signature verification
- Provide caching for efficiency

### 6. Package Update Management

**Current State:** No support for package updates or version comparison during installation.

**Missing Functionality:**
- Detecting if a newer version is available
- Comparing package versions for updates
- Finding packages that depend on the package being updated
- Managing update chains (recursive updates)

**Proposed API:**

```rust
// In new update module
pub struct UpdateManager<'a> {
    pkgdb: &'a PkgDB,
    available: Vec<Summary>,
}

impl<'a> UpdateManager<'a> {
    /// Create a new update manager
    pub fn new(pkgdb: &'a PkgDB, available: Vec<Summary>) -> Self;

    /// Check if a newer version of a package is available
    pub fn has_update(&self, pkgname: &str) -> Option<String>;

    /// Find all packages that depend on the given package
    pub fn find_dependents(&self, pkgname: &str) -> Vec<String>;

    /// Build an update plan for a package and its dependents
    pub fn plan_update(
        &self,
        pkgname: &str,
        recursive: bool,
    ) -> Result<Vec<String>, UpdateError>;

    /// Check if an update would break dependencies
    pub fn would_break_dependencies(
        &self,
        old_pkg: &str,
        new_pkg: &str,
    ) -> Vec<String>;
}
```

**Rationale:** Package updates require careful coordination to avoid breaking dependencies. A library API would:
- Ensure version comparison is correct
- Handle recursive update scenarios
- Prevent dependency breakage
- Support both `-U` and `-u` flag behaviors

### 7. Platform and Architecture Validation

**Current State:** Build info is accessible but not validated.

**Missing Functionality:**
- Checking package OS compatibility
- Checking package architecture compatibility
- Validation against current system
- ABI compatibility checking

**Proposed API:**

```rust
// In new platform module
pub struct PlatformInfo {
    pub os: String,
    pub os_version: String,
    pub machine_arch: String,
}

impl PlatformInfo {
    /// Get the current system's platform information
    pub fn current() -> Result<Self, PlatformError>;

    /// Check if a package is compatible with this platform
    pub fn is_compatible(&self, pkg: &BinaryPackage) -> Result<bool, PlatformError>;

    /// Get detailed compatibility information
    pub fn check_compatibility(
        &self,
        pkg: &BinaryPackage,
    ) -> CompatibilityReport;
}

pub struct CompatibilityReport {
    pub os_compatible: bool,
    pub os_version_compatible: bool,
    pub arch_compatible: bool,
    pub warnings: Vec<String>,
}
```

**Rationale:** Platform validation prevents installation of incompatible packages. A library API would:
- Detect the current system automatically
- Provide clear compatibility reporting
- Support force-install override
- Handle version compatibility ranges

### 8. Plist Utilities for File Management

**Current State:** Plist parsing is excellent, but utilities for processing plist directives are limited.

**Missing Functionality:**
- Iterating files with accumulated state (@mode, @owner, @group)
- Processing @exec and @unexec commands
- Handling @pkgdir and @dirrm directives
- Processing @option preserve

**Proposed API:**

```rust
// In plist module
pub struct PlistProcessor<'a> {
    plist: &'a Plist,
    mode: Option<u32>,
    owner: Option<String>,
    group: Option<String>,
}

impl<'a> PlistProcessor<'a> {
    /// Create a new plist processor
    pub fn new(plist: &'a Plist) -> Self;

    /// Process plist entries with state tracking
    pub fn process_entries<F>(&mut self, handler: F) -> Result<(), PlistError>
    where
        F: FnMut(&PlistEntry, &FileState) -> Result<(), PlistError>;

    /// Get directories that should be created/removed
    pub fn managed_directories(&self) -> Vec<&OsStr>;

    /// Get exec commands to run during installation
    pub fn exec_commands(&self) -> Vec<&OsStr>;

    /// Get unexec commands to run during deinstallation
    pub fn unexec_commands(&self) -> Vec<&OsStr>;
}

pub struct FileState {
    pub mode: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub preserve: bool,
}
```

**Rationale:** Plist processing requires state tracking across entries. A library API would:
- Handle stateful directives correctly
- Simplify extraction logic
- Support all plist commands
- Enable consistent plist interpretation

## Implementation Priority

Based on the complexity and utility of each API:

1. **High Priority:**
   - Package Database Write Operations (critical for any installation tool)
   - Dependency Resolution Utilities (core functionality)
   - Plist Utilities for File Management (improves extraction)

2. **Medium Priority:**
   - Conflict Detection and Resolution (important for safety)
   - Platform and Architecture Validation (prevents errors)
   - Installation Script Execution (needed for many packages)

3. **Lower Priority:**
   - URL Fetching and Remote Package Support (useful but can be external)
   - Package Update Management (can build on other APIs)

## Usage Example

Here's how the proposed APIs would simplify the pkg_add example:

```rust
// Current manual approach
let pkg_dir = self.pkg_dbdir.join(&pkg_info.pkgname);
fs::create_dir_all(&pkg_dir)?;
fs::write(pkg_dir.join("+CONTENTS"), pkg_info.package.metadata().contents())?;
fs::write(pkg_dir.join("+COMMENT"), pkg_info.package.metadata().comment())?;
// ... many more manual writes

// With proposed API
let mut db = PkgDBWriter::open(&self.pkg_dbdir)?;
db.register_package(
    &pkg_info.pkgname,
    pkg_info.package.metadata(),
    pkg_info.automatic,
)?;
```

```rust
// Current manual dependency checking
for dep_str in pkg_info.package.plist().depends() {
    let depend = Depend::new(dep_str)?;
    if let Some(satisfied) = self.is_satisfied(depend.pattern()) {
        // Handle satisfied
    } else {
        // Handle missing
    }
}

// With proposed API
let resolver = DependencyResolver::new(&pkgdb, available_packages);
let missing = resolver.resolve_dependencies(&pkg_info.package)?;
for dep in missing {
    if let Some(suggestion) = resolver.suggest_packages(&dep).first() {
        println!("Install {} to satisfy {}", suggestion, dep);
    }
}
```

## Conclusion

The current pkgsrc-rs library provides an excellent foundation for reading and understanding pkgsrc packages. Adding these APIs would transform it into a complete package management library, enabling robust implementations of `pkg_add`, `pkg_delete`, and other pkg_install tools.

The proposed APIs follow Rust best practices:
- Strong typing to prevent errors
- Builder patterns for configuration
- Clear error types for each module
- Zero-cost abstractions where possible
- Safe defaults with escape hatches for advanced use

These additions would make pkgsrc-rs the definitive library for pkgsrc package management in Rust.
