use std::path::Path;

use gix::bstr::ByteSlice as _;

use crate::Error;

/// A glob pattern with an optional local destination prefix.
///
/// Raw config syntax: `<glob>` or `<glob>:<destination>`.
/// Example: `src/**:third_party/lib/` maps upstream `src/foo.rs` to
/// `third_party/lib/foo.rs` in the local tree.
///
/// Globs use gitignore wildmatch semantics: `*` and `?` never cross `/`, and
/// there is no brace expansion. A bare directory name matches only a file
/// literally named that, never its contents — to vendor a directory, write
/// `dir/**`.
#[derive(Debug)]
pub struct PatternMapping {
    /// The glob string (left of the colon, or the whole value).
    pub glob: String,
    /// The local destination prefix (right of the colon), if present.
    pub destination: Option<String>,
}

impl PatternMapping {
    /// Parse a raw pattern string, splitting on the first `:` only.
    pub fn parse(raw: &str) -> Self {
        match raw.find(':') {
            Some(i) => Self {
                glob: raw[..i].to_owned(),
                destination: Some(raw[i + 1..].to_owned()),
            },
            None => Self {
                glob: raw.to_owned(),
                destination: None,
            },
        }
    }

    /// Serialize back to the raw config string.
    pub fn to_raw(&self) -> String {
        match &self.destination {
            Some(dest) => format!("{}:{}", self.glob, dest),
            None => self.glob.clone(),
        }
    }

    /// The literal (non-glob) leading prefix of the glob, e.g. `src/` from
    /// `src/**`. gix-glob (gitignore wildmatch) has no brace expansion, so `{`
    /// is a literal character, not a metachar boundary.
    pub fn literal_prefix(&self) -> &str {
        let end = self.glob.find(['*', '?', '[']).unwrap_or(self.glob.len());
        &self.glob[..end]
    }

    /// Map an upstream path that matched this pattern to its local path.
    ///
    /// Strips [`Self::literal_prefix`], then prepends [`Self::destination`] if
    /// set. Operates on raw bytes: upstream tree paths are arbitrary bytes, not
    /// guaranteed UTF-8, and must survive selection and remapping byte-for-byte.
    /// Returns `None` if `upstream_path` does not start with the literal prefix.
    pub fn local_path(&self, upstream_path: &gix::bstr::BStr) -> Option<gix::bstr::BString> {
        let prefix = self.literal_prefix();
        let rest = upstream_path.strip_prefix(prefix.as_bytes())?;
        let mut out: Vec<u8> =
            Vec::with_capacity(self.destination.as_ref().map_or(0, String::len) + rest.len());
        if let Some(dest) = &self.destination {
            out.extend_from_slice(dest.as_bytes());
        }
        out.extend_from_slice(rest);
        Some(out.into())
    }
}

/// A validated vendor name.
///
/// The only way to obtain one is [`VendorName::new`], which rejects anything
/// unsafe as a git ref component or git config subsection name. Holding a
/// `VendorName` is therefore a proof that `refs/vendor/<name>` cannot escape
/// its namespace and that `.gitvendors` cannot be corrupted by the name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VendorName(String);

impl VendorName {
    /// Validate and wrap a vendor name. The allowed set (`[A-Za-z0-9._-]`, no
    /// leading/trailing `.`, no `..`, no `.lock` suffix) is a subset of valid
    /// `git check-ref-format` path components and config subsection names.
    pub fn new(name: impl Into<String>) -> Result<Self, Error> {
        let name = name.into();
        validate_name(&name)?;
        Ok(Self(name))
    }

    /// The name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for VendorName {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for VendorName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for VendorName {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for VendorName {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// A vendored dependency defined in `.gitvendors`.
#[derive(Debug)]
pub struct VendorEntry {
    /// Unique identifier for this vendor; maps to `[vendor "<name>"]` in config.
    pub name: VendorName,
    /// Remote URL of the upstream repository.
    pub url: String,
    /// Branch, tag, or SHA to track on the upstream remote (defaults to `HEAD`).
    pub ref_name: Option<String>,
    /// Object ID of the most recent merge base; absent until the first merge.
    pub base: Option<gix::ObjectId>,
    /// Patterns selecting which upstream files to vendor, with optional remapping.
    pub patterns: Vec<PatternMapping>,
}

impl VendorEntry {
    /// The ref to track on the upstream remote.
    pub fn tracking_ref(&self) -> &str {
        self.ref_name.as_deref().unwrap_or("HEAD")
    }

    /// The local ref that holds the latest fetched upstream tip.
    ///
    /// Safe because [`VendorName`] is unconstructible without validation, so
    /// `name` cannot escape the `refs/vendor/` namespace.
    pub fn vendor_ref(&self) -> String {
        format!("refs/vendor/{}", self.name)
    }
}

/// Parsed `.gitvendors` configuration file (git config format).
#[derive(Debug)]
pub struct VendorConfig {
    pub(crate) file: gix::config::File<'static>,
}

impl VendorConfig {
    /// Parse configuration from a file on disk.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let file = gix::config::File::from_path_no_includes(
            path.to_path_buf(),
            gix::config::Source::Local,
        )
        .map_err(|e| Error::Config(e.to_string()))?;
        Ok(Self { file })
    }

    /// Parse configuration from a byte slice.
    pub fn open_from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let s = std::str::from_utf8(bytes).map_err(|e| Error::Config(e.to_string()))?;
        Self::parse(s)
    }

    /// Parse configuration from a string.
    pub fn parse(s: &str) -> Result<Self, Error> {
        let file = s
            .parse::<gix::config::File<'static>>()
            .map_err(|e| Error::Config(e.to_string()))?;
        Ok(Self { file })
    }

    /// Return all vendor entries.
    pub fn entries(&self) -> Result<Vec<VendorEntry>, Error> {
        let Some(sections) = self.file.sections_by_name("vendor") else {
            return Ok(Vec::new());
        };
        sections.map(entry_from_section).collect()
    }

    /// Look up a vendor entry by name.
    pub fn get(&self, name: &str) -> Result<Option<VendorEntry>, Error> {
        let Some(sections) = self.file.sections_by_name("vendor") else {
            return Ok(None);
        };
        for section in sections {
            let Some(sub) = section.header().subsection_name() else {
                continue;
            };
            if sub == gix::bstr::BStr::new(name.as_bytes()) {
                return Ok(Some(entry_from_section(section)?));
            }
        }
        Ok(None)
    }

    /// Insert or replace a vendor entry.
    pub fn insert(&mut self, entry: &VendorEntry) -> Result<(), Error> {
        let name_bstr = gix::bstr::BStr::new(entry.name.as_str().as_bytes());

        // Drain config content of all 'vendor.$name' instances.
        while self
            .file
            .remove_section("vendor", Some(name_bstr))
            .is_some()
        {}

        let subsection: std::borrow::Cow<'static, gix::bstr::BStr> =
            std::borrow::Cow::Owned(gix::bstr::BString::from(entry.name.as_str().as_bytes()));
        let mut section = self
            .file
            .new_section("vendor", Some(subsection))
            .map_err(|e| Error::Config(e.to_string()))?;

        push_kv(&mut section, "url", &entry.url);
        if let Some(r) = &entry.ref_name {
            push_kv(&mut section, "ref", r);
        }
        if let Some(base) = &entry.base {
            push_kv(&mut section, "base", &base.to_hex().to_string());
        }
        for pattern in &entry.patterns {
            push_kv(&mut section, "pattern", &pattern.to_raw());
        }

        Ok(())
    }

    /// Remove a vendor entry by name. Returns `true` if it existed.
    pub fn remove(&mut self, name: &str) -> Result<bool, Error> {
        let name_bstr = gix::bstr::BStr::new(name.as_bytes());
        Ok(self
            .file
            .remove_section("vendor", Some(name_bstr))
            .is_some())
    }
}

fn entry_from_section(section: &gix::config::file::Section<'_>) -> Result<VendorEntry, Error> {
    let name = section
        .header()
        .subsection_name()
        .ok_or_else(|| Error::Config("vendor section missing name".into()))?
        .to_str_lossy()
        .into_owned();
    let name = VendorName::new(name)?;

    let body = section.body();

    let url = body
        .value("url")
        .ok_or_else(|| Error::Config(format!("vendor '{name}' missing url")))?
        .to_str_lossy()
        .into_owned();

    let ref_name = body.value("ref").map(|v| v.to_str_lossy().into_owned());

    let base = body
        .value("base")
        .and_then(|v| gix::ObjectId::from_hex(v.as_bytes()).ok());

    let patterns = body
        .values("pattern")
        .iter()
        .map(|v| PatternMapping::parse(&v.to_str_lossy()))
        .collect();

    Ok(VendorEntry {
        name,
        url,
        ref_name,
        base,
        patterns,
    })
}

/// Reject vendor names that are unsafe as a git ref component or a git config
/// subsection name. The allowed set (`[A-Za-z0-9._-]`, no leading/trailing
/// `.`, no `..`, no `.lock` suffix) is a subset of valid `git check-ref-format`
/// path components and config subsection names, so a validated name cannot
/// escape `refs/vendor/<name>` or corrupt `.gitvendors`.
fn validate_name(name: &str) -> Result<(), Error> {
    if name.is_empty() {
        return Err(Error::InvalidName("name is empty".into()));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(Error::InvalidName(format!(
            "{name:?} contains characters outside [A-Za-z0-9._-]"
        )));
    }
    if name.starts_with('.') || name.ends_with('.') {
        return Err(Error::InvalidName(format!(
            "{name:?} may not start or end with '.'"
        )));
    }
    if name.contains("..") {
        return Err(Error::InvalidName(format!("{name:?} may not contain '..'")));
    }
    if name.ends_with(".lock") {
        return Err(Error::InvalidName(format!(
            "{name:?} may not end with '.lock'"
        )));
    }
    Ok(())
}

fn push_kv(section: &mut gix::config::file::SectionMut<'_, 'static>, key: &str, val: &str) {
    section.push(
        gix::config::parse::section::ValueName::try_from(key.to_owned())
            .expect("static key is valid"),
        Some(gix::bstr::BStr::new(val.as_bytes())),
    );
}

impl std::fmt::Display for VendorConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.file)
    }
}

/// The relationship between a vendor's recorded base and the current upstream tip.
#[derive(Debug)]
pub enum VendorStatus {
    /// `refs/vendor/<name>` does not exist; the vendor has never been fetched.
    NotFetched,
    /// The recorded base matches the current upstream tip.
    UpToDate,
    /// A new upstream tip is available and is a descendant of the recorded base.
    UpdateAvailable { upstream: gix::ObjectId },
    /// The upstream tip is not a descendant of the recorded base (force-push detected).
    ForcePushed { upstream: gix::ObjectId },
}

/// The outcome of three-way merging an upstream commit into the local vendor paths.
///
/// Self-contained: carries the upstream commit and ancestor so the result can be
/// committed independently of how it was produced.
#[derive(Debug)]
pub struct VendorMerge {
    /// Upstream commit that was merged in; becomes the vendor's new `base`.
    ///
    /// A commit, distinct from [`upstream_tree`](VendorRepository::upstream_tree).
    pub upstream_commit: gix::ObjectId,
    /// Ancestor tree used for the three-way merge (`None` on the first add).
    ///
    /// A tree, distinct from [`VendorEntry::base`], which is a commit.
    pub ancestor_tree: Option<gix::ObjectId>,
    /// The merged result tree in local path space.
    ///
    /// Textual conflicts are written with conflict markers; binary conflicts keep
    /// the local ("ours") blob. The conflicting paths are listed in `conflicts`.
    pub result_tree: gix::ObjectId,
    /// Local paths with unresolved conflicts, empty on a clean merge.
    pub conflicts: Vec<String>,
}

impl VendorMerge {
    /// Whether the merge produced unresolved conflicts.
    pub fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }
}

/// Object-database and ref operations for managing vendored external content.
///
/// Every method here is a function over the object database and refs: it reads
/// and writes git objects (trees, commits) and the private `refs/vendor/<name>`
/// bookmark, and never touches the working copy. Projecting results into the
/// working copy is [`VendorWorktree`].
///
/// No method reads ambient `HEAD` or index state: the local ("ours") commit and
/// any commit parent are always passed explicitly, keeping this a pure
/// object-database function. Defaulting them to `HEAD` is caller policy.
///
/// Every tree-returning method works in *local path space*. The upstream side
/// ([`upstream_tree`](Self::upstream_tree), [`base_tree`](Self::base_tree)) gets
/// there via the entry's [`PatternMapping`]s; the local side
/// ([`ours_tree`](Self::ours_tree)) via the `.gitattributes` content filter.
pub trait VendorRepository {
    /// Fetch the upstream remote into `refs/vendor/<name>`, returning the tip OID.
    fn fetch_vendor(&self, entry: &VendorEntry) -> Result<gix::ObjectId, Error>;

    /// The upstream tip recorded in `refs/vendor/<name>`, if it has been fetched.
    ///
    /// Reads the local ref only; never touches the network.
    fn vendor_tip(&self, entry: &VendorEntry) -> Result<Option<gix::ObjectId>, Error>;

    /// Compare the vendor's recorded base against the current upstream tip.
    fn vendor_status(&self, entry: &VendorEntry) -> Result<VendorStatus, Error>;

    /// The filtered and remapped upstream tree at `commit`.
    fn upstream_tree(
        &self,
        entry: &VendorEntry,
        commit: gix::ObjectId,
    ) -> Result<gix::ObjectId, Error>;

    /// The filtered and remapped upstream tree at the recorded `base`.
    ///
    /// Equivalent to [`upstream_tree`](Self::upstream_tree) at `entry.base`.
    /// Returns `None` before the first add, in which case the empty tree is the
    /// implied merge ancestor.
    fn base_tree(&self, entry: &VendorEntry) -> Result<Option<gix::ObjectId>, Error>;

    /// The vendor's local snapshot ("ours"): the `ours` commit's tree filtered
    /// by the `.gitattributes` `vendor=<name>` content filter, restricted to
    /// [`vendor_paths`](Self::vendor_paths).
    ///
    /// `ours` is supplied explicitly; defaulting it to `HEAD` is caller policy.
    fn ours_tree(&self, entry: &VendorEntry, ours: gix::ObjectId) -> Result<gix::ObjectId, Error>;

    /// The local paths carrying the `vendor=<name>` attribute — the local-side
    /// content filter, independent of the upstream pattern filter.
    ///
    /// Resolved from `.gitattributes` in the `ours` commit's tree (the same
    /// tree [`ours_tree`](Self::ours_tree) draws content from), never the
    /// working copy. `ours` is supplied explicitly; defaulting it to `HEAD` is
    /// caller policy.
    fn vendor_paths(&self, entry: &VendorEntry, ours: gix::ObjectId) -> Result<Vec<String>, Error>;

    /// Three-way merge the pattern-filtered upstream tree at `theirs`
    /// ("theirs") against the attribute-filtered tree of the `ours` commit
    /// ("ours"), with [`base_tree`](Self::base_tree) as the ancestor (the empty
    /// tree on the first add, which makes this the "add" operation).
    ///
    /// Both sides are passed explicitly; defaulting `ours` to `HEAD` is caller
    /// policy. Conflicts are reported in the returned [`VendorMerge`], not
    /// raised as errors and not auto-resolved. Produces objects only; no ref or
    /// working tree is moved.
    fn merge_vendor(
        &self,
        entry: &VendorEntry,
        ours: gix::ObjectId,
        theirs: gix::ObjectId,
    ) -> Result<VendorMerge, Error>;

    /// Mint the merge commit recording `merge` on top of the `parent` local
    /// commit, returning the new local commit.
    ///
    /// The result is a two-parent merge commit: `parent` is the first parent
    /// (the local "ours" commit the result builds on, passed explicitly;
    /// defaulting it to `HEAD` is caller policy), and `merge.upstream_commit`
    /// is the second parent — a real commit edge into the `refs/vendor/<name>`
    /// graph recording the pristine upstream point that was integrated. Does
    /// not move `HEAD` or any branch ref. The caller integrates the commit and
    /// advances `entry.base` to `merge.upstream_commit`, persisting the authoritative
    /// pointer via [`VendorConfig::insert`].
    fn commit_vendor(
        &self,
        entry: &VendorEntry,
        parent: gix::ObjectId,
        merge: &VendorMerge,
    ) -> Result<gix::ObjectId, Error>;
}
