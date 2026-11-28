//! `fd`, as an anyx core tool: finds files by name, skipping what ignore
//! files ignore. It is built on the libraries `fd` is built on (`ignore`,
//! `regex` and `globset`), not on `fd`'s command line.
//!
//! Its schema is `fd.anyx`, and its server is generated from it. [`answer`]
//! answers one request, a JSON list of calls, as the program
//! `anyx-fd --anyx-protocol` does for the anyx shell.
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;

include!(concat!(env!("OUT_DIR"), "/protocol.rs"));

use protocol::{
    Age, Case as CaseChoice, Details, End, FdError, FileKind, FileSystems, GitRepository, Group,
    HiddenFiles, IgnoreFiles, LeastDepth, LeastSize, Limit, Links, MatchedDirectories, Moment,
    MostDepth, MostSize, Oldness, Outcome, ParentIgnores, Recency, Start, Subject,
    Syntax as SyntaxChoice, User, VersionControlIgnores,
};

/// The schema, as written.
pub const SCHEMA: &str = include_str!("../fd.anyx");

/// Answers `request`, a JSON list of calls, with paths relative to
/// `directory`.
pub fn answer(request: &str, directory: &Path) -> String {
    block_on(protocol::handle_request(
        &Fd {
            directory: directory.to_path_buf(),
        },
        request,
    ))
}

/// Runs a future that never waits, as every one of fd's does.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

struct Fd {
    directory: PathBuf,
}

impl protocol::Server for Fd {
    async fn paths_matching_under_by(
        &self,
        pattern: String,
        paths: Vec<String>,
        search: protocol::Search,
    ) -> Outcome<Vec<String>> {
        outcome(
            Search::new(&pattern, paths, &search, &self.directory)
                .and_then(|search| search.run(None))
                .map(|found| found.into_iter().map(|found| found.path).collect()),
        )
    }

    async fn details_matching_under_by(
        &self,
        pattern: String,
        paths: Vec<String>,
        search: protocol::Search,
    ) -> Outcome<Vec<Details>> {
        outcome(
            Search::new(&pattern, paths, &search, &self.directory)
                .and_then(|search| search.run(None))
                .map(|found| found.iter().map(Found::details).collect()),
        )
    }

    async fn whether_any_matches_under_by(
        &self,
        pattern: String,
        paths: Vec<String>,
        search: protocol::Search,
    ) -> Outcome<bool> {
        outcome(
            Search::new(&pattern, paths, &search, &self.directory)
                .and_then(|search| search.run(Some(1)))
                .map(|found| !found.is_empty()),
        )
    }
}

fn outcome<T>(result: Result<T, FdError>) -> Outcome<T> {
    match result {
        Ok(result) => Outcome::Answered { result },
        Err(error) => Outcome::Failed { error },
    }
}

fn age(age: &Age) -> Duration {
    let (count, seconds) = match age {
        Age::Seconds { count } => (*count, 1),
        Age::Minutes { count } => (*count, 60),
        Age::Hours { count } => (*count, 60 * 60),
        Age::Days { count } => (*count, 24 * 60 * 60),
        Age::Weeks { count } => (*count, 7 * 24 * 60 * 60),
    };
    Duration::from_secs(count.saturating_mul(seconds))
}

fn moment(moment: &Moment) -> SystemTime {
    let Moment { seconds } = moment;
    if *seconds >= 0 {
        UNIX_EPOCH + Duration::from_secs(seconds.unsigned_abs())
    } else {
        UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs())
    }
}

fn seconds_since_the_epoch(time: SystemTime) -> Moment {
    let seconds = match time.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    };
    Moment { seconds }
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
    fn new(pattern: &str, syntax: &Syntax, case: &Case) -> Result<Self, FdError> {
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
                .map_err(|problem| FdError::InvalidGlob {
                    message: problem.kind().to_string(),
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
                    .map_err(|problem| FdError::InvalidPattern {
                        message: regex_message(&problem),
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
    fn read(kind: &FileKind) -> Self {
        match kind {
            FileKind::File => Self::File,
            FileKind::Directory => Self::Directory,
            FileKind::Symlink => Self::Symlink,
            FileKind::Executable => Self::Executable,
            FileKind::Empty => Self::Empty,
            FileKind::Socket => Self::Socket,
            FileKind::Pipe => Self::Pipe,
            FileKind::BlockDevice => Self::BlockDevice,
            FileKind::CharDevice => Self::CharDevice,
        }
    }

    fn written(self) -> FileKind {
        match self {
            Self::File => FileKind::File,
            Self::Directory => FileKind::Directory,
            Self::Symlink => FileKind::Symlink,
            Self::Executable => FileKind::Executable,
            Self::Empty => FileKind::Empty,
            Self::Socket => FileKind::Socket,
            Self::Pipe => FileKind::Pipe,
            Self::BlockDevice => FileKind::BlockDevice,
            Self::CharDevice => FileKind::CharDevice,
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
        search: &protocol::Search,
        directory: &Path,
    ) -> Result<Self, FdError> {
        let now = SystemTime::now();
        let ago = |age_ago: &Age| now.checked_sub(age(age_ago)).unwrap_or(UNIX_EPOCH);
        let depth = |depth: u64| usize::try_from(depth).unwrap_or(usize::MAX);
        // Both bounds of a side must hold, so the later lower bound and the
        // earlier upper bound win.
        let changed_after = [
            match &search.recency {
                Recency::ChangedWithin { age } => Some(ago(age)),
                Recency::ChangedAnyTime => None,
            },
            match &search.start {
                Start::ChangedSince { time } => Some(moment(time)),
                Start::SinceTheBeginning => None,
            },
        ]
        .into_iter()
        .flatten()
        .max();
        let changed_before = [
            match &search.end {
                End::ChangedBefore { time } => Some(moment(time)),
                End::UntilNow => None,
            },
            match &search.oldness {
                Oldness::ChangedMoreThanAgo { age } => Some(ago(age)),
                Oldness::AnyAge => None,
            },
        ]
        .into_iter()
        .flatten()
        .min();
        let mut ignore_files = Vec::new();
        for path in &search.files {
            let full = directory.join(path);
            if !full.exists() {
                return Err(FdError::DoesNotExist { path: path.clone() });
            }
            ignore_files.push(full);
        }
        let syntax = match search.syntax {
            SyntaxChoice::GlobPattern => Syntax::Glob,
            SyntaxChoice::FixedTextPattern => Syntax::Fixed,
            SyntaxChoice::RegexPattern => Syntax::Regex,
        };
        let case = match search.case {
            CaseChoice::ExactCase => Case::Exact,
            CaseChoice::AnyCase => Case::Any,
            CaseChoice::SmartCase => Case::Smart,
        };
        let mut patterns = vec![pattern.to_string()];
        patterns.extend(search.patterns.iter().cloned());
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
            full_path: search.subject == Subject::FullPath,
            hidden: search.hidden == HiddenFiles::Hidden,
            no_ignore: search.ignore == IgnoreFiles::NoIgnore,
            no_ignore_vcs: search.vcs == VersionControlIgnores::NoIgnoreVcs,
            no_ignore_parent: search.parents == ParentIgnores::NoIgnoreParent,
            no_require_git: search.git == GitRepository::NoRequireGit,
            ignore_files,
            excluding: search.globs.clone(),
            follow_links: search.links == Links::FollowLinks,
            one_file_system: search.file_systems == FileSystems::OneFileSystem,
            prune: search.pruning == MatchedDirectories::Prune,
            min_depth: match search.min_depth {
                LeastDepth::MinDepth { depth: least } => Some(depth(least)),
                LeastDepth::FromTheTop => None,
            },
            max_depth: match search.max_depth {
                MostDepth::MaxDepth { depth: most } => Some(depth(most)),
                MostDepth::AnyDepth => None,
            },
            kinds: search.kinds.iter().map(Kind::read).collect(),
            extensions: search
                .extensions
                .iter()
                .map(|extension| extension.trim_start_matches('.').to_lowercase())
                .collect(),
            at_least: match search.least_size {
                LeastSize::AtLeastBytes { size } => Some(size),
                LeastSize::NoMinimumSize => None,
            },
            at_most: match search.most_size {
                MostSize::AtMostBytes { size } => Some(size),
                MostSize::NoMaximumSize => None,
            },
            changed_after,
            changed_before,
            user: match search.user {
                User::OwnedByUser { id } => Some(id),
                User::AnyUser => None,
            },
            group: match search.group {
                Group::OwnedByGroup { id } => Some(id),
                Group::AnyGroup => None,
            },
            limit: match search.limit {
                Limit::AtMostResults { count } => Some(depth(count)),
                Limit::AllResults => None,
            },
        })
    }

    /// What the search finds, in the order of the walk: by name, each
    /// directory before what it holds. `limit` stops it early.
    fn run(&self, limit: Option<usize>) -> Result<Vec<Found>, FdError> {
        let limit = match (limit, self.limit) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let mut found = Vec::new();
        if limit == Some(0) {
            return Ok(found);
        }
        for root in &self.roots {
            let start = match root {
                Some(root) => self.directory.join(root),
                None => self.directory.clone(),
            };
            if !start.exists() {
                return Err(FdError::DoesNotExist {
                    path: root.clone().unwrap_or_else(|| ".".into()),
                });
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
                    overrides
                        .add(&format!("!{glob}"))
                        .map_err(|problem| FdError::InvalidGlob {
                            message: glob_message(&problem),
                        })?;
                }
                walk.overrides(overrides.build().map_err(|problem| FdError::InvalidGlob {
                    message: glob_message(&problem),
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
                    return Err(FdError::IsNotUnicode {
                        path: path.to_string_lossy().into_owned(),
                    });
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
    /// What `details of` says of the file.
    fn details(&self) -> Details {
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
        Details {
            path: self.path.clone(),
            kind: kind.written(),
            size: metadata.as_ref().map_or(0, std::fs::Metadata::len),
            modified: seconds_since_the_epoch(
                metadata
                    .and_then(|metadata| metadata.modified().ok())
                    .unwrap_or(UNIX_EPOCH),
            ),
        }
    }
}
