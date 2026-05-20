//! Plugin manager for discovery, loading, and lifecycle management.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use super::git::{copy_dir_recursive_within, git_clone, git_pull};
use super::install::{InstallScope, InstalledPlugins, PluginInstallEntry};
use super::marketplace::{
    GitHubSource, MarketplaceManifest, MarketplacePlugin, MarketplaceSource, PluginSource,
    PluginSourceDef, UrlSource,
};
use super::policy::{self, PluginPolicy, PolicyAction, PolicyRejection};
use super::validate::{verify_signature, SignatureError};
use super::{Plugin, PluginCommand, PluginError, PluginHook, PluginMcpServer};

/// Resolve the project root that owns per-project tracking state
/// (`<project_root>/.openclaudia/plugins/installed_plugins.json`).
///
/// Falls back to the current process cwd as a best-effort root; if even
/// `current_dir()` fails (deleted cwd, etc.) we use `"."`, which the
/// caller's atomic-save path will canonicalize via `create_dir_all`.
/// This matches the value the install entries themselves already record
/// in `project_path` (see [`PluginInstallEntry::project_path`]).
fn project_root_cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Manages plugin discovery, loading, and lifecycle
pub struct PluginManager {
    /// Loaded plugins by name
    plugins: HashMap<String, Plugin>,
    /// Search paths for plugins
    search_paths: Vec<PathBuf>,
    /// Installation tracking
    installed: InstalledPlugins,
}

impl PluginManager {
    /// Create a new plugin manager with default search paths
    #[must_use]
    pub fn new() -> Self {
        let mut search_paths = Vec::new();

        // User plugins directory
        if let Some(home) = dirs::home_dir() {
            search_paths.push(home.join(".openclaudia").join("plugins"));
            // Also search Claude Code's plugin cache for compatibility
            search_paths.push(home.join(".claude").join("plugins"));
        }

        // Project plugins directory
        search_paths.push(PathBuf::from(".openclaudia/plugins"));

        Self {
            plugins: HashMap::new(),
            search_paths,
            installed: InstalledPlugins::load(&project_root_cwd()),
        }
    }

    /// Create a plugin manager with custom search paths
    #[must_use]
    pub fn with_paths(paths: Vec<PathBuf>) -> Self {
        Self {
            plugins: HashMap::new(),
            search_paths: paths,
            installed: InstalledPlugins::default(),
        }
    }

    /// Discover and load all plugins from search paths and `installed_plugins.json`
    pub fn discover(&mut self) -> Vec<PluginError> {
        let mut errors = Vec::new();

        // Load from search paths (convention-based discovery)
        for search_path in &self.search_paths.clone() {
            if !search_path.exists() {
                debug!(path = ?search_path, "Plugin search path does not exist");
                continue;
            }

            let entries = match fs::read_dir(search_path) {
                Ok(entries) => entries,
                Err(e) => {
                    warn!(path = ?search_path, error = %e, "Failed to read plugin directory");
                    continue;
                }
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    match Plugin::load(&path) {
                        Ok(plugin) => {
                            info!(
                                name = %plugin.name(),
                                version = ?plugin.manifest.version,
                                path = ?path,
                                commands = plugin.command_paths.len(),
                                hooks = plugin.hook_definitions.len(),
                                mcp = plugin.mcp_configs.len(),
                                "Loaded plugin"
                            );
                            self.plugins.insert(plugin.name().to_string(), plugin);
                        }
                        Err(PluginError::ManifestNotFound(_)) => {
                            // Not a plugin directory, skip silently
                            debug!(path = ?path, "Directory has no plugin manifest, skipping");
                        }
                        Err(e) => {
                            warn!(path = ?path, error = %e, "Failed to load plugin");
                            errors.push(e);
                        }
                    }
                }
            }
        }

        // Load from installed_plugins.json (tracked installations)
        for (plugin_id, entries) in &self.installed.plugins {
            for entry in entries {
                let install_path = PathBuf::from(&entry.install_path);
                if !install_path.exists() {
                    debug!(plugin = %plugin_id, path = ?install_path, "Installed plugin path missing");
                    continue;
                }
                // Skip if already loaded from search paths
                let name = plugin_id.split('@').next().unwrap_or(plugin_id);
                if self.plugins.contains_key(name) {
                    continue;
                }
                match Plugin::load(&install_path) {
                    Ok(mut plugin) => {
                        plugin.id.clone_from(plugin_id);
                        if let Some(marketplace) = plugin_id.split('@').nth(1) {
                            plugin.source = marketplace.to_string();
                        }
                        info!(
                            id = %plugin_id,
                            name = %plugin.name(),
                            scope = %entry.scope,
                            "Loaded installed plugin"
                        );
                        self.plugins.insert(plugin.name().to_string(), plugin);
                    }
                    Err(e) => {
                        warn!(plugin = %plugin_id, error = %e, "Failed to load installed plugin");
                        errors.push(e);
                    }
                }
            }
        }

        errors
    }

    /// Get a plugin by name
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Plugin> {
        self.plugins.get(name)
    }

    /// Get all loaded plugins
    pub fn all(&self) -> impl Iterator<Item = &Plugin> {
        self.plugins.values()
    }

    /// Get the number of loaded plugins
    #[must_use]
    pub fn count(&self) -> usize {
        self.plugins.len()
    }

    /// Get all hooks from all enabled plugins
    #[must_use]
    pub fn all_hooks(&self) -> Vec<(&Plugin, PluginHook)> {
        self.plugins
            .values()
            .filter(|p| p.enabled)
            .flat_map(|plugin| {
                plugin
                    .resolved_hooks()
                    .into_iter()
                    .map(move |hook| (plugin, hook))
            })
            .collect()
    }

    /// Get hooks for a specific event
    #[must_use]
    pub fn hooks_for_event(&self, event: &str) -> Vec<(&Plugin, PluginHook)> {
        self.all_hooks()
            .into_iter()
            .filter(|(_, hook)| hook.event == event)
            .collect()
    }

    /// Get all commands from all enabled plugins
    #[must_use]
    pub fn all_commands(&self) -> Vec<(&Plugin, PluginCommand)> {
        self.plugins
            .values()
            .filter(|p| p.enabled)
            .flat_map(|plugin| {
                plugin
                    .resolved_commands()
                    .into_iter()
                    .map(move |cmd| (plugin, cmd))
            })
            .collect()
    }

    /// Get all MCP servers from all enabled plugins
    #[must_use]
    pub fn all_mcp_servers(&self) -> Vec<(&Plugin, PluginMcpServer)> {
        self.plugins
            .values()
            .filter(|p| p.enabled)
            .flat_map(|plugin| {
                plugin
                    .resolved_mcp_servers()
                    .into_iter()
                    .map(move |server| (plugin, server))
            })
            .collect()
    }

    /// Get the installation tracker
    #[must_use]
    pub const fn installed(&self) -> &InstalledPlugins {
        &self.installed
    }

    /// Get mutable installation tracker
    pub const fn installed_mut(&mut self) -> &mut InstalledPlugins {
        &mut self.installed
    }

    /// Enable a plugin
    ///
    /// # Errors
    ///
    /// Returns `PluginError::NotFound` if no plugin with the given name is loaded.
    pub fn enable(&mut self, name: &str) -> Result<(), PluginError> {
        if let Some(plugin) = self.plugins.get_mut(name) {
            plugin.enabled = true;
            Ok(())
        } else {
            Err(PluginError::NotFound(name.to_string()))
        }
    }

    /// Disable a plugin
    ///
    /// # Errors
    ///
    /// Returns `PluginError::NotFound` if no plugin with the given name is loaded.
    pub fn disable(&mut self, name: &str) -> Result<(), PluginError> {
        if let Some(plugin) = self.plugins.get_mut(name) {
            plugin.enabled = false;
            Ok(())
        } else {
            Err(PluginError::NotFound(name.to_string()))
        }
    }

    /// Reload all plugins
    pub fn reload(&mut self) -> Vec<PluginError> {
        self.plugins.clear();
        self.installed = InstalledPlugins::load(&project_root_cwd());
        self.discover()
    }

    /// Get the marketplaces directory (~/.claude/marketplaces/)
    fn marketplaces_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".claude")
            .join("marketplaces")
    }

    /// List installed marketplaces
    #[must_use]
    pub fn list_marketplaces(&self) -> Vec<(String, MarketplaceManifest)> {
        let dir = Self::marketplaces_dir();
        let mut marketplaces = Vec::new();
        if !dir.exists() {
            return marketplaces;
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                // Try loading marketplace manifest
                let manifest_path = path.join(".claude-plugin").join("marketplace.json");
                let alt_manifest_path = path.join("marketplace.json");
                let mp = if manifest_path.exists() {
                    &manifest_path
                } else if alt_manifest_path.exists() {
                    &alt_manifest_path
                } else {
                    continue;
                };
                if let Ok(content) = fs::read_to_string(mp) {
                    if let Ok(manifest) = serde_json::from_str::<MarketplaceManifest>(&content) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        marketplaces.push((name, manifest));
                    }
                }
            }
        }
        marketplaces
    }

    /// Enforce all [`PolicyAction`]s that bear on signature verification for
    /// `plugin_name`. Reads raw manifest bytes from `manifest_json` (already
    /// loaded from the marketplace source) and applies every
    /// `RequireSignature` action in the policy.
    ///
    /// # Errors
    ///
    /// - [`PluginError::UnsignedPlugin`] — policy requires a signature but
    ///   `manifest_sig` is `None`.
    /// - [`PluginError::UnknownSigner`] — signature present but no trusted
    ///   key accepted it.
    /// - [`PluginError::SignatureMismatch`] — signature bytes are
    ///   cryptographically invalid over the supplied bytes.
    fn enforce_signature_policy(
        plugin_name: &str,
        manifest_bytes: &[u8],
        manifest_sig: Option<&crate::plugins::validate::PluginSignature>,
        policy: &PluginPolicy,
    ) -> Result<(), PluginError> {
        for action in &policy.actions {
            let PolicyAction::RequireSignature { trusted_keys } = action;
            let sig =
                manifest_sig.ok_or_else(|| PluginError::UnsignedPlugin(plugin_name.to_string()))?;
            match verify_signature(manifest_bytes, sig, trusted_keys) {
                Ok(()) => {}
                Err(SignatureError::UnknownSigner | SignatureError::MalformedKey(_)) => {
                    return Err(PluginError::UnknownSigner(plugin_name.to_string()));
                }
                Err(
                    SignatureError::SignatureMismatch
                    | SignatureError::MissingSignature
                    | SignatureError::InvalidLength(_)
                    | SignatureError::InvalidEncoding(_),
                ) => {
                    return Err(PluginError::SignatureMismatch(plugin_name.to_string()));
                }
            }
        }
        Ok(())
    }

    /// Build a [`MarketplaceSource`] from a per-plugin [`PluginSourceDef`],
    /// suitable for re-running [`policy::check_marketplace_allowed`] against
    /// the upstream URL the plugin will actually pull from. Returns `None`
    /// for source variants (`npm` / `pip`) that don't carry a git/HTTP URL
    /// the marketplace policy can enforce against — those are rejected later
    /// by the base installer's match arm.
    ///
    /// Closes crosslink #729: the per-plugin source URL is now policy-checked
    /// before any `git_clone` / `fs::copy` fires.
    fn plugin_source_to_marketplace_source(def: &PluginSourceDef) -> Option<MarketplaceSource> {
        match def {
            PluginSourceDef::Url(UrlSource { url, git_ref }) => Some(MarketplaceSource::Git {
                url: url.clone(),
                git_ref: git_ref.clone(),
                path: None,
            }),
            PluginSourceDef::GitHub(GitHubSource { repo, git_ref }) => {
                Some(MarketplaceSource::GitHub {
                    repo: repo.clone(),
                    git_ref: git_ref.clone(),
                    path: None,
                })
            }
            // npm / pip carry no git URL — base installer rejects them with
            // InvalidManifest. Returning None here means the policy gate
            // doesn't fire and the existing rejection path handles them.
            PluginSourceDef::Npm(_) | PluginSourceDef::Pip(_) => None,
        }
    }

    /// Apply the marketplace-policy gate to a per-plugin source. Pure
    /// function — extracted so unit tests can drive the #729 gate
    /// without standing up a real marketplace on disk. Returns `Ok(())`
    /// when the source is permitted (or when it has no upstream URL to
    /// check), and [`PluginError::PolicyRejected`] when the policy
    /// rejects the upstream.
    ///
    /// # Errors
    ///
    /// [`PluginError::PolicyRejected`] when
    /// [`policy::check_marketplace_allowed`] rejects the rebuilt source.
    fn check_plugin_source_policy(
        source: &PluginSource,
        policy: &PluginPolicy,
    ) -> Result<(), PluginError> {
        let PluginSource::Structured(def) = source else {
            return Ok(());
        };
        let Some(per_plugin_source) = Self::plugin_source_to_marketplace_source(def) else {
            return Ok(());
        };
        match policy::check_marketplace_allowed(&per_plugin_source, policy) {
            Ok(()) => Ok(()),
            Err(rejection) => Err(Self::policy_rejection_to_error(rejection, policy)),
        }
    }

    /// Install a plugin from a marketplace, enforcing all [`PolicyAction`]s
    /// including signature verification AND re-validating the per-plugin
    /// upstream source URL against `policy.strict_known_marketplaces` /
    /// `policy.blocked_marketplaces` (crosslink #729).
    ///
    /// Without this re-validation, an allowlisted marketplace could ship a
    /// `marketplace.json` whose plugin entries point at arbitrary upstream
    /// URLs — silently downgrading the managed policy to advisory. This
    /// method closes that gap by rebuilding a [`MarketplaceSource`] from
    /// the resolved plugin's [`PluginSourceDef`] and running
    /// [`policy::check_marketplace_allowed`] against it BEFORE any
    /// `git_clone` / `fs::copy` side effects.
    ///
    /// # Errors
    ///
    /// - [`PluginError::PolicyRejected`] when the per-plugin upstream source
    ///   URL is on the blocklist or not in the strict allowlist.
    /// - [`PluginError::UnsignedPlugin`] when policy requires a signature and
    ///   the manifest has none.
    /// - [`PluginError::UnknownSigner`] when the signature does not match any
    ///   trusted key.
    /// - [`PluginError::SignatureMismatch`] when the signature is
    ///   cryptographically invalid.
    /// - All errors from [`Self::install_from_marketplace`].
    pub fn install_from_marketplace_with_policy(
        &mut self,
        plugin_name: &str,
        marketplace_name: &str,
        policy: &PluginPolicy,
    ) -> Result<String, PluginError> {
        // Per-plugin upstream URL policy check (crosslink #729). The
        // marketplace itself was gated at `add_marketplace_*_with_policy`
        // time, but the plugin entries inside it can name arbitrary upstream
        // URLs that the policy never saw. Re-validate here BEFORE any
        // filesystem side effects so a managed policy is actually enforcing.
        {
            let marketplaces = self.list_marketplaces();
            let (_name, mp_manifest) = marketplaces
                .iter()
                .find(|(n, _)| n == marketplace_name)
                .ok_or_else(|| {
                    PluginError::NotFound(format!("Marketplace '{marketplace_name}' not found"))
                })?;
            let mp_plugin = mp_manifest
                .plugins
                .iter()
                .find(|p| p.name == plugin_name)
                .ok_or_else(|| {
                    PluginError::NotFound(format!(
                        "Plugin '{plugin_name}' not found in marketplace '{marketplace_name}'"
                    ))
                })?;
            // `PluginSource::Path` is local to the (already-policy-checked)
            // marketplace directory — no separate upstream URL to validate.
            // The helper returns Ok(()) for Path / npm / pip and only gates
            // structured Url / GitHub sources.
            Self::check_plugin_source_policy(&mp_plugin.source, policy)?;
        }

        // Only do the manifest-load + signature check when there are
        // RequireSignature actions to enforce — avoids double-loading otherwise.
        let has_sig_requirement = policy
            .actions
            .iter()
            .any(|a| matches!(a, PolicyAction::RequireSignature { .. }));

        if has_sig_requirement {
            // Locate the marketplace and plugin manifest to get the raw bytes
            // and the inline signature field before any install side effects.
            let marketplaces = self.list_marketplaces();
            let (_name, mp_manifest) = marketplaces
                .iter()
                .find(|(n, _)| n == marketplace_name)
                .ok_or_else(|| {
                    PluginError::NotFound(format!("Marketplace '{marketplace_name}' not found"))
                })?;

            let mp_plugin = mp_manifest
                .plugins
                .iter()
                .find(|p| p.name == plugin_name)
                .ok_or_else(|| {
                    PluginError::NotFound(format!(
                        "Plugin '{plugin_name}' not found in marketplace '{marketplace_name}'"
                    ))
                })?;

            // For path-based sources we can load the manifest from disk and
            // check the inline `signature` field. For git sources the manifest
            // is not yet cloned — we check the MarketplacePlugin-level
            // signature field (if any) against the serialized plugin entry.
            let marketplace_dir = Self::marketplaces_dir().join(marketplace_name);
            let (manifest_bytes, manifest_sig) = match &mp_plugin.source {
                super::marketplace::PluginSource::Path(rel_path) => {
                    let plugin_dir = marketplace_dir.join(rel_path);
                    // Try loading the plugin manifest to get its signature field.
                    let cc_path = plugin_dir.join(".claude-plugin").join("plugin.json");
                    let root_path = plugin_dir.join("plugin.json");
                    let manifest_path = if cc_path.exists() { cc_path } else { root_path };
                    let raw = std::fs::read(&manifest_path).map_err(|e| {
                        PluginError::IoError(format!(
                            "Cannot read manifest for signature check: {e}"
                        ))
                    })?;
                    let parsed: crate::plugins::manifest::PluginManifest =
                        serde_json::from_slice(&raw).map_err(|e| {
                            PluginError::InvalidManifest(format!(
                                "Cannot parse manifest for signature check: {e}"
                            ))
                        })?;
                    let sig = parsed.signature;
                    (raw, sig)
                }
                super::marketplace::PluginSource::Structured(_) => {
                    // For git/GitHub sources the content is not yet local.
                    // Serialize the marketplace plugin entry as a stable byte
                    // representation for the signature check. This covers the
                    // case where the marketplace index itself is signed.
                    let raw = serde_json::to_vec(mp_plugin).map_err(|e| {
                        PluginError::InvalidManifest(format!(
                            "Cannot serialize plugin entry for signature check: {e}"
                        ))
                    })?;
                    // No inline manifest signature available pre-clone.
                    (raw, None)
                }
            };

            Self::enforce_signature_policy(
                plugin_name,
                &manifest_bytes,
                manifest_sig.as_ref(),
                policy,
            )?;
        }

        // Policy actions satisfied — delegate to the base installer.
        self.install_from_marketplace(plugin_name, marketplace_name)
    }

    /// Convert a [`PolicyRejection`] into a [`PluginError`]. Centralizes
    /// the human-readable reason string so CLI / TUI / audit logs get
    /// consistent messaging regardless of which guard rejected.
    fn policy_rejection_to_error(rejection: PolicyRejection, policy: &PluginPolicy) -> PluginError {
        let reason = match rejection {
            PolicyRejection::Blocked => "source is on the block list".to_string(),
            PolicyRejection::NotInAllowlist => {
                "source is not on the allowed list (strict_known_marketplaces)".to_string()
            }
        };
        PluginError::PolicyRejected {
            reason,
            scope: if policy.managed { "managed" } else { "user" },
        }
    }

    /// Add a marketplace from a local directory, enforcing
    /// `policy.strict_known_marketplaces` / `blocked_marketplaces`.
    /// Prefer this over [`Self::add_marketplace_from_directory`] in
    /// code paths that have a `PluginPolicy` in hand.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::PolicyRejected`] when the source fails
    /// policy checks, or whatever
    /// [`Self::add_marketplace_from_directory`] would return for a
    /// permitted source.
    pub fn add_marketplace_from_directory_with_policy(
        &self,
        source_path: &Path,
        policy: &PluginPolicy,
    ) -> Result<MarketplaceManifest, PluginError> {
        let canonical = source_path
            .canonicalize()
            .unwrap_or_else(|_| source_path.to_path_buf())
            .to_string_lossy()
            .into_owned();
        let source = MarketplaceSource::Directory { path: canonical };
        if let Err(rejection) = policy::check_marketplace_allowed(&source, policy) {
            return Err(Self::policy_rejection_to_error(rejection, policy));
        }
        self.add_marketplace_from_directory(source_path)
    }

    /// Add a marketplace from a git URL, enforcing policy. See
    /// [`Self::add_marketplace_from_directory_with_policy`] for the
    /// error contract.
    ///
    /// # Errors
    ///
    /// Policy rejection → [`PluginError::PolicyRejected`]. Everything
    /// else matches [`Self::add_marketplace_from_git`].
    pub fn add_marketplace_from_git_with_policy(
        &self,
        url: &str,
        git_ref: Option<&str>,
        policy: &PluginPolicy,
    ) -> Result<MarketplaceManifest, PluginError> {
        let source = MarketplaceSource::Git {
            url: url.to_string(),
            git_ref: git_ref.map(str::to_string),
            path: None,
        };
        if let Err(rejection) = policy::check_marketplace_allowed(&source, policy) {
            return Err(Self::policy_rejection_to_error(rejection, policy));
        }
        self.add_marketplace_from_git(url, git_ref)
    }

    /// Add a marketplace from a local directory path
    ///
    /// # Errors
    ///
    /// Returns an error if the directory has no marketplace manifest, IO fails,
    /// or the marketplace already exists.
    pub fn add_marketplace_from_directory(
        &self,
        source_path: &Path,
    ) -> Result<MarketplaceManifest, PluginError> {
        // Validate source has a marketplace manifest
        let manifest_path = source_path.join(".claude-plugin").join("marketplace.json");
        let alt_manifest_path = source_path.join("marketplace.json");
        let mp = if manifest_path.exists() {
            &manifest_path
        } else if alt_manifest_path.exists() {
            &alt_manifest_path
        } else {
            return Err(PluginError::InvalidManifest(
                "No marketplace.json found in directory".to_string(),
            ));
        };
        let content = fs::read_to_string(mp).map_err(|e| PluginError::IoError(e.to_string()))?;
        let manifest: MarketplaceManifest = serde_json::from_str(&content)
            .map_err(|e| PluginError::InvalidManifest(e.to_string()))?;

        // Copy to marketplaces directory
        let dest = Self::marketplaces_dir().join(&manifest.name);
        if dest.exists() {
            return Err(PluginError::InvalidManifest(format!(
                "Marketplace '{}' already exists. Remove it first.",
                manifest.name
            )));
        }
        let canonical_source = source_path.canonicalize().map_err(|e| {
            PluginError::IoError(format!(
                "Failed to canonicalize marketplace source path {}: {}",
                source_path.display(),
                e
            ))
        })?;
        copy_dir_recursive_within(&canonical_source, &dest, &canonical_source)
            .map_err(|e| PluginError::IoError(e.to_string()))?;

        info!(name = %manifest.name, plugins = manifest.plugins.len(), "Added marketplace");
        Ok(manifest)
    }

    /// Add a marketplace from a git repository URL
    ///
    /// # Errors
    /// Returns an error if the git clone fails or the manifest cannot be parsed.
    pub fn add_marketplace_from_git(
        &self,
        url: &str,
        git_ref: Option<&str>,
    ) -> Result<MarketplaceManifest, PluginError> {
        // Validate URL up front — git_clone also validates, but failing here
        // avoids an early mkdir when the URL is going to be rejected.
        super::validate::validate_source_url(url)?;

        let dest = Self::marketplaces_dir();
        fs::create_dir_all(&dest).map_err(|e| PluginError::IoError(e.to_string()))?;

        // Derive the destination name from the URL with the centralized
        // validator — rejects `..`, empty segments, path separators, leading
        // dots, NUL, and control chars. Closes crosslink #248.
        let name = super::validate::derive_dir_name_from_url(url)?;

        let clone_dest = dest.join(&name);
        if clone_dest.exists() {
            return Err(PluginError::InvalidManifest(format!(
                "Marketplace '{name}' already exists. Remove it first."
            )));
        }

        // Clone the repository. SHA is ignored here because marketplaces
        // themselves aren't pinned in install tracking — individual
        // plugin installs carry the commit SHA.
        let _ = git_clone(url, &clone_dest, git_ref)?;

        // Validate the cloned repo has a marketplace manifest
        let manifest_path = clone_dest.join(".claude-plugin").join("marketplace.json");
        let alt_path = clone_dest.join("marketplace.json");
        let mp = if manifest_path.exists() {
            &manifest_path
        } else if alt_path.exists() {
            &alt_path
        } else {
            // Clean up if no manifest
            let _ = fs::remove_dir_all(&clone_dest);
            return Err(PluginError::InvalidManifest(
                "Cloned repository has no marketplace.json".to_string(),
            ));
        };

        let content = fs::read_to_string(mp).map_err(|e| PluginError::IoError(e.to_string()))?;
        let manifest: MarketplaceManifest = serde_json::from_str(&content)
            .map_err(|e| PluginError::InvalidManifest(e.to_string()))?;

        info!(name = %manifest.name, url = %url, plugins = manifest.plugins.len(), "Added git marketplace");
        Ok(manifest)
    }

    /// Remove a marketplace by name
    ///
    /// # Errors
    /// Returns an error if the marketplace is not found or cannot be removed.
    pub fn remove_marketplace(&self, name: &str) -> Result<(), PluginError> {
        let dir = Self::marketplaces_dir().join(name);
        if !dir.exists() {
            return Err(PluginError::NotFound(format!(
                "Marketplace '{name}' not found"
            )));
        }
        fs::remove_dir_all(&dir).map_err(|e| PluginError::IoError(e.to_string()))?;
        info!(name = %name, "Removed marketplace");
        Ok(())
    }

    /// Update a marketplace (git pull or re-copy)
    ///
    /// # Errors
    /// Returns an error if the marketplace is not found or the update fails.
    pub fn update_marketplace(&self, name: &str) -> Result<MarketplaceManifest, PluginError> {
        let dir = Self::marketplaces_dir().join(name);
        if !dir.exists() {
            return Err(PluginError::NotFound(format!(
                "Marketplace '{name}' not found"
            )));
        }

        // Check if it's a git repo
        if dir.join(".git").exists() {
            git_pull(&dir)?;
        } else {
            return Err(PluginError::InvalidManifest(
                "Non-git marketplaces cannot be updated automatically. Remove and re-add."
                    .to_string(),
            ));
        }

        // Re-read manifest
        let manifest_path = dir.join(".claude-plugin").join("marketplace.json");
        let alt_path = dir.join("marketplace.json");
        let mp = if manifest_path.exists() {
            &manifest_path
        } else if alt_path.exists() {
            &alt_path
        } else {
            return Err(PluginError::InvalidManifest(
                "Marketplace manifest missing after update".to_string(),
            ));
        };

        let content = fs::read_to_string(mp).map_err(|e| PluginError::IoError(e.to_string()))?;
        let manifest: MarketplaceManifest = serde_json::from_str(&content)
            .map_err(|e| PluginError::InvalidManifest(e.to_string()))?;
        Ok(manifest)
    }

    /// Install a plugin from a marketplace
    ///
    /// # Errors
    /// Returns an error if the plugin is not found in the marketplace or installation fails.
    #[allow(clippy::too_many_lines)] // Complex installer, splitting would reduce readability
    pub fn install_from_marketplace(
        &mut self,
        plugin_name: &str,
        marketplace_name: &str,
    ) -> Result<String, PluginError> {
        // Find the marketplace
        let marketplaces = self.list_marketplaces();
        let (_name, manifest) = marketplaces
            .iter()
            .find(|(n, _)| n == marketplace_name)
            .ok_or_else(|| {
                PluginError::NotFound(format!("Marketplace '{marketplace_name}' not found"))
            })?;

        // Find the plugin in the marketplace
        let mp_plugin = manifest
            .plugins
            .iter()
            .find(|p| p.name == plugin_name)
            .ok_or_else(|| {
                PluginError::NotFound(format!(
                    "Plugin '{plugin_name}' not found in marketplace '{marketplace_name}'"
                ))
            })?;

        // Determine install path — validate plugin name to prevent path traversal
        if plugin_name.contains("..") || plugin_name.contains('/') || plugin_name.contains('\\') {
            return Err(PluginError::InvalidManifest(format!(
                "Plugin name '{plugin_name}' contains invalid path characters"
            )));
        }
        let plugins_dir = PathBuf::from(".openclaudia/plugins");
        let dest = plugins_dir.join(plugin_name);
        if dest.exists() {
            return Err(PluginError::InvalidManifest(format!(
                "Plugin '{}' already exists at {}",
                plugin_name,
                dest.display()
            )));
        }

        // Install based on source type
        let marketplace_dir = Self::marketplaces_dir().join(marketplace_name);
        // Canonicalize the marketplace root once. This canonical path is the
        // immutable containment boundary used both for the top-level
        // starts_with pre-flight and for the per-entry guard inside
        // copy_dir_recursive_within (crosslink #258).
        let canonical_marketplace = marketplace_dir.canonicalize().map_err(|e| {
            PluginError::IoError(format!(
                "Failed to canonicalize marketplace dir {}: {}",
                marketplace_dir.display(),
                e
            ))
        })?;
        let source_path = match &mp_plugin.source {
            PluginSource::Path(rel_path) => {
                let full = marketplace_dir.join(rel_path);
                if !full.exists() {
                    return Err(PluginError::IoError(format!(
                        "Plugin source path not found: {}",
                        full.display()
                    )));
                }
                // Top-level containment pre-flight: canonicalize the full path
                // and verify it sits inside canonical_marketplace. The per-entry
                // check inside copy_dir_recursive_within provides the definitive
                // per-node guard (crosslink #258).
                let canonical_plugin = full.canonicalize().map_err(|e| {
                    PluginError::IoError(format!(
                        "Failed to canonicalize plugin path {}: {}",
                        full.display(),
                        e
                    ))
                })?;
                if !canonical_plugin.starts_with(&canonical_marketplace) {
                    return Err(PluginError::IoError(format!(
                        "Plugin path traversal detected: {} escapes marketplace directory {}",
                        full.display(),
                        marketplace_dir.display()
                    )));
                }
                canonical_plugin
            }
            PluginSource::Structured(def) => {
                // For structured sources, clone/download directly to dest.
                // Capture the commit SHA returned by `git_clone` so the
                // install record pins exactly what was materialized —
                // crosslink #249 mandated refactor point 1.
                let commit_sha = match def {
                    PluginSourceDef::Url(UrlSource { url, git_ref }) => {
                        // No-silent-HEAD rule (#249 mandated point 5): a
                        // `PluginSourceDef::Url` without an explicit
                        // `git_ref` would silently track upstream HEAD,
                        // meaning any future push to that repo becomes
                        // active in the agent's privilege domain without
                        // review. Require explicit pinning.
                        if git_ref.is_none() {
                            return Err(PluginError::InvalidManifest(format!(
                                "Plugin source URL '{url}' has no `ref`; \
                                 refusing to track upstream HEAD. Specify \
                                 a tag, branch, or commit SHA in the manifest."
                            )));
                        }
                        fs::create_dir_all(&plugins_dir)
                            .map_err(|e| PluginError::IoError(e.to_string()))?;
                        git_clone(url, &dest, git_ref.as_deref())?
                    }
                    PluginSourceDef::GitHub(GitHubSource { repo, git_ref }) => {
                        let resolved_url = format!("https://github.com/{repo}.git");
                        fs::create_dir_all(&plugins_dir)
                            .map_err(|e| PluginError::IoError(e.to_string()))?;
                        git_clone(&resolved_url, &dest, git_ref.as_deref())?
                    }
                    _ => {
                        return Err(PluginError::InvalidManifest(
                            "npm/pip sources not yet supported. Use git or path sources."
                                .to_string(),
                        ));
                    }
                };
                // Track and return (dest already populated by git clone)
                let plugin_id = format!("{plugin_name}@{marketplace_name}");
                let project_root = project_root_cwd();
                let mut installed = InstalledPlugins::load(&project_root);
                installed.upsert(
                    &plugin_id,
                    PluginInstallEntry {
                        scope: InstallScope::Project,
                        project_path: Some(
                            std::env::current_dir()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                        ),
                        install_path: dest.to_string_lossy().to_string(),
                        version: mp_plugin.version.clone(),
                        installed_at: Some(chrono::Utc::now().to_rfc3339()),
                        last_updated: None,
                        git_commit_sha: Some(commit_sha),
                    },
                );
                if let Err(e) = installed.save(&project_root) {
                    warn!("Failed to save install tracking: {}", e);
                }
                let _ = self.reload();
                info!(plugin = %plugin_name, marketplace = %marketplace_name, "Installed plugin from marketplace (git)");
                return Ok(plugin_id);
            }
        };

        // Copy plugin to install directory.
        // source_path is the canonical_plugin path returned from the match arm
        // above — already verified to be within canonical_marketplace.
        // copy_dir_recursive_within enforces containment on every entry in
        // the directory walk, closing the per-entry TOCTOU window (crosslink #258).
        fs::create_dir_all(&plugins_dir).map_err(|e| PluginError::IoError(e.to_string()))?;
        copy_dir_recursive_within(&source_path, &dest, &canonical_marketplace)
            .map_err(|e| PluginError::IoError(e.to_string()))?;

        // Track installation
        let plugin_id = format!("{plugin_name}@{marketplace_name}");
        let project_root = project_root_cwd();
        let mut installed = InstalledPlugins::load(&project_root);
        installed.upsert(
            &plugin_id,
            PluginInstallEntry {
                scope: InstallScope::Project,
                project_path: Some(
                    std::env::current_dir()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                ),
                install_path: dest.to_string_lossy().to_string(),
                version: mp_plugin.version.clone(),
                installed_at: Some(chrono::Utc::now().to_rfc3339()),
                last_updated: None,
                git_commit_sha: None,
            },
        );
        if let Err(e) = installed.save(&project_root) {
            warn!("Failed to save install tracking: {}", e);
        }

        // Reload to pick up the new plugin
        let _ = self.reload();

        info!(plugin = %plugin_name, marketplace = %marketplace_name, "Installed plugin from marketplace");
        Ok(plugin_id)
    }

    /// Install a plugin directly from a git repository
    ///
    /// # Errors
    /// Returns an error if the git clone fails or the plugin manifest is invalid.
    pub fn install_from_git(
        &mut self,
        url: &str,
        git_ref: Option<&str>,
    ) -> Result<String, PluginError> {
        // Reject disallowed URL schemes (http://, file://, git://, inline
        // credentials) before any filesystem work. git_clone will validate
        // again — deliberately redundant, cheap defense-in-depth.
        super::validate::validate_source_url(url)?;

        // Derive the plugins/ subdir name from the URL's last segment with
        // full traversal protection — closes crosslink #248. Previously the
        // url-last-segment extraction was raw and accepted `..`, leading
        // dots, etc., so a crafted URL could place the clone outside the
        // `.openclaudia/plugins/` jail.
        let name = super::validate::derive_dir_name_from_url(url)?;

        let plugins_dir = PathBuf::from(".openclaudia/plugins");
        let dest = plugins_dir.join(&name);
        if dest.exists() {
            return Err(PluginError::InvalidManifest(format!(
                "Plugin '{}' already exists at {}",
                name,
                dest.display()
            )));
        }

        fs::create_dir_all(&plugins_dir).map_err(|e| PluginError::IoError(e.to_string()))?;

        // Clone the repo. Capture the commit SHA so the install record
        // pins exactly what was materialized (crosslink #249 point 1).
        let commit_sha = git_clone(url, &dest, git_ref)?;

        // Validate it's a valid plugin
        match Plugin::load(&dest) {
            Ok(plugin) => {
                let actual_name = plugin.name().to_string();
                // Track installation
                let project_root = project_root_cwd();
                let mut installed = InstalledPlugins::load(&project_root);
                installed.upsert(
                    &actual_name,
                    PluginInstallEntry {
                        scope: InstallScope::Project,
                        project_path: Some(
                            std::env::current_dir()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                        ),
                        install_path: dest.to_string_lossy().to_string(),
                        version: plugin.manifest.version,
                        installed_at: Some(chrono::Utc::now().to_rfc3339()),
                        last_updated: None,
                        git_commit_sha: Some(commit_sha),
                    },
                );
                if let Err(e) = installed.save(&project_root) {
                    warn!("Failed to save install tracking: {}", e);
                }
                let _ = self.reload();
                info!(plugin = %actual_name, url = %url, "Installed plugin from git");
                Ok(actual_name)
            }
            Err(e) => {
                // Clean up invalid clone
                let _ = fs::remove_dir_all(&dest);
                Err(e)
            }
        }
    }

    /// List plugins available from all installed marketplaces
    #[must_use]
    pub fn list_available_plugins(&self) -> Vec<(String, MarketplacePlugin)> {
        let mut available = Vec::new();
        for (marketplace_name, manifest) in self.list_marketplaces() {
            for plugin in &manifest.plugins {
                available.push((marketplace_name.clone(), plugin.clone()));
            }
        }
        available
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn directory_add_rejected_by_blocklist_without_touching_fs() {
        // Build a policy that blocks every Directory source. The
        // add method must fail BEFORE any filesystem side effects —
        // Claude Code's guarantee that the check happens before the
        // download. We verify by handing it a nonexistent path: if
        // the policy check fires first we get PolicyRejected; if
        // the path-read fires first we'd get IoError.
        let tmp = TempDir::new().unwrap();
        let bogus = tmp.path().join("does-not-exist");
        let pm = PluginManager::new();
        let policy = PluginPolicy {
            blocked_marketplaces: vec![MarketplaceSource::Directory {
                path: bogus
                    .canonicalize()
                    .unwrap_or_else(|_| bogus.clone())
                    .to_string_lossy()
                    .into_owned(),
            }],
            ..PluginPolicy::default()
        };
        let err = pm
            .add_marketplace_from_directory_with_policy(&bogus, &policy)
            .expect_err("blocked source must be rejected");
        match err {
            PluginError::PolicyRejected { scope, .. } => {
                assert_eq!(scope, "user");
            }
            other => panic!("expected PolicyRejected, got {other:?}"),
        }
    }

    #[test]
    fn git_add_rejected_when_not_in_strict_allowlist() {
        let pm = PluginManager::new();
        let policy = PluginPolicy {
            strict_known_marketplaces: Some(vec![MarketplaceSource::Git {
                url: "https://example.com/allowed".to_string(),
                git_ref: None,
                path: None,
            }]),
            managed: true,
            ..PluginPolicy::default()
        };
        let err = pm
            .add_marketplace_from_git_with_policy("https://example.com/unknown", None, &policy)
            .expect_err("unknown source must be rejected");
        match err {
            PluginError::PolicyRejected { scope, reason } => {
                assert_eq!(scope, "managed");
                assert!(reason.contains("allowed list"));
            }
            other => panic!("expected PolicyRejected, got {other:?}"),
        }
    }

    #[test]
    fn policy_error_display_is_informative() {
        // Display impl is surfaced to the CLI / TUI — a change here
        // would flow to user-visible strings, so keep it covered.
        let err = PluginError::PolicyRejected {
            reason: "source is on the block list".to_string(),
            scope: "managed",
        };
        let s = err.to_string();
        assert!(s.contains("block list"));
        assert!(s.contains("managed"));
    }

    // -----------------------------------------------------------------
    // crosslink #729 — per-plugin upstream URL is policy-checked.
    //
    // These tests drive `check_plugin_source_policy` directly. That
    // helper is the single gate `install_from_marketplace_with_policy`
    // runs before any filesystem side effects, so exercising it is
    // equivalent to exercising the install gate without needing a real
    // marketplace at `~/.claude/marketplaces/...`.
    // -----------------------------------------------------------------

    fn url_plugin_source(url: &str) -> PluginSource {
        PluginSource::Structured(PluginSourceDef::Url(super::super::marketplace::UrlSource {
            url: url.to_string(),
            git_ref: Some("v1".to_string()),
        }))
    }

    fn github_plugin_source(repo: &str) -> PluginSource {
        PluginSource::Structured(PluginSourceDef::GitHub(
            super::super::marketplace::GitHubSource {
                repo: repo.to_string(),
                git_ref: Some("main".to_string()),
            },
        ))
    }

    #[test]
    fn issue_729_blocklisted_per_plugin_url_is_rejected_with_reason() {
        // Marketplace itself was previously allowlisted, but the plugin
        // entry inside it points at a blocked upstream URL. Without the
        // #729 fix this slips through; with it the install bails out
        // before any git_clone.
        let evil_url = "https://evil.example.com/payload.git";
        let policy = PluginPolicy {
            blocked_marketplaces: vec![MarketplaceSource::Git {
                url: evil_url.to_string(),
                git_ref: None,
                path: None,
            }],
            managed: true,
            ..PluginPolicy::default()
        };
        let source = url_plugin_source(evil_url);
        let err = PluginManager::check_plugin_source_policy(&source, &policy)
            .expect_err("blocked upstream URL must be rejected");
        match err {
            PluginError::PolicyRejected { scope, reason } => {
                assert_eq!(scope, "managed");
                assert!(
                    reason.contains("block list"),
                    "reason must surface the block-list cause, got: {reason}"
                );
            }
            other => panic!("expected PolicyRejected, got {other:?}"),
        }
    }

    #[test]
    fn issue_729_per_plugin_url_not_in_allowlist_is_rejected() {
        // strict_known_marketplaces names only the legitimate
        // marketplace's source; the per-plugin entry resolves to an
        // unrelated upstream. The gate must reject — otherwise the
        // managed allowlist becomes advisory.
        let policy = PluginPolicy {
            strict_known_marketplaces: Some(vec![MarketplaceSource::GitHub {
                repo: "trusted/marketplace".to_string(),
                git_ref: None,
                path: None,
            }]),
            ..PluginPolicy::default()
        };
        // The plugin's structured source points at a different GitHub
        // repo, not on the allowlist.
        let source = github_plugin_source("rogue/plugin-repo");
        let err = PluginManager::check_plugin_source_policy(&source, &policy)
            .expect_err("unlisted upstream must be rejected");
        match err {
            PluginError::PolicyRejected { scope, reason } => {
                assert_eq!(scope, "user");
                assert!(
                    reason.contains("allowed list"),
                    "reason must surface the allowlist cause, got: {reason}"
                );
            }
            other => panic!("expected PolicyRejected, got {other:?}"),
        }
    }

    #[test]
    fn issue_729_allowlisted_per_plugin_url_proceeds() {
        // Plugin's resolved upstream IS on the allowlist (matching by
        // repo, with the rule's ref omitted wildcarding any candidate
        // ref). The gate must return Ok so the install can proceed.
        let policy = PluginPolicy {
            strict_known_marketplaces: Some(vec![MarketplaceSource::GitHub {
                repo: "trusted/plugin-repo".to_string(),
                git_ref: None,
                path: None,
            }]),
            ..PluginPolicy::default()
        };
        let source = github_plugin_source("trusted/plugin-repo");
        PluginManager::check_plugin_source_policy(&source, &policy)
            .expect("allowlisted upstream must be accepted");
    }

    #[test]
    fn issue_729_path_source_bypasses_url_check_but_npm_pip_do_too() {
        // PluginSource::Path is local to the marketplace — its
        // containment was already validated when the marketplace was
        // added. No upstream URL to re-validate, so the gate is a
        // no-op. Likewise npm / pip carry no git URL that the
        // marketplace allowlist could match; the base installer
        // rejects them with InvalidManifest, not the policy gate.
        let policy = PluginPolicy {
            strict_known_marketplaces: Some(vec![]), // deny-all allowlist
            managed: true,
            ..PluginPolicy::default()
        };
        // Path source — gate returns Ok even under a deny-all allowlist.
        let path_source = PluginSource::Path("./local-plugin".to_string());
        PluginManager::check_plugin_source_policy(&path_source, &policy)
            .expect("path source must not be gated by marketplace URL policy");

        // npm source — gate returns Ok (no URL to check); rejection is
        // the base installer's job.
        let npm_source =
            PluginSource::Structured(PluginSourceDef::Npm(super::super::marketplace::NpmSource {
                package: "some-pkg".to_string(),
                version: None,
                registry: None,
            }));
        PluginManager::check_plugin_source_policy(&npm_source, &policy)
            .expect("npm source must not be gated by marketplace URL policy");
    }

    #[test]
    fn issue_729_url_source_blocklist_takes_precedence_over_allowlist() {
        // Block list beats allow list — same semantics as
        // check_marketplace_allowed. A plugin pointing at a URL that's
        // BOTH allowlisted AND blocklisted must still be rejected
        // (blocked wins), and the reason string must say so.
        let url = "https://example.com/contested.git";
        let policy = PluginPolicy {
            strict_known_marketplaces: Some(vec![MarketplaceSource::Git {
                url: url.to_string(),
                git_ref: None,
                path: None,
            }]),
            blocked_marketplaces: vec![MarketplaceSource::Git {
                url: url.to_string(),
                git_ref: None,
                path: None,
            }],
            managed: true,
            ..PluginPolicy::default()
        };
        let source = url_plugin_source(url);
        let err = PluginManager::check_plugin_source_policy(&source, &policy)
            .expect_err("blocked-and-allowlisted URL must still be rejected");
        match err {
            PluginError::PolicyRejected { scope, reason } => {
                assert_eq!(scope, "managed");
                assert!(reason.contains("block list"));
            }
            other => panic!("expected PolicyRejected, got {other:?}"),
        }
    }
}

/// Tests that directly exercise the TOCTOU path-traversal fix from
/// crosslink #258. Each test creates a real filesystem layout (tempdir)
/// and asserts the copy walker accepts or rejects it without going near
/// actual marketplace plumbing.
#[cfg(test)]
mod toctou_tests {
    use crate::plugins::git::{copy_dir_recursive, copy_dir_recursive_within};
    use std::fs;
    use tempfile::TempDir;

    /// A plain directory tree with no symlinks copies successfully and stays
    /// within the allowed root. Validates the happy-path is not broken by
    /// the new per-entry guard.
    #[test]
    fn legitimate_path_within_root_passes() {
        let root = TempDir::new().unwrap();
        let plugin_dir = root.path().join("plugin");
        let sub_dir = plugin_dir.join("sub");
        fs::create_dir_all(&sub_dir).unwrap();
        fs::write(plugin_dir.join("manifest.json"), r#"{"name":"ok"}"#).unwrap();
        fs::write(sub_dir.join("file.txt"), "data").unwrap();

        let dst = TempDir::new().unwrap();
        let output_path = dst.path().join("out");

        let canonical_root = root.path().canonicalize().unwrap();
        let canonical_plugin = plugin_dir.canonicalize().unwrap();

        copy_dir_recursive_within(&canonical_plugin, &output_path, &canonical_root)
            .expect("legitimate tree within root must copy without error");

        assert!(output_path.join("manifest.json").exists());
        assert!(output_path.join("sub/file.txt").exists());
        assert_eq!(
            fs::read_to_string(output_path.join("sub/file.txt")).unwrap(),
            "data"
        );
    }

    /// A symlink inside the source tree that points outside the allowed root
    /// must be rejected. This is the primary TOCTOU scenario from crosslink
    /// #258: an attacker plants a symlink in the marketplace directory that
    /// redirects a copy operation to an arbitrary path.
    #[cfg(unix)]
    #[test]
    fn symlink_to_outside_root_is_rejected() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret").unwrap();

        let plugin_dir = root.path().join("plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("ok.txt"), "ok").unwrap();
        std::os::unix::fs::symlink(outside.path(), plugin_dir.join("evil")).unwrap();

        let dst = TempDir::new().unwrap();
        let output_path = dst.path().join("out");

        let canonical_root = root.path().canonicalize().unwrap();
        let canonical_plugin = plugin_dir.canonicalize().unwrap();

        let err = copy_dir_recursive_within(&canonical_plugin, &output_path, &canonical_root)
            .expect_err("symlink to outside root must be rejected");

        let msg = err.to_string();
        assert!(
            msg.contains("symlink rejected"),
            "error message must name symlink rejection, got: {msg}"
        );
        assert!(
            !output_path.join("evil").exists(),
            "symlink target must not have been copied"
        );
    }

    /// A path resolved outside the allowed root must be rejected even when
    /// no symlinks are present — defence-in-depth for the case where the
    /// top-level `canonicalize+starts_with` check is bypassed.
    #[test]
    fn path_outside_root_is_rejected() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("file.txt"), "exfil").unwrap();

        let canonical_root = root.path().canonicalize().unwrap();
        let canonical_outside = outside.path().canonicalize().unwrap();

        // Precondition: outside is genuinely disjoint from root.
        assert!(!canonical_outside.starts_with(&canonical_root));

        let dst = TempDir::new().unwrap();
        let output_path = dst.path().join("out");

        let err = copy_dir_recursive_within(&canonical_outside, &output_path, &canonical_root)
            .expect_err("path outside allowed root must be rejected");

        let msg = err.to_string();
        assert!(
            msg.contains("path traversal") || msg.contains("escapes allowed root"),
            "error message must name traversal, got: {msg}"
        );
        assert!(
            !output_path.join("file.txt").exists(),
            "file outside root must not have been copied"
        );
    }

    /// The unconstrained `copy_dir_recursive` (no `allowed_root`) still rejects
    /// symlinks — the symlink guard is not conditional on `allowed_root` being set.
    #[cfg(unix)]
    #[test]
    fn copy_dir_recursive_rejects_symlinks_even_without_root_constraint() {
        let src = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        fs::write(target.path().join("secret"), "secret data").unwrap();
        std::os::unix::fs::symlink(target.path(), src.path().join("link")).unwrap();

        let dst = TempDir::new().unwrap();
        let output_path = dst.path().join("out");

        let err = copy_dir_recursive(src.path(), &output_path)
            .expect_err("symlink must be rejected even without root constraint");
        assert!(
            err.to_string().contains("symlink rejected"),
            "error must name symlink rejection, got: {err}"
        );
    }
}
