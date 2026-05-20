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

/// Return the candidate skill directories in priority order.
///
/// Project directory comes first so its skills win on name collision.
fn skill_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from(".openclaudia/skills")];
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
/// Returns `None` for files without `---` frontmatter, files we cannot read,
/// or files whose frontmatter fails to parse as a [`SkillDefinition`]. Parse
/// failures and IO errors are logged at `WARN` via `tracing` so the user can
/// diagnose silently-dropped skills (crosslink #432).
#[must_use]
pub fn parse_skill_file(path: &Path) -> Option<SkillDefinition> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                skill_path = %path.display(),
                error = %e,
                "failed to read skill file"
            );
            return None;
        }
    };

    // Split frontmatter from body
    if !content.starts_with("---") {
        return None;
    }

    let rest = &content[3..];
    let end = rest.find("---")?;
    let frontmatter = rest[..end].trim();
    let body = rest[end + 3..].trim();

    let mut skill: SkillDefinition = match serde_yaml::from_str(frontmatter) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                skill_path = %path.display(),
                error = %e,
                "failed to parse skill frontmatter as YAML; skill will not load"
            );
            return None;
        }
    };
    skill.prompt = body.to_string();
    skill.path = path.to_path_buf();

    Some(skill)
}

/// Scan a single directory for skill definitions, appending into `out`.
fn scan_one_dir(dir: &Path, out: &mut Vec<SkillDefinition>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            // Look for SKILL.md inside subdirectory
            let skill_file = path.join("SKILL.md");
            if skill_file.exists() {
                if let Some(mut skill) = parse_skill_file(&skill_file) {
                    if skill.name.is_empty() {
                        skill.name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                    }
                    out.push(skill);
                }
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            // Direct .md file in skills dir
            if let Some(mut skill) = parse_skill_file(&path) {
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
        assert!(parse_skill_file(&tmp).is_none());
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

    /// Parse failure on bad YAML returns `None` (the warn-log side effect is
    /// observed via cargo test stderr; this asserts the user-visible behavior).
    #[test]
    fn parse_skill_file_returns_none_on_bad_yaml() {
        let tmp = std::env::temp_dir().join("openclaudia_bad_yaml_skill.md");
        // Frontmatter is structurally present (--- ... ---) but the YAML body
        // is invalid (unclosed bracket, no required fields).
        std::fs::write(&tmp, "---\nname: [unterminated\n---\n\nbody").unwrap();
        assert!(parse_skill_file(&tmp).is_none(), "bad YAML must yield None");
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
