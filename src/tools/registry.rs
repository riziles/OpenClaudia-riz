//! Tool handler registry — OCP-clean dispatch for tools, mirroring #232's
//! `CommandRegistry` pattern.
//!
//! Adding a new tool is now:
//!   1. Define a unit struct and implement [`ToolHandler`] for it.
//!   2. Add one line to [`HANDLERS`].
//!
//! The central match arms in `execute_tool_with_memory`, `execute_tool_full`,
//! and `execute_tool_with_tasks` have been replaced by
//! [`ToolRegistry::dispatch`].

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::config::AppConfig;
use crate::memory::MemoryDb;
use crate::session::TaskManager;
use serde_json::Value;

// ─── Context ─────────────────────────────────────────────────────────────────

/// Everything a [`ToolHandler`] may need at dispatch time.
///
/// Bundles the three optional context objects that the old 3-function overload
/// set threaded independently. Handlers that don't need a field ignore it.
pub struct ToolContext<'a> {
    /// Optional archival memory database (stateful mode).
    pub memory_db: Option<&'a MemoryDb>,
    /// Optional application configuration (subagent tools).
    pub app_config: Option<&'a AppConfig>,
    /// Optional mutable session task manager (task_* tools).
    pub task_mgr: Option<&'a mut TaskManager>,
}

// ─── Trait ────────────────────────────────────────────────────────────────────

/// A single tool that the agent can invoke.
///
/// Implementations are unit structs stored as `&'static dyn ToolHandler`
/// inside the registry map, avoiding any heap allocation per dispatch.
/// The `execute` method receives context by `&mut` so that task handlers
/// can access the mutable `TaskManager` field.
pub trait ToolHandler: Send + Sync {
    /// The canonical tool name sent by the model.
    fn name(&self) -> &'static str;

    /// Execute the tool and return `(output_text, is_error)`.
    fn execute(&self, args: &HashMap<String, Value>, ctx: &mut ToolContext<'_>) -> (String, bool);
}

// ─── Registry ─────────────────────────────────────────────────────────────────

/// Maps tool names to static handler references.
pub struct ToolRegistry {
    handlers: HashMap<&'static str, &'static dyn ToolHandler>,
}

impl ToolRegistry {
    /// Look up a handler by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&'static dyn ToolHandler> {
        self.handlers.get(name).copied()
    }

    /// Dispatch `tool_name` with `args` to the registered handler, or return
    /// `None` if no handler is registered (caller handles unknown-tool path).
    pub fn dispatch(
        &self,
        tool_name: &str,
        args: &HashMap<String, Value>,
        ctx: &mut ToolContext<'_>,
    ) -> Option<(String, bool)> {
        self.handlers.get(tool_name).map(|h| h.execute(args, ctx))
    }
}

// ─── Handler implementations ──────────────────────────────────────────────────

use super::{ask_user, bash, chainlink, cron, file, lsp, plan_mode, task, todo, web, worktree};

// ── bash ─────────────────────────────────────────────────────────────────────

struct BashHandler;
impl ToolHandler for BashHandler {
    fn name(&self) -> &'static str {
        "bash"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        bash::execute_bash(args)
    }
}

struct BashOutputHandler;
impl ToolHandler for BashOutputHandler {
    fn name(&self) -> &'static str {
        "bash_output"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        bash::execute_bash_output(args)
    }
}

struct KillShellHandler;
impl ToolHandler for KillShellHandler {
    fn name(&self) -> &'static str {
        "kill_shell"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        bash::execute_kill_shell(args)
    }
}

// ── file ─────────────────────────────────────────────────────────────────────

struct ReadFileHandler;
impl ToolHandler for ReadFileHandler {
    fn name(&self) -> &'static str {
        "read_file"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        file::execute_read_file(args)
    }
}

struct WriteFileHandler;
impl ToolHandler for WriteFileHandler {
    fn name(&self) -> &'static str {
        "write_file"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        file::execute_write_file(args)
    }
}

struct EditFileHandler;
impl ToolHandler for EditFileHandler {
    fn name(&self) -> &'static str {
        "edit_file"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        file::execute_edit_file(args)
    }
}

struct NotebookEditHandler;
impl ToolHandler for NotebookEditHandler {
    fn name(&self) -> &'static str {
        "notebook_edit"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        file::execute_notebook_edit(args)
    }
}

struct ListFilesHandler;
impl ToolHandler for ListFilesHandler {
    fn name(&self) -> &'static str {
        "list_files"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        file::execute_list_files(args)
    }
}

// ── chainlink ─────────────────────────────────────────────────────────────────

struct ChainlinkHandler;
impl ToolHandler for ChainlinkHandler {
    fn name(&self) -> &'static str {
        "chainlink"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        chainlink::execute_chainlink(args)
    }
}

// ── web ───────────────────────────────────────────────────────────────────────

struct WebFetchHandler;
impl ToolHandler for WebFetchHandler {
    fn name(&self) -> &'static str {
        "web_fetch"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        web::execute_web_fetch(args)
    }
}

struct WebSearchHandler;
impl ToolHandler for WebSearchHandler {
    fn name(&self) -> &'static str {
        "web_search"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        web::execute_web_search(args)
    }
}

struct WebBrowserHandler;
impl ToolHandler for WebBrowserHandler {
    fn name(&self) -> &'static str {
        "web_browser"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        web::execute_web_browser(args)
    }
}

// ── lsp ───────────────────────────────────────────────────────────────────────

struct LspHandler;
impl ToolHandler for LspHandler {
    fn name(&self) -> &'static str {
        "lsp"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        lsp::execute_lsp(args)
    }
}

// ── todo ─────────────────────────────────────────────────────────────────────

struct TodoWriteHandler;
impl ToolHandler for TodoWriteHandler {
    fn name(&self) -> &'static str {
        "todo_write"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        todo::execute_todo_write(args)
    }
}

struct TodoReadHandler;
impl ToolHandler for TodoReadHandler {
    fn name(&self) -> &'static str {
        "todo_read"
    }
    fn execute(
        &self,
        _args: &HashMap<String, Value>,
        _ctx: &mut ToolContext<'_>,
    ) -> (String, bool) {
        todo::execute_todo_read()
    }
}

// ── ask_user ─────────────────────────────────────────────────────────────────

struct AskUserQuestionHandler;
impl ToolHandler for AskUserQuestionHandler {
    fn name(&self) -> &'static str {
        "ask_user_question"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        ask_user::execute_ask_user_question(args)
    }
}

// ── worktree ─────────────────────────────────────────────────────────────────

struct EnterWorktreeHandler;
impl ToolHandler for EnterWorktreeHandler {
    fn name(&self) -> &'static str {
        "enter_worktree"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        worktree::execute_enter_worktree(args)
    }
}

struct ExitWorktreeHandler;
impl ToolHandler for ExitWorktreeHandler {
    fn name(&self) -> &'static str {
        "exit_worktree"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        worktree::execute_exit_worktree(args)
    }
}

struct ListWorktreesHandler;
impl ToolHandler for ListWorktreesHandler {
    fn name(&self) -> &'static str {
        "list_worktrees"
    }
    fn execute(
        &self,
        _args: &HashMap<String, Value>,
        _ctx: &mut ToolContext<'_>,
    ) -> (String, bool) {
        worktree::execute_list_worktrees()
    }
}

// ── cron ─────────────────────────────────────────────────────────────────────

struct CronCreateHandler;
impl ToolHandler for CronCreateHandler {
    fn name(&self) -> &'static str {
        "cron_create"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        cron::execute_cron_create(args)
    }
}

struct CronDeleteHandler;
impl ToolHandler for CronDeleteHandler {
    fn name(&self) -> &'static str {
        "cron_delete"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        cron::execute_cron_delete(args)
    }
}

struct CronListHandler;
impl ToolHandler for CronListHandler {
    fn name(&self) -> &'static str {
        "cron_list"
    }
    fn execute(
        &self,
        _args: &HashMap<String, Value>,
        _ctx: &mut ToolContext<'_>,
    ) -> (String, bool) {
        cron::execute_cron_list(&HashMap::new())
    }
}

// ── plan_mode ────────────────────────────────────────────────────────────────

struct EnterPlanModeHandler;
impl ToolHandler for EnterPlanModeHandler {
    fn name(&self) -> &'static str {
        "enter_plan_mode"
    }
    fn execute(
        &self,
        _args: &HashMap<String, Value>,
        _ctx: &mut ToolContext<'_>,
    ) -> (String, bool) {
        plan_mode::execute_enter_plan_mode()
    }
}

struct ExitPlanModeHandler;
impl ToolHandler for ExitPlanModeHandler {
    fn name(&self) -> &'static str {
        "exit_plan_mode"
    }
    fn execute(&self, args: &HashMap<String, Value>, _ctx: &mut ToolContext<'_>) -> (String, bool) {
        plan_mode::execute_exit_plan_mode(args)
    }
}

// ── task (session task management) ────────────────────────────────────────────

const NO_SESSION: (&str, bool) = ("Task management not available (no session)", true);

struct TaskCreateHandler;
impl ToolHandler for TaskCreateHandler {
    fn name(&self) -> &'static str {
        "task_create"
    }
    fn execute(&self, args: &HashMap<String, Value>, ctx: &mut ToolContext<'_>) -> (String, bool) {
        ctx.task_mgr.as_deref_mut().map_or_else(
            || (NO_SESSION.0.to_string(), NO_SESSION.1),
            |tm| task::execute_task_create(args, tm),
        )
    }
}

struct TaskUpdateHandler;
impl ToolHandler for TaskUpdateHandler {
    fn name(&self) -> &'static str {
        "task_update"
    }
    fn execute(&self, args: &HashMap<String, Value>, ctx: &mut ToolContext<'_>) -> (String, bool) {
        ctx.task_mgr.as_deref_mut().map_or_else(
            || (NO_SESSION.0.to_string(), NO_SESSION.1),
            |tm| task::execute_task_update(args, tm),
        )
    }
}

struct TaskGetHandler;
impl ToolHandler for TaskGetHandler {
    fn name(&self) -> &'static str {
        "task_get"
    }
    fn execute(&self, args: &HashMap<String, Value>, ctx: &mut ToolContext<'_>) -> (String, bool) {
        ctx.task_mgr.as_deref_mut().map_or_else(
            || (NO_SESSION.0.to_string(), NO_SESSION.1),
            |tm| task::execute_task_get(args, tm),
        )
    }
}

struct TaskListHandler;
impl ToolHandler for TaskListHandler {
    fn name(&self) -> &'static str {
        "task_list"
    }
    fn execute(&self, _args: &HashMap<String, Value>, ctx: &mut ToolContext<'_>) -> (String, bool) {
        ctx.task_mgr.as_deref_mut().map_or_else(
            || (NO_SESSION.0.to_string(), NO_SESSION.1),
            |tm| task::execute_task_list(tm),
        )
    }
}

// ─── Registry construction ────────────────────────────────────────────────────

/// All registered handlers as static references.
///
/// Each handler appears once; the registry key is `handler.name()`.
static HANDLERS: &[&dyn ToolHandler] = &[
    // bash
    &BashHandler,
    &BashOutputHandler,
    &KillShellHandler,
    // file
    &ReadFileHandler,
    &WriteFileHandler,
    &EditFileHandler,
    &NotebookEditHandler,
    &ListFilesHandler,
    // chainlink
    &ChainlinkHandler,
    // web
    &WebFetchHandler,
    &WebSearchHandler,
    &WebBrowserHandler,
    // lsp
    &LspHandler,
    // todo
    &TodoWriteHandler,
    &TodoReadHandler,
    // ask_user
    &AskUserQuestionHandler,
    // worktree
    &EnterWorktreeHandler,
    &ExitWorktreeHandler,
    &ListWorktreesHandler,
    // cron
    &CronCreateHandler,
    &CronDeleteHandler,
    &CronListHandler,
    // plan_mode
    &EnterPlanModeHandler,
    &ExitPlanModeHandler,
    // task (session task management)
    &TaskCreateHandler,
    &TaskUpdateHandler,
    &TaskGetHandler,
    &TaskListHandler,
];

fn build_registry() -> ToolRegistry {
    let mut handlers: HashMap<&'static str, &'static dyn ToolHandler> =
        HashMap::with_capacity(HANDLERS.len());
    for &handler in HANDLERS {
        handlers.insert(handler.name(), handler);
    }
    ToolRegistry { handlers }
}

/// Global registry, initialised exactly once.
pub fn registry() -> &'static ToolRegistry {
    static REGISTRY: OnceLock<ToolRegistry> = OnceLock::new();
    REGISTRY.get_or_init(build_registry)
}
