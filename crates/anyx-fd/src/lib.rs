//! `fd`, as an anyx protocol tool: finds files by name, skipping what ignore
//! files ignore. It is built on the libraries `fd` is built on (`ignore`,
//! `regex` and `globset`), not on `fd`'s command line.
//!
//! [`answer`] reads one request and writes its response, as the program
//! `anyx-fd --anyx-protocol` does for the anyx shell.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use anyx_protocol::server::{argument, built_in, read_request, respond, validate_outcome};
use anyx_protocol::{Call, IntoValue, Outcome, Schema, Value};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;

/// The schema, as written.
pub const SOURCE: &str = include_str!("../fd.protocol.anyx");

/// The schema's file, as `SCHEMA FILES` names it.
pub const PATH: &str = "fd.protocol.anyx";

/// The schema, read once.
pub fn schema() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        Schema::load(PATH, |path| match path == PATH {
            true => Ok(SOURCE.to_string()),
            false => Err(format!("fd has no file `{path}`")),
        })
        .expect("the fd schema is valid")
    })
}

/// Answers `request`, with paths relative to `directory`.
pub fn answer(request: &str, directory: &Path) -> String {
    let schema = schema();
    let calls = match read_request(schema, request) {
        Ok(calls) => calls,
        Err(response) => return response,
    };
    let mut outcomes = Vec::new();
    let mut stopped = false;
    for call in &calls {
        if stopped {
            outcomes.push(Outcome::NotRun);
            continue;
        }
        let outcome = built_in(schema, call).unwrap_or_else(|| invoke(call, directory));
        if validate_outcome(schema, call, &outcome).is_err() {
            stopped = true;
            outcomes.push(Outcome::InternalError);
        } else {
            stopped = matches!(outcome, Outcome::InternalError);
            outcomes.push(outcome);
        }
    }
    respond(schema, &calls, outcomes)
}

fn invoke(call: &Call, directory: &Path) -> Outcome {
    let method = schema().method_at(call.method);
    let mut arguments = call.arguments.clone().into_iter();
    let pattern: String = argument(&mut arguments);
    // `find _`, `details of _ in _ with _`: the verb, then which of the
    // paths and the settings the shape has.
    let shape = method.shape.text.as_str();
    let (verb, rest) = ["find _", "details of _", "any match of _"]
        .into_iter()
        .find_map(|verb| Some((verb, shape.strip_prefix(verb)?)))
        .unwrap_or_else(|| unreachable!("fd has no method `{shape}`"));
    let paths = match rest.starts_with(" in _") {
        true => argument(&mut arguments),
        false => Vec::new(),
    };
    let fields = match rest.ends_with(" with _") {
        true => match arguments.next() {
            Some(Value::Record(fields)) => fields,
            _ => unreachable!("the codec checked the search"),
        },
        false => Default::default(),
    };
    let search = match Search::new(&pattern, paths, &Fields(fields), directory) {
        Ok(search) => search,
        Err(error) => return Outcome::Error(error),
    };
    let found = match verb {
        "any match of _" => search.run(Some(1)),
        _ => search.run(None),
    };
    let found = match found {
        Ok(found) => found,
        Err(error) => return Outcome::Error(error),
    };
    Outcome::Value(match verb {
        "any match of _" => (!found.is_empty()).into_value(),
        "details of _" => Value::List(found.iter().map(Found::details).collect()),
        _ => Value::List(
            found
                .into_iter()
                .map(|found| Value::Text(found.path))
                .collect(),
        ),
    })
}

/// The fields of a `Search`, each read as its type; a field left out, as
/// in a method without `with _`, reads as its default.
struct Fields(std::collections::BTreeMap<String, Value>);

impl Fields {
    fn get(&self, name: &str) -> Option<&Value> {
        // A nullable field's `value _` holds its item.
        match self.0.get(name)? {
            Value::Variant { shape, arguments } if shape == "value _" => arguments.first(),
            Value::Variant { shape, .. } if shape == "null" => None,
            value => Some(value),
        }
    }

    fn checked(&self, name: &str) -> bool {
        matches!(self.get(name), Some(Value::Variant { shape, .. }) if shape == "true")
    }

    fn variant(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Value::Variant { shape, .. } => Some(shape),
            _ => unreachable!("the codec checked `{name}`"),
        }
    }

    fn integer(&self, name: &str) -> Option<i64> {
        match self.get(name)? {
            Value::Integer(n) => Some(*n),
            _ => unreachable!("the codec checked `{name}`"),
        }
    }

    fn instant(&self, name: &str) -> Option<SystemTime> {
        match self.get(name)? {
            Value::Instant(time) => Some(*time),
            _ => unreachable!("the codec checked `{name}`"),
        }
    }

    /// The time `name`'s duration ago.
    fn ago(&self, name: &str, now: SystemTime) -> Option<SystemTime> {
        match self.get(name)? {
            Value::Duration(period) => {
                Some(now.checked_sub(*period).unwrap_or(SystemTime::UNIX_EPOCH))
            }
            _ => unreachable!("the codec checked `{name}`"),
        }
    }

    fn list(&self, name: &str) -> &[Value] {
        match self.get(name) {
            Some(Value::List(items)) => items,
            None => &[],
            _ => unreachable!("the codec checked `{name}`"),
        }
    }

    fn texts(&self, name: &str) -> Vec<String> {
        self.list(name)
            .iter()
            .map(|item| match item {
                Value::Text(text) => text.clone(),
                _ => unreachable!("the codec checked `{name}`"),
            })
            .collect()
    }
}

fn error(shape: &str, arguments: Vec<Value>) -> Value {
    Value::Variant {
        shape: shape.to_string(),
        arguments,
    }
}

enum Syntax {
    Regex,
    Glob,
    Fixed,
}

enum Case {
    Exact,
    Any,
    Smart,
}

enum Matcher {
    Regex(regex::Regex),
    Glob(globset::GlobMatcher),
}

impl Matcher {
    fn new(pattern: &str, syntax: &Syntax, case: &Case) -> Result<Self, Value> {
        let insensitive = match case {
            Case::Exact => false,
            Case::Any => true,
            Case::Smart => !pattern.chars().any(char::is_uppercase),
        };
        match syntax {
            Syntax::Glob => globset::GlobBuilder::new(pattern)
                .case_insensitive(insensitive)
                .literal_separator(true)
                .build()
                .map(|glob| Self::Glob(glob.compile_matcher()))
                .map_err(|problem| {
                    error(
                        "invalid glob _",
                        vec![Value::Text(problem.kind().to_string())],
                    )
                }),
            Syntax::Regex | Syntax::Fixed => {
                let pattern = match syntax {
                    Syntax::Fixed => regex::escape(pattern),
                    _ => pattern.to_string(),
                };
                regex::RegexBuilder::new(&pattern)
                    .case_insensitive(insensitive)
                    .build()
                    .map(Self::Regex)
                    .map_err(|problem| {
                        error(
                            "invalid pattern _",
                            vec![Value::Text(regex_message(&problem))],
                        )
                    })
            }
        }
    }

    fn matches(&self, subject: &str) -> bool {
        match self {
            Self::Regex(regex) => regex.is_match(subject),
            Self::Glob(glob) => glob.is_match(subject),
        }
    }
}

/// The last line of a regex error, without its `error: `, such as
/// `unclosed group`.
fn regex_message(problem: &regex::Error) -> String {
    let text = problem.to_string();
    let last = text.lines().last().unwrap_or_default();
    last.strip_prefix("error: ").unwrap_or(last).to_string()
}

/// A kind of file `only _` allows.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    File,
    Directory,
    Symlink,
    Executable,
    Empty,
    Socket,
    Pipe,
    BlockDevice,
    CharDevice,
}

impl Kind {
    fn read(value: &Value) -> Self {
        match value {
            Value::Variant { shape, .. } => match shape.as_str() {
                "file" => Self::File,
                "directory" => Self::Directory,
                "symlink" => Self::Symlink,
                "executable" => Self::Executable,
                "empty" => Self::Empty,
                "socket" => Self::Socket,
                "pipe" => Self::Pipe,
                "block device" => Self::BlockDevice,
                "char device" => Self::CharDevice,
                other => unreachable!("the codec checked the kind `{other}`"),
            },
            _ => unreachable!("the codec checked the kind"),
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
            Self::Symlink => "symlink",
            Self::Executable => "executable",
            Self::Empty => "empty",
            Self::Socket => "socket",
            Self::Pipe => "pipe",
            Self::BlockDevice => "block device",
            Self::CharDevice => "char device",
        }
    }
}

struct Search {
    directory: PathBuf,
    /// Each search path as written, or `None` for the working directory.
    roots: Vec<Option<String>>,
    matchers: Vec<Matcher>,
    full_path: bool,
    hidden: bool,
    no_ignore: bool,
    no_ignore_vcs: bool,
    no_ignore_parent: bool,
    no_require_git: bool,
    ignore_files: Vec<PathBuf>,
    excluding: Vec<String>,
    follow_links: bool,
    one_file_system: bool,
    prune: bool,
    min_depth: Option<usize>,
    max_depth: Option<usize>,
    kinds: Vec<Kind>,
    extensions: Vec<String>,
    at_least: Option<u64>,
    at_most: Option<u64>,
    changed_after: Option<SystemTime>,
    changed_before: Option<SystemTime>,
    user: Option<i64>,
    group: Option<i64>,
    limit: Option<usize>,
}

impl Search {
    fn new(
        pattern: &str,
        paths: Vec<String>,
        fields: &Fields,
        directory: &Path,
    ) -> Result<Self, Value> {
        let now = SystemTime::now();
        let depth = |name: &str| {
            fields
                .integer(name)
                .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
        };
        let size = |name: &str| fields.integer(name).map(|n| n as u64);
        // Both bounds of a side must hold, so the later lower bound and the
        // earlier upper bound win.
        let changed_after = [
            fields.ago("changed within", now),
            fields.instant("changed since"),
        ]
        .into_iter()
        .flatten()
        .max();
        let changed_before = [
            fields.instant("changed before"),
            fields.ago("changed more than ago", now),
        ]
        .into_iter()
        .flatten()
        .min();
        let mut ignore_files = Vec::new();
        for path in fields.texts("ignore files") {
            let full = directory.join(&path);
            if !full.exists() {
                return Err(error("_ does not exist", vec![Value::Text(path)]));
            }
            ignore_files.push(full);
        }
        let syntax = match fields.variant("pattern") {
            Some("glob") => Syntax::Glob,
            Some("fixed text") => Syntax::Fixed,
            _ => Syntax::Regex,
        };
        let case = match fields.variant("case") {
            Some("exact") => Case::Exact,
            Some("any") => Case::Any,
            _ => Case::Smart,
        };
        let mut patterns = vec![pattern.to_string()];
        patterns.extend(fields.texts("also matching"));
        let matchers = patterns
            .iter()
            .map(|pattern| Matcher::new(pattern, &syntax, &case))
            .collect::<Result<_, _>>()?;
        Ok(Search {
            directory: directory.to_path_buf(),
            roots: match paths.is_empty() {
                true => vec![None],
                false => paths.into_iter().map(Some).collect(),
            },
            matchers,
            full_path: fields.checked("full path"),
            hidden: fields.checked("hidden"),
            no_ignore: fields.checked("no ignore"),
            no_ignore_vcs: fields.checked("no ignore vcs"),
            no_ignore_parent: fields.checked("no ignore parent"),
            no_require_git: fields.checked("no require git"),
            ignore_files,
            excluding: fields.texts("excluding"),
            follow_links: fields.checked("follow links"),
            one_file_system: fields.checked("one file system"),
            prune: fields.checked("prune"),
            min_depth: depth("min depth"),
            max_depth: depth("max depth"),
            kinds: fields.list("only").iter().map(Kind::read).collect(),
            extensions: fields
                .texts("extensions")
                .iter()
                .map(|extension| extension.trim_start_matches('.').to_lowercase())
                .collect(),
            at_least: size("at least bytes"),
            at_most: size("at most bytes"),
            changed_after,
            changed_before,
            user: fields.integer("owned by user"),
            group: fields.integer("owned by group"),
            limit: depth("at most results"),
        })
    }

    /// What the search finds, in the order of the walk: by name, each
    /// directory before what it holds. `limit` stops it early.
    fn run(&self, limit: Option<usize>) -> Result<Vec<Found>, Value> {
        let limit = match (limit, self.limit) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let mut found = Vec::new();
        for root in &self.roots {
            let start = match root {
                Some(root) => self.directory.join(root),
                None => self.directory.clone(),
            };
            if !start.exists() {
                return Err(error(
                    "_ does not exist",
                    vec![Value::Text(root.clone().unwrap_or_else(|| ".".into()))],
                ));
            }
            let mut walk = WalkBuilder::new(&start);
            walk.standard_filters(!self.no_ignore)
                .hidden(!self.hidden)
                .parents(!self.no_ignore && !self.no_ignore_parent)
                .require_git(!self.no_require_git)
                .follow_links(self.follow_links)
                .same_file_system(self.one_file_system)
                .max_depth(self.max_depth)
                .sort_by_file_name(|a, b| a.cmp(b));
            if !self.no_ignore {
                walk.add_custom_ignore_filename(".fdignore");
            }
            if self.no_ignore_vcs {
                walk.git_ignore(false).git_global(false).git_exclude(false);
            }
            for file in &self.ignore_files {
                walk.add_ignore(file);
            }
            if !self.excluding.is_empty() {
                let mut overrides = OverrideBuilder::new(&start);
                for glob in &self.excluding {
                    overrides.add(&format!("!{glob}")).map_err(|problem| {
                        error("invalid glob _", vec![Value::Text(glob_message(&problem))])
                    })?;
                }
                walk.overrides(overrides.build().map_err(|problem| {
                    error("invalid glob _", vec![Value::Text(glob_message(&problem))])
                })?);
            }
            let mut pruned: Vec<PathBuf> = Vec::new();
            for entry in walk.build() {
                // Like fd, skip what can't be read rather than fail.
                let Ok(entry) = entry else { continue };
                if entry.depth() == 0 {
                    continue;
                }
                if pruned
                    .iter()
                    .any(|directory| entry.path().starts_with(directory))
                {
                    continue;
                }
                let relative = entry
                    .path()
                    .strip_prefix(&start)
                    .expect("the walk stays under its start");
                let path = match root {
                    Some(root) => Path::new(root).join(relative),
                    None => relative.to_path_buf(),
                };
                let Some(text) = path.to_str() else {
                    return Err(error(
                        "_ is not unicode",
                        vec![Value::Text(path.to_string_lossy().into_owned())],
                    ));
                };
                if !self.matches(&entry, &path) {
                    continue;
                }
                let is_directory = entry.file_type().is_some_and(|kind| kind.is_dir());
                if self.prune && is_directory {
                    pruned.push(entry.path().to_path_buf());
                }
                if !self.filters(&entry) {
                    continue;
                }
                found.push(Found {
                    path: text.to_string(),
                    entry,
                    follow_links: self.follow_links,
                });
                if limit.is_some_and(|limit| found.len() >= limit) {
                    return Ok(found);
                }
            }
        }
        Ok(found)
    }

    fn matches(&self, entry: &ignore::DirEntry, path: &Path) -> bool {
        let subject = match self.full_path {
            true => self.directory.join(path).to_string_lossy().into_owned(),
            false => entry.file_name().to_string_lossy().into_owned(),
        };
        self.matchers
            .iter()
            .all(|matcher| matcher.matches(&subject))
    }

    fn filters(&self, entry: &ignore::DirEntry) -> bool {
        if self.min_depth.is_some_and(|depth| entry.depth() < depth) {
            return false;
        }
        if !self.kinds.is_empty() && !self.kinds.iter().any(|kind| is(entry, *kind)) {
            return false;
        }
        if !self.extensions.is_empty() {
            let extension = entry
                .path()
                .extension()
                .map(|extension| extension.to_string_lossy().to_lowercase());
            if !extension.is_some_and(|extension| self.extensions.contains(&extension)) {
                return false;
            }
        }
        let needs_metadata = self.at_least.is_some()
            || self.at_most.is_some()
            || self.changed_after.is_some()
            || self.changed_before.is_some()
            || self.user.is_some()
            || self.group.is_some();
        if !needs_metadata {
            return true;
        }
        let Ok(metadata) = entry.metadata() else {
            return false;
        };
        if self.at_least.is_some() || self.at_most.is_some() {
            // As in fd, a size is of a file.
            if !metadata.is_file()
                || self.at_least.is_some_and(|size| metadata.len() < size)
                || self.at_most.is_some_and(|size| metadata.len() > size)
            {
                return false;
            }
        }
        if self.changed_after.is_some() || self.changed_before.is_some() {
            let Ok(modified) = metadata.modified() else {
                return false;
            };
            if self.changed_after.is_some_and(|time| modified < time)
                || self.changed_before.is_some_and(|time| modified >= time)
            {
                return false;
            }
        }
        owned(&metadata, self.user, self.group)
    }
}

#[cfg(unix)]
fn owned(metadata: &std::fs::Metadata, user: Option<i64>, group: Option<i64>) -> bool {
    use std::os::unix::fs::MetadataExt;
    user.is_none_or(|id| i64::from(metadata.uid()) == id)
        && group.is_none_or(|id| i64::from(metadata.gid()) == id)
}

#[cfg(not(unix))]
fn owned(_: &std::fs::Metadata, user: Option<i64>, group: Option<i64>) -> bool {
    user.is_none() && group.is_none()
}

fn glob_message(problem: &ignore::Error) -> String {
    match problem {
        ignore::Error::Glob { err, .. } => err.clone(),
        other => other.to_string(),
    }
}

fn is(entry: &ignore::DirEntry, kind: Kind) -> bool {
    let Some(file_type) = entry.file_type() else {
        return false;
    };
    match kind {
        Kind::File => file_type.is_file(),
        Kind::Directory => file_type.is_dir(),
        Kind::Symlink => entry.path_is_symlink(),
        Kind::Executable => file_type.is_file() && executable(entry),
        Kind::Empty => match file_type.is_dir() {
            true => {
                std::fs::read_dir(entry.path()).is_ok_and(|mut entries| entries.next().is_none())
            }
            false => file_type.is_file() && entry.metadata().is_ok_and(|m| m.len() == 0),
        },
        Kind::Socket | Kind::Pipe | Kind::BlockDevice | Kind::CharDevice => {
            special(&file_type, kind)
        }
    }
}

#[cfg(unix)]
fn executable(entry: &ignore::DirEntry) -> bool {
    use std::os::unix::fs::PermissionsExt;
    entry
        .metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_: &ignore::DirEntry) -> bool {
    false
}

#[cfg(unix)]
fn special(file_type: &std::fs::FileType, kind: Kind) -> bool {
    use std::os::unix::fs::FileTypeExt;
    match kind {
        Kind::Socket => file_type.is_socket(),
        Kind::Pipe => file_type.is_fifo(),
        Kind::BlockDevice => file_type.is_block_device(),
        Kind::CharDevice => file_type.is_char_device(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn special(_: &std::fs::FileType, _: Kind) -> bool {
    false
}

struct Found {
    path: String,
    entry: ignore::DirEntry,
    follow_links: bool,
}

impl Found {
    /// A `Details` record.
    fn details(&self) -> Value {
        let metadata = match self.follow_links {
            true => std::fs::metadata(self.entry.path()),
            false => std::fs::symlink_metadata(self.entry.path()),
        }
        .ok();
        let kind = [
            Kind::Symlink,
            Kind::Directory,
            Kind::File,
            Kind::Socket,
            Kind::Pipe,
            Kind::BlockDevice,
            Kind::CharDevice,
        ]
        .into_iter()
        .find(|kind| is(&self.entry, *kind))
        .unwrap_or(Kind::File);
        Value::Record(
            [
                ("path".to_string(), Value::Text(self.path.clone())),
                (
                    "kind".to_string(),
                    Value::Variant {
                        shape: kind.word().to_string(),
                        arguments: Vec::new(),
                    },
                ),
                (
                    "size".to_string(),
                    Value::Integer(
                        metadata
                            .as_ref()
                            .map_or(0, |m| i64::try_from(m.len()).unwrap_or(i64::MAX)),
                    ),
                ),
                (
                    "modified".to_string(),
                    Value::Instant(
                        metadata
                            .and_then(|m| m.modified().ok())
                            .unwrap_or(SystemTime::UNIX_EPOCH),
                    ),
                ),
            ]
            .into(),
        )
    }
}
