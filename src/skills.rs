//! Skills system for `OpenClaudia`.
//!
//! Loads user-defined skills from `.openclaudia/skills/` directories.
//! Skills are markdown files with YAML frontmatter that define
//! reusable prompts invokable as slash commands.
//!
//! Skill file format (SKILL.md or <name>.md):
//! ```markdown
//! ---
//! name: my-skill
//! description: Does something useful
//! allowed_tools: [bash, read_file, edit_file]
//! ---
//!
//! You are a specialized agent that...
//! ```
//!
//! ## Caching (crosslink #432)
//!
//! [`load_skills`] used to re-walk the skill directories and re-parse every
//! `SKILL.md` on every system-prompt build, even though the prompt cache is
//! split specifically so that skills can change without invalidating the
//! stable prefix. With N skills and T turns that is O(N*T) filesystem calls
//! and `serde_yaml` parses per session.
//!
//! We now cache the loaded skills in a process-wide [`LazyLock`] +
//! [`RwLock`]. The cache key is the pair of directory mtimes (project +
//! user). On each call we cheaply `stat()` both directories; if neither
//! mtime has changed we return the cached `Vec` without touching any
//! `SKILL.md`. When either mtime changes (a skill was added, removed, or
//! its containing dir was otherwise modified) we re-scan and refresh the
//! cache under a write lock.
//!
//! This still re-scans when files *inside* a skill subdirectory change
//! without bumping the parent dir's mtime — but that's a deliberate
//! correctness/perf trade: mtime of the top-level skills dir is enough
//! to catch add/remove of skills, which is the common edit pattern. Power
//! users editing a `SKILL.md` in place can force a reload via
//! [`invalidate_cache`].

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};
use std::time::SystemTime;
use thiserror::Error;

/// Structured failure modes for [`parse_skill_file`].
///
/// Returning a typed error (instead of `Option`) lets call sites discriminate
/// between "this isn't a skill file" (`FrontmatterMissing`) and real corruption
/// (`YamlFailed`, `ReadFailed`). The public scan path in [`load_skills`]
/// converts this back to `Option` and logs each failure with full context via
/// `tracing::warn!` so users can diagnose silently-dropped skills (crosslink
/// #441 / #432).
#[derive(Debug, Error)]
pub enum SkillParseError {
    /// The skill file could not be read from disk (missing, permission denied, etc).
    #[error("failed to read skill file: {0}")]
    ReadFailed(#[from] std::io::Error),
    /// The file did not begin with the YAML frontmatter `---` delimiter,
    /// or the closing `---` was missing. The file is silently treated as
    /// "not a skill" — every plain `.md` in a skills dir hits this path.
    #[error("skill file has no YAML frontmatter (`---` delimiters)")]
    FrontmatterMissing,
    /// The frontmatter delimiters were present but the contents failed to
    /// deserialize into a [`SkillDefinition`].
    #[error("failed to parse skill frontmatter as YAML: {0}")]
    YamlFailed(#[from] serde_yaml::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    /// The prompt content (markdown body after frontmatter)
    #[serde(skip)]
    pub prompt: String,
    /// Path to the skill file
    #[serde(skip)]
    pub path: PathBuf,
}

/// Cache key: the mtime of each scanned directory, in scan order.
///
/// `None` means the directory did not exist at the last scan; the cache
/// is invalidated when an absent directory appears (or vice versa).
type DirMtimes = Vec<(PathBuf, Option<SystemTime>)>;

struct SkillsCache {
    mtimes: DirMtimes,
    skills: Vec<SkillDefinition>,
}

static SKILLS_CACHE: LazyLock<RwLock<Option<SkillsCache>>> = LazyLock::new(|| RwLock::new(None));

/// Walk upward from `start` looking for the project root — the nearest
/// ancestor that contains `.openclaudia/config.yaml`. Returns `None` when no
/// such ancestor exists, in which case the project-skills dir is skipped
/// entirely rather than silently picking up `.openclaudia/skills/` from
/// whatever directory the process happens to be running in (crosslink #823).
fn find_project_root(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if ancestor.join(".openclaudia").join("config.yaml").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// Return the candidate skill directories in priority order.
///
/// Project directory comes first so its skills win on name collision. The
/// project directory is resolved against an explicit project root (the
/// nearest ancestor containing `.openclaudia/config.yaml`) and an absolute
/// `PathBuf` is pushed. If no project root can be located the project-skills
/// entry is omitted entirely (crosslink #823): the previous relative
/// `PathBuf::from(".openclaudia/skills")` would otherwise silently pick up
/// whatever directory the process happened to be in at scan time, and the
/// loaded skills are injected straight into the model context.
fn skill_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::with_capacity(2);
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(root) = find_project_root(&cwd) {
            let project_skills = root.join(".openclaudia").join("skills");
            tracing::info!(
                path = %project_skills.display(),
                "Project skills dir resolved (absolute)"
            );
            dirs.push(project_skills);
        } else {
            tracing::debug!(
                cwd = %cwd.display(),
                "No .openclaudia/config.yaml ancestor found; skipping project skills dir"
            );
        }
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".openclaudia/skills"));
    }
    dirs
}

/// Read the current mtime fingerprint for the given directories.
///
/// A directory that does not exist contributes `None`, which is distinct
/// from any `SystemTime` and so will invalidate the cache when the
/// directory is later created (or removed).
fn current_mtimes(dirs: &[PathBuf]) -> DirMtimes {
    dirs.iter()
        .map(|d| {
            let mtime = std::fs::metadata(d).and_then(|m| m.modified()).ok();
            (d.clone(), mtime)
        })
        .collect()
}

/// Parse a skill file (YAML frontmatter + markdown body).
///
/// Returns [`SkillParseError`] for files without `---` frontmatter, files we
/// cannot read, or files whose frontmatter fails to parse as a
/// [`SkillDefinition`]. Call sites in [`load_skills`] convert the error back
/// to `Option`, logging each failure at `WARN` via `tracing` so users can
/// diagnose silently-dropped skills (crosslink #441 / #432).
///
/// Normalizes two common editor artifacts before parsing:
/// * **UTF-8 BOM** (`U+FEFF`) at the very start of the file — Windows editors
///   like Notepad emit this; without stripping it the frontmatter check
///   (`starts_with("---")`) fails and the skill is silently dropped.
/// * **CRLF line endings** — `serde_yaml` accepts CRLF, but our manual
///   delimiter search would treat `\r---` differently from `---`, so we
///   normalize to `\n` first for stable behavior across platforms.
///
/// # Errors
///
/// Returns [`SkillParseError::ReadFailed`] if the file cannot be read,
/// [`SkillParseError::FrontmatterMissing`] if the leading or trailing `---`
/// is absent, or [`SkillParseError::YamlFailed`] if the frontmatter is not
/// valid YAML for a [`SkillDefinition`].
pub fn parse_skill_file(path: &Path) -> Result<SkillDefinition, SkillParseError> {
    let raw = std::fs::read_to_string(path)?;

    // Strip UTF-8 BOM (Windows editors emit this) and normalize CRLF → LF
    // before any delimiter inspection. Both are no-ops for already-clean
    // files, so well-formed Unix UTF-8 skills are unaffected.
    let stripped = raw.trim_start_matches('\u{FEFF}');
    let content: String = if stripped.contains('\r') {
        stripped.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        stripped.to_string()
    };

    // Split frontmatter from body
    if !content.starts_with("---") {
        return Err(SkillParseError::FrontmatterMissing);
    }

    let rest = &content[3..];
    let end = rest
        .find("---")
        .ok_or(SkillParseError::FrontmatterMissing)?;
    let frontmatter = rest[..end].trim();
    let body = rest[end + 3..].trim();

    let mut skill: SkillDefinition = serde_yaml::from_str(frontmatter)?;
    skill.prompt = body.to_string();
    skill.path = path.to_path_buf();

    Ok(skill)
}

/// Adapter that converts a [`parse_skill_file`] error into an `Option`,
/// logging the failure with full structured context. Keeps the scan loop
/// in [`scan_one_dir`] terse while preserving the per-file `tracing::warn!`
/// behavior the previous `Option`-returning API had.
fn parse_skill_file_logged(path: &Path) -> Option<SkillDefinition> {
    match parse_skill_file(path) {
        Ok(skill) => Some(skill),
        // Files without frontmatter are *expected* — every README.md or
        // notes.md in a skills dir hits this path. Log at TRACE rather
        // than WARN so it doesn't pollute the user's stderr.
        Err(SkillParseError::FrontmatterMissing) => {
            tracing::trace!(
                skill_path = %path.display(),
                "skipping file without YAML frontmatter"
            );
            None
        }
        Err(err) => {
            tracing::warn!(
                skill_path = %path.display(),
                error = %err,
                "failed to load skill; file will be ignored"
            );
            None
        }
    }
}

/// Either a subdirectory containing a `SKILL.md` (the canonical packaged-skill
/// layout) or a single `.md` file at the top of a skills directory.
///
/// Returned by [`walk_skill_entries`] — the SINGLE shared walker that
/// both `skills::load_skills` and `plugins::Plugin::resolve_skills`
/// route through (crosslink #832). The two previously walked the same
/// kind of directory with subtly different rules; this enum is the
/// chokepoint that prevents future drift.
#[derive(Debug, Clone)]
pub enum SkillEntry {
    /// `<dir>/SKILL.md` exists. `dir` is the subdirectory path, `file`
    /// the resolved `SKILL.md` inside it.
    DirWithSkillMd { dir: PathBuf, file: PathBuf },
    /// A `.md` file directly inside the skills directory.
    BareMdFile(PathBuf),
}

impl SkillEntry {
    /// Return the directory or file that callers should treat as the
    /// skill's "root" for permission-checking / path-recording.
    #[must_use]
    pub fn root_path(&self) -> &Path {
        match self {
            Self::DirWithSkillMd { dir, .. } => dir.as_path(),
            Self::BareMdFile(p) => p.as_path(),
        }
    }
}

/// Walk a single skills directory and emit one [`SkillEntry`] per
/// candidate.
///
/// Silently returns an empty `Vec` if `dir` is not readable (the
/// walker is best-effort; callers that need the failure must stat
/// `dir` themselves first).
///
/// Crosslink #832: extracted from `skills::scan_one_dir` and
/// `plugins::Plugin::resolve_skills` so the two stay in sync. Adding a
/// new packaging convention (e.g. `<dir>/skill.yaml`) is now one edit
/// here, not two.
#[must_use]
pub fn walk_skill_entries(dir: &Path) -> Vec<SkillEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let skill_file = path.join("SKILL.md");
            if skill_file.exists() {
                out.push(SkillEntry::DirWithSkillMd {
                    dir: path,
                    file: skill_file,
                });
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(SkillEntry::BareMdFile(path));
        }
    }
    out
}

/// Scan a single directory for skill definitions, appending into `out`.
fn scan_one_dir(dir: &Path, out: &mut Vec<SkillDefinition>) {
    for entry in walk_skill_entries(dir) {
        match entry {
            SkillEntry::DirWithSkillMd { dir, file } => {
                if let Some(mut skill) = parse_skill_file_logged(&file) {
                    if skill.name.is_empty() {
                        skill.name = dir
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                    }
                    out.push(skill);
                }
            }
            SkillEntry::BareMdFile(path) => {
                if let Some(mut skill) = parse_skill_file_logged(&path) {
                    if skill.name.is_empty() {
                        skill.name = path
                            .file_stem()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                    }
                    out.push(skill);
                }
            }
        }
    }
}

/// Walk the skill directories and load every skill from scratch.
///
/// Project skills (`.openclaudia/skills/`) take priority over user skills
/// (`~/.openclaudia/skills/`) on name collision.
fn load_skills_uncached(dirs: &[PathBuf]) -> Vec<SkillDefinition> {
    let mut skills = Vec::new();
    for dir in dirs {
        if dir.exists() {
            scan_one_dir(dir, &mut skills);
        }
    }

    // Deduplicate by name (project skills take priority over user skills)
    let mut seen = std::collections::HashSet::new();
    skills.retain(|s| seen.insert(s.name.clone()));

    skills
}

/// Scan directories for skill files, with mtime-based caching.
///
/// The cache is keyed on the mtime of each scanned skills directory. If
/// neither directory has changed since the last scan, the cached vector is
/// cloned and returned without touching the filesystem. When a directory's
/// mtime changes (or it appears/disappears), the cache is refreshed under a
/// write lock. See the module-level docs for the trade-offs.
#[must_use]
pub fn load_skills() -> Vec<SkillDefinition> {
    let dirs = skill_dirs();
    let mtimes_now = current_mtimes(&dirs);

    // Fast path: read lock, cache hit.
    if let Ok(guard) = SKILLS_CACHE.read() {
        if let Some(cache) = guard.as_ref() {
            if cache.mtimes == mtimes_now {
                return cache.skills.clone();
            }
        }
    }

    // Slow path: rescan under the write lock. Re-check inside the write
    // lock to avoid a thundering-herd of refreshes if many callers raced.
    let mut guard = match SKILLS_CACHE.write() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(cache) = guard.as_ref() {
        if cache.mtimes == mtimes_now {
            return cache.skills.clone();
        }
    }
    let skills = load_skills_uncached(&dirs);
    *guard = Some(SkillsCache {
        mtimes: mtimes_now,
        skills: skills.clone(),
    });
    skills
}

/// Force the skills cache to be discarded on the next [`load_skills`] call.
///
/// Useful for tests and for editor watchers that detect in-place edits to
/// a `SKILL.md` without changing the parent directory's mtime.
pub fn invalidate_cache() {
    if let Ok(mut guard) = SKILLS_CACHE.write() {
        *guard = None;
    }
}

/// Get a skill by name
#[must_use]
pub fn get_skill(name: &str) -> Option<SkillDefinition> {
    load_skills().into_iter().find(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_file() {
        let content =
            "---\nname: test-skill\ndescription: A test skill\n---\n\nYou are a test agent.";
        let tmp = std::env::temp_dir().join("test_skill.md");
        std::fs::write(&tmp, content).unwrap();

        let skill = parse_skill_file(&tmp).unwrap();
        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.description, "A test skill");
        assert_eq!(skill.prompt, "You are a test agent.");

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_parse_skill_no_frontmatter() {
        let tmp = std::env::temp_dir().join("no_fm.md");
        std::fs::write(&tmp, "Just plain text").unwrap();
        assert!(matches!(
            parse_skill_file(&tmp),
            Err(SkillParseError::FrontmatterMissing)
        ));
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_parse_skill_with_tools() {
        let content = "---\nname: coder\ndescription: Codes stuff\nallowed_tools:\n  - bash\n  - edit_file\n---\n\nWrite code.";
        let tmp = std::env::temp_dir().join("tools_skill.md");
        std::fs::write(&tmp, content).unwrap();

        let skill = parse_skill_file(&tmp).unwrap();
        assert_eq!(
            skill.allowed_tools,
            Some(vec!["bash".to_string(), "edit_file".to_string()])
        );

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_load_skills_empty() {
        // Should not panic even if dirs don't exist
        let _skills = load_skills();
    }

    // ── #432: cache + mtime invalidation + YAML error logging ───────────────

    /// Parse failure on bad YAML returns a `YamlFailed` error (the warn-log
    /// side effect is observed via cargo test stderr; this asserts the
    /// user-visible behavior).
    #[test]
    fn parse_skill_file_returns_err_on_bad_yaml() {
        let tmp = std::env::temp_dir().join("openclaudia_bad_yaml_skill.md");
        // Frontmatter is structurally present (--- ... ---) but the YAML body
        // is invalid (unclosed bracket, no required fields).
        std::fs::write(&tmp, "---\nname: [unterminated\n---\n\nbody").unwrap();
        let err = parse_skill_file(&tmp).expect_err("bad YAML must yield Err");
        assert!(
            matches!(err, SkillParseError::YamlFailed(_)),
            "expected YamlFailed, got {err:?}"
        );
        std::fs::remove_file(&tmp).ok();
    }

    /// `load_skills_uncached` honors project-over-user precedence on collision.
    #[test]
    fn load_skills_uncached_dedupes_project_first() {
        let root = tempfile::tempdir().unwrap();
        let proj = root.path().join("project");
        let user = root.path().join("user");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(&user).unwrap();

        std::fs::write(
            proj.join("dup.md"),
            "---\nname: dup\ndescription: from project\n---\nproject body",
        )
        .unwrap();
        std::fs::write(
            user.join("dup.md"),
            "---\nname: dup\ndescription: from user\n---\nuser body",
        )
        .unwrap();
        std::fs::write(
            user.join("solo.md"),
            "---\nname: solo\ndescription: only in user\n---\nsolo body",
        )
        .unwrap();

        let dirs = vec![proj, user];
        let skills = load_skills_uncached(&dirs);

        let dup = skills
            .iter()
            .find(|s| s.name == "dup")
            .expect("dup present");
        assert_eq!(
            dup.description, "from project",
            "project skill must win on name collision"
        );
        assert!(
            skills.iter().any(|s| s.name == "solo"),
            "user-only skill must still load"
        );
    }

    /// The cache key changes when a scanned directory's mtime changes — proving
    /// that cache lookups will miss after a real edit and pick up the new state.
    #[test]
    fn current_mtimes_changes_when_dir_mtime_changes() {
        let root = tempfile::tempdir().unwrap();
        let d = root.path().join("skills");
        std::fs::create_dir_all(&d).unwrap();
        let dirs = vec![d.clone()];

        let m1 = current_mtimes(&dirs);
        // Mutate the directory (adding a file bumps the parent's mtime on
        // every mainstream filesystem we care about). Sleep a beat so the
        // filesystem timestamp granularity (1s on some FSes) actually moves.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(d.join("new.md"), "x").unwrap();

        let m2 = current_mtimes(&dirs);
        assert_ne!(
            m1, m2,
            "adding a file must change the dir mtime fingerprint"
        );
    }

    // ── #441: BOM + CRLF normalization + structured errors ─────────────────

    /// A UTF-8 BOM (`U+FEFF`) at the start of the file must not prevent the
    /// `---` frontmatter from being detected. Windows editors like Notepad
    /// emit a BOM by default; pre-#441 we silently dropped these skills.
    #[test]
    fn parse_skill_file_strips_utf8_bom() {
        let bom = "\u{FEFF}";
        let body = "---\nname: bom-skill\ndescription: BOM prefixed\n---\n\nBOM body.";
        let content = format!("{bom}{body}");
        let tmp = std::env::temp_dir().join("openclaudia_bom_skill.md");
        std::fs::write(&tmp, content).unwrap();

        let skill = parse_skill_file(&tmp).expect("BOM-prefixed file must parse");
        assert_eq!(skill.name, "bom-skill");
        assert_eq!(skill.description, "BOM prefixed");
        assert_eq!(skill.prompt, "BOM body.");
        std::fs::remove_file(&tmp).ok();
    }

    /// Windows-style CRLF line endings around the frontmatter delimiters
    /// must parse identically to LF-only input. Pre-#441 the embedded `\r`
    /// in `\r---\r` confused the manual delimiter search.
    #[test]
    fn parse_skill_file_normalizes_crlf() {
        let content =
            "---\r\nname: crlf-skill\r\ndescription: CRLF endings\r\n---\r\n\r\nCRLF body.\r\n";
        let tmp = std::env::temp_dir().join("openclaudia_crlf_skill.md");
        std::fs::write(&tmp, content).unwrap();

        let skill = parse_skill_file(&tmp).expect("CRLF file must parse");
        assert_eq!(skill.name, "crlf-skill");
        assert_eq!(skill.description, "CRLF endings");
        assert_eq!(
            skill.prompt, "CRLF body.",
            "body must be CRLF-normalized and trimmed"
        );
        std::fs::remove_file(&tmp).ok();
    }

    /// Combined: BOM + CRLF (the most common "Windows Notepad" trifecta)
    /// must parse cleanly without producing a `FrontmatterMissing` error.
    #[test]
    fn parse_skill_file_handles_bom_and_crlf_together() {
        let content =
            "\u{FEFF}---\r\nname: win-skill\r\ndescription: Windows-style\r\n---\r\n\r\nbody";
        let tmp = std::env::temp_dir().join("openclaudia_bom_crlf_skill.md");
        std::fs::write(&tmp, content).unwrap();

        let skill = parse_skill_file(&tmp).expect("BOM+CRLF file must parse");
        assert_eq!(skill.name, "win-skill");
        std::fs::remove_file(&tmp).ok();
    }

    /// The logged adapter must convert a `YamlFailed` into `None` and let
    /// `scan_one_dir` continue (this is the `load_skills` path's contract).
    #[test]
    fn parse_skill_file_logged_returns_none_on_bad_yaml() {
        let tmp = std::env::temp_dir().join("openclaudia_logged_bad_yaml.md");
        std::fs::write(&tmp, "---\nname: [bad\n---\n\nbody").unwrap();
        assert!(
            parse_skill_file_logged(&tmp).is_none(),
            "logged adapter must convert YamlFailed → None for scan_one_dir"
        );
        std::fs::remove_file(&tmp).ok();
    }

    /// A nonexistent path surfaces as `ReadFailed`, not a panic.
    #[test]
    fn parse_skill_file_missing_path_is_read_failed() {
        let tmp = std::env::temp_dir().join("openclaudia_definitely_not_present_skill.md");
        std::fs::remove_file(&tmp).ok();
        let err = parse_skill_file(&tmp).expect_err("missing file must yield Err");
        assert!(
            matches!(err, SkillParseError::ReadFailed(_)),
            "expected ReadFailed, got {err:?}"
        );
    }

    /// End-to-end: two back-to-back [`load_skills`] calls return equal data,
    /// and the cache holds a `SkillsCache` afterwards (proving we populated it).
    #[test]
    fn load_skills_populates_and_reuses_cache() {
        // Force a refresh so any earlier test's state does not interfere.
        invalidate_cache();
        let first = load_skills();
        let second = load_skills();
        assert_eq!(
            first.len(),
            second.len(),
            "cached and uncached calls must agree on count"
        );
        // Scope the read guard tightly so clippy::significant_drop_tightening
        // is happy; we only need it long enough to inspect `is_some`.
        let populated = {
            let guard = SKILLS_CACHE.read().unwrap();
            guard.is_some()
        };
        assert!(
            populated,
            "load_skills must populate the cache on first call"
        );
    }
}
