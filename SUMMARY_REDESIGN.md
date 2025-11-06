# Summary Module Redesign

## Current Problems

1. **SummaryStream** - Awkward Write trait implementation for parsing
2. **Runtime panics** - HashMap with panic! on type mismatches
3. **No serde** - Can't serialize/deserialize summaries
4. **Verbose API** - 20+ setter methods
5. **No iterator traits** - Can't iterate over variables
6. **No indexing** - Can't use `summary[SummaryVariable::Pkgname]`

## New Design Goals

### 1. Single Package Summary
```rust
// A single pkg_summary entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    // Individual fields for type safety and serde
    // Required fields
    build_date: String,
    categories: String,
    comment: String,
    description: Vec<String>,
    machine_arch: String,
    opsys: String,
    os_version: String,
    pkgname: String,
    pkgpath: String,
    pkgtools_version: String,
    size_pkg: i64,

    // Optional fields
    #[serde(skip_serializing_if = "Option::is_none")]
    conflicts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    depends: Option<Vec<String>>,
    // ... other optional fields
}
```

### 2. Collection of Summaries
```rust
// A collection of pkg_summary entries
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summaries {
    entries: Vec<Summary>,
}

// Implement IntoIterator
impl IntoIterator for Summaries {
    type Item = Summary;
    type IntoIter = std::vec::IntoIter<Summary>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

// Implement Index
impl Index<usize> for Summaries {
    type Output = Summary;
    fn index(&self, idx: usize) -> &Self::Output {
        &self.entries[idx]
    }
}
```

### 3. Variable Access
```rust
// Access variables dynamically if needed
impl Summary {
    pub fn get(&self, var: SummaryVariable) -> Option<SummaryValue> {
        match var {
            SummaryVariable::Pkgname => Some(SummaryValue::S(&self.pkgname)),
            SummaryVariable::Comment => Some(SummaryValue::S(&self.comment)),
            // ...
        }
    }
}

// Or use Index trait
impl Index<SummaryVariable> for Summary {
    type Output = str; // for string variables
    fn index(&self, var: SummaryVariable) -> &Self::Output {
        // ...
    }
}
```

### 4. Parsing
```rust
// Parse from string
impl FromStr for Summary {
    type Err = SummaryError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // ...
    }
}

// Parse multiple entries
impl FromStr for Summaries {
    type Err = SummaryError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.split("\n\n")
            .filter(|s| !s.trim().is_empty())
            .map(Summary::from_str)
            .collect()
    }
}

// Read from reader
impl Summaries {
    pub fn from_reader(reader: impl BufRead) -> Result<Self> {
        // Streaming parser
    }
}
```

### 5. Builder Pattern
```rust
// Fluent builder for constructing summaries
pub struct SummaryBuilder {
    // Private fields
}

impl SummaryBuilder {
    pub fn new() -> Self { ... }

    pub fn pkgname(mut self, pkgname: impl Into<String>) -> Self {
        self.pkgname = Some(pkgname.into());
        self
    }

    pub fn build(self) -> Result<Summary, SummaryError> {
        // Validate all required fields present
    }
}
```

## API Examples

```rust
// Parse a single summary
let summary: Summary = "PKGNAME=foo-1.0\n...".parse()?;

// Parse multiple summaries
let summaries: Summaries = "PKGNAME=foo-1.0\n...\n\nPKGNAME=bar-2.0\n...".parse()?;

// Iterate
for summary in &summaries {
    println!("{}", summary.pkgname);
}

// Index
let first = &summaries[0];

// Access variables
println!("{}", summary.pkgname);
println!("{:?}", summary.depends);

// Build
let summary = SummaryBuilder::new()
    .pkgname("test-1.0")
    .comment("A test")
    .categories("devel")
    .build()?;

// Serialize
let json = serde_json::to_string(&summary)?;
let yaml = serde_yaml::to_string(&summaries)?;

// From reader
let file = File::open("pkg_summary.txt")?;
let summaries = Summaries::from_reader(BufReader::new(file))?;
```

## Implementation Plan

1. ✅ Create design document
2. [ ] Implement new Summary struct with all fields
3. [ ] Add serde derives and attributes
4. [ ] Implement FromStr for Summary
5. [ ] Implement Summaries collection
6. [ ] Add all iterator traits
7. [ ] Add builder pattern
8. [ ] Migrate tests
9. [ ] Update examples
10. [ ] Deprecate SummaryStream
