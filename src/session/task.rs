//! Structured task management with dependency tracking.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// Status of a managed task
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
        }
    }
}

/// A structured task with dependency tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task identifier (auto-incrementing, e.g. "task-1")
    pub id: String,
    /// Brief title in imperative form (e.g. "Add permission system")
    pub subject: String,
    /// Detailed description of the task
    pub description: String,
    /// Present continuous form for spinner display (e.g. "Adding permission system")
    pub active_form: Option<String>,
    /// Current task status
    pub status: TaskStatus,
    /// IDs of tasks that this task blocks (downstream dependencies)
    pub blocks: Vec<String>,
    /// IDs of tasks that block this task (upstream dependencies)
    pub blocked_by: Vec<String>,
    /// When the task was created
    pub created_at: DateTime<Utc>,
}

/// Status values accepted by task updates (includes Deleted which removes the task).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskUpdateStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

impl TaskUpdateStatus {
    /// Parse from string. Returns None for unrecognized values.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "deleted" => Some(Self::Deleted),
            _ => None,
        }
    }
}

/// Parameters for updating an existing task.
#[derive(Default)]
pub struct TaskUpdateParams {
    pub status: Option<TaskUpdateStatus>,
    pub subject: Option<String>,
    pub description: Option<String>,
    pub active_form: Option<String>,
    pub add_blocks: Option<Vec<String>>,
    pub add_blocked_by: Option<Vec<String>>,
}

/// Manages structured tasks with dependency tracking.
///
/// Enforces the invariant that only one task can be `InProgress` at a time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskManager {
    tasks: Vec<Task>,
    next_id: u64,
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManager {
    /// Create a new empty `TaskManager`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tasks: Vec::new(),
            next_id: 1,
        }
    }

    /// Create a new task. Returns the created task.
    ///
    /// # Panics
    ///
    /// Panics if the internal tasks vector is somehow empty after pushing
    /// (should be unreachable).
    pub fn create_task(
        &mut self,
        subject: String,
        description: String,
        active_form: Option<String>,
    ) -> &Task {
        let id = format!("task-{}", self.next_id);
        self.next_id += 1;

        let task = Task {
            id,
            subject,
            description,
            active_form,
            status: TaskStatus::Pending,
            blocks: Vec::new(),
            blocked_by: Vec::new(),
            created_at: Utc::now(),
        };
        self.tasks.push(task);
        self.tasks
            .last()
            .expect("tasks must be non-empty after push")
    }

    /// Get a task by ID.
    #[must_use]
    pub fn get_task(&self, task_id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == task_id)
    }

    /// Get a mutable reference to a task by ID.
    fn get_task_mut(&mut self, task_id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == task_id)
    }

    /// Check if adding an edge from `from_id` blocks `to_id` would create a cycle.
    /// Uses DFS: starting from `to_id`, follow `blocks` edges. If we reach `from_id`,
    /// there's a cycle.
    fn would_create_cycle(&self, from_id: &str, to_id: &str) -> bool {
        let mut visited = std::collections::HashSet::new();
        let mut stack = vec![to_id.to_string()];
        while let Some(current) = stack.pop() {
            if current == from_id {
                return true;
            }
            if !visited.insert(current.clone()) {
                continue;
            }
            if let Some(task) = self.get_task(&current) {
                for blocked in &task.blocks {
                    stack.push(blocked.clone());
                }
            }
        }
        false
    }

    /// Update a task's fields. Returns an error message if validation fails.
    ///
    /// Enforces that only one task can be `InProgress` at a time. When a task
    /// is set to `InProgress`, any currently in-progress task is moved back to
    /// `Pending`.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the task is not found, the status is invalid,
    /// a dependency references itself or a nonexistent task, or the task
    /// is deleted (deletion is signaled via `Err` with a message).
    ///
    /// # Panics
    ///
    /// Panics if internal lookups fail after validation (should be unreachable).
    #[allow(clippy::too_many_lines)]
    pub fn update_task(
        &mut self,
        task_id: &str,
        params: TaskUpdateParams,
    ) -> Result<Option<&Task>, String> {
        // Validate the task exists
        if self.get_task(task_id).is_none() {
            return Err(format!("Task '{task_id}' not found"));
        }

        let TaskUpdateParams {
            status,
            subject,
            description,
            active_form,
            add_blocks,
            add_blocked_by,
        } = params;

        // Handle status update
        let new_status = match status {
            Some(TaskUpdateStatus::Deleted) => {
                self.tasks.retain(|t| t.id != task_id);
                return Ok(None);
            }
            Some(TaskUpdateStatus::Pending) => Some(TaskStatus::Pending),
            Some(TaskUpdateStatus::InProgress) => Some(TaskStatus::InProgress),
            Some(TaskUpdateStatus::Completed) => Some(TaskStatus::Completed),
            None => None,
        };

        // If setting to InProgress, demote any currently in-progress task
        if new_status == Some(TaskStatus::InProgress) {
            for task in &mut self.tasks {
                if task.status == TaskStatus::InProgress && task.id != task_id {
                    task.status = TaskStatus::Pending;
                }
            }
        }

        // Validate dependency references
        if let Some(ref block_ids) = add_blocks {
            for bid in block_ids {
                if bid == task_id {
                    return Err("A task cannot block itself".to_string());
                }
                if !self.tasks.iter().any(|t| t.id == *bid) {
                    return Err(format!("Referenced task '{bid}' not found"));
                }
                // Cycle detection: if bid already (transitively) blocks task_id, adding
                // task_id blocks bid would create a cycle.
                if self.would_create_cycle(task_id, bid) {
                    return Err(format!(
                        "Adding '{task_id}' blocks '{bid}' would create a circular dependency"
                    ));
                }
            }
        }
        if let Some(ref blocked_ids) = add_blocked_by {
            for bid in blocked_ids {
                if bid == task_id {
                    return Err("A task cannot be blocked by itself".to_string());
                }
                if !self.tasks.iter().any(|t| t.id == *bid) {
                    return Err(format!("Referenced task '{bid}' not found"));
                }
                // Cycle detection: if task_id already (transitively) blocks bid, adding
                // bid blocks task_id would create a cycle.
                if self.would_create_cycle(bid, task_id) {
                    return Err(format!(
                        "Adding '{bid}' blocks '{task_id}' would create a circular dependency"
                    ));
                }
            }
        }

        // Apply updates to the task -- task existence validated above
        let task = self
            .get_task_mut(task_id)
            .expect("task must exist after validation");

        if let Some(s) = new_status {
            task.status = s;
        }
        if let Some(subj) = subject {
            task.subject = subj;
        }
        if let Some(desc) = description {
            task.description = desc;
        }
        if active_form.is_some() {
            task.active_form = active_form;
        }
        if let Some(block_ids) = add_blocks {
            for bid in block_ids {
                if !task.blocks.contains(&bid) {
                    task.blocks.push(bid.clone());
                }
                // Also add the reverse relationship on the other task
                // We need to drop the mutable borrow first, so we collect and do it below
            }
        }
        if let Some(blocked_ids) = add_blocked_by {
            for bid in blocked_ids {
                if !task.blocked_by.contains(&bid) {
                    task.blocked_by.push(bid.clone());
                }
            }
        }

        // Now handle reverse relationships for add_blocks/add_blocked_by
        // We need to re-borrow after the first mutable borrow ends
        let task_id_owned = task_id.to_string();

        // Second pass: sync reverse dependencies
        // Collect the current blocks and blocked_by for the target task
        let current_blocks: Vec<String> = self
            .get_task(&task_id_owned)
            .map(|t| t.blocks.clone())
            .unwrap_or_default();
        let current_blocked_by: Vec<String> = self
            .get_task(&task_id_owned)
            .map(|t| t.blocked_by.clone())
            .unwrap_or_default();

        // For each task that this task blocks, ensure they have us in blocked_by
        for bid in &current_blocks {
            if let Some(other) = self.get_task_mut(bid) {
                if !other.blocked_by.contains(&task_id_owned) {
                    other.blocked_by.push(task_id_owned.clone());
                }
            }
        }

        // For each task that blocks this task, ensure they have us in blocks
        for bid in &current_blocked_by {
            if let Some(other) = self.get_task_mut(bid) {
                if !other.blocks.contains(&task_id_owned) {
                    other.blocks.push(task_id_owned.clone());
                }
            }
        }

        Ok(Some(
            self.get_task(&task_id_owned)
                .expect("task must exist after update"),
        ))
    }

    /// List all tasks.
    #[must_use]
    pub fn list_tasks(&self) -> &[Task] {
        &self.tasks
    }

    /// Get the currently in-progress task, if any.
    #[must_use]
    pub fn current_task(&self) -> Option<&Task> {
        self.tasks
            .iter()
            .find(|t| t.status == TaskStatus::InProgress)
    }

    /// Format a task summary for display.
    #[must_use]
    pub fn format_task_summary(task: &Task) -> String {
        let status_icon = match task.status {
            TaskStatus::Pending => "[ ]",
            TaskStatus::InProgress => "[>]",
            TaskStatus::Completed => "[x]",
        };

        let mut summary = format!(
            "{status_icon} {} {} ({})",
            task.id, task.subject, task.status
        );

        if let Some(ref af) = task.active_form {
            let _ = write!(summary, " -- {af}");
        }

        if !task.blocks.is_empty() {
            let _ = write!(summary, "\n    blocks: {}", task.blocks.join(", "));
        }
        if !task.blocked_by.is_empty() {
            let _ = write!(summary, "\n    blocked_by: {}", task.blocked_by.join(", "));
        }

        summary
    }

    /// Format full task details for display.
    #[must_use]
    pub fn format_task_detail(task: &Task) -> String {
        let mut detail = String::new();
        let _ = writeln!(detail, "ID: {}", task.id);
        let _ = writeln!(detail, "Subject: {}", task.subject);
        let _ = writeln!(detail, "Status: {}", task.status);
        let _ = writeln!(detail, "Description: {}", task.description);
        if let Some(ref af) = task.active_form {
            let _ = writeln!(detail, "Active form: {af}");
        }
        let _ = writeln!(
            detail,
            "Created: {}",
            task.created_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
        if !task.blocks.is_empty() {
            let _ = writeln!(detail, "Blocks: {}", task.blocks.join(", "));
        }
        if !task.blocked_by.is_empty() {
            let _ = writeln!(detail, "Blocked by: {}", task.blocked_by.join(", "));
        }
        detail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_manager_create() {
        let mut tm = TaskManager::new();
        let task = tm.create_task(
            "Implement feature".to_string(),
            "Add the new feature".to_string(),
            Some("Implementing feature".to_string()),
        );
        assert_eq!(task.id, "task-1");
        assert_eq!(task.subject, "Implement feature");
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.active_form, Some("Implementing feature".to_string()));
    }

    #[test]
    fn test_task_manager_auto_increment() {
        let mut tm = TaskManager::new();
        tm.create_task("A".to_string(), "Desc".to_string(), None);
        tm.create_task("B".to_string(), "Desc".to_string(), None);
        tm.create_task("C".to_string(), "Desc".to_string(), None);

        let tasks = tm.list_tasks();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].id, "task-1");
        assert_eq!(tasks[1].id, "task-2");
        assert_eq!(tasks[2].id, "task-3");
    }

    #[test]
    fn test_task_manager_update_status() {
        let mut tm = TaskManager::new();
        tm.create_task("Task A".to_string(), "Desc".to_string(), None);

        let result = tm.update_task(
            "task-1",
            TaskUpdateParams {
                status: Some(TaskUpdateStatus::InProgress),
                ..Default::default()
            },
        );
        assert!(result.is_ok());
        assert_eq!(
            tm.get_task("task-1").unwrap().status,
            TaskStatus::InProgress
        );
    }

    #[test]
    fn test_task_manager_single_in_progress() {
        let mut tm = TaskManager::new();
        tm.create_task("Task A".to_string(), "Desc".to_string(), None);
        tm.create_task("Task B".to_string(), "Desc".to_string(), None);

        tm.update_task(
            "task-1",
            TaskUpdateParams {
                status: Some(TaskUpdateStatus::InProgress),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            tm.get_task("task-1").unwrap().status,
            TaskStatus::InProgress
        );

        tm.update_task(
            "task-2",
            TaskUpdateParams {
                status: Some(TaskUpdateStatus::InProgress),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(tm.get_task("task-1").unwrap().status, TaskStatus::Pending);
        assert_eq!(
            tm.get_task("task-2").unwrap().status,
            TaskStatus::InProgress
        );
    }

    #[test]
    fn test_task_manager_delete() {
        let mut tm = TaskManager::new();
        tm.create_task("To delete".to_string(), "Desc".to_string(), None);
        assert_eq!(tm.list_tasks().len(), 1);

        let result = tm.update_task(
            "task-1",
            TaskUpdateParams {
                status: Some(TaskUpdateStatus::Deleted),
                ..Default::default()
            },
        );
        // "deleted" returns Ok(None) — task removed, no reference to return
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
        assert_eq!(tm.list_tasks().len(), 0);
    }

    #[test]
    fn test_task_update_status_parse() {
        assert_eq!(
            TaskUpdateStatus::parse("pending"),
            Some(TaskUpdateStatus::Pending)
        );
        assert_eq!(
            TaskUpdateStatus::parse("in_progress"),
            Some(TaskUpdateStatus::InProgress)
        );
        assert_eq!(
            TaskUpdateStatus::parse("completed"),
            Some(TaskUpdateStatus::Completed)
        );
        assert_eq!(
            TaskUpdateStatus::parse("deleted"),
            Some(TaskUpdateStatus::Deleted)
        );
        assert_eq!(TaskUpdateStatus::parse("invalid"), None);
        assert_eq!(TaskUpdateStatus::parse(""), None);
    }

    #[test]
    fn test_task_manager_not_found() {
        let mut tm = TaskManager::new();
        let result = tm.update_task(
            "task-999",
            TaskUpdateParams {
                status: Some(TaskUpdateStatus::Completed),
                ..Default::default()
            },
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_task_manager_dependencies() {
        let mut tm = TaskManager::new();
        tm.create_task("Setup".to_string(), "First step".to_string(), None);
        tm.create_task("Build".to_string(), "Second step".to_string(), None);

        // task-2 blocked by task-1
        tm.update_task(
            "task-2",
            TaskUpdateParams {
                add_blocked_by: Some(vec!["task-1".to_string()]),
                ..Default::default()
            },
        )
        .unwrap();

        let task1 = tm.get_task("task-1").unwrap();
        let task2 = tm.get_task("task-2").unwrap();
        assert!(task2.blocked_by.contains(&"task-1".to_string()));
        assert!(task1.blocks.contains(&"task-2".to_string()));
    }

    #[test]
    fn test_task_manager_self_dependency_blocked() {
        let mut tm = TaskManager::new();
        tm.create_task("Task".to_string(), "Desc".to_string(), None);

        let result = tm.update_task(
            "task-1",
            TaskUpdateParams {
                add_blocks: Some(vec!["task-1".to_string()]),
                ..Default::default()
            },
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot block itself"));
    }

    #[test]
    fn test_task_manager_current_task() {
        let mut tm = TaskManager::new();
        assert!(tm.current_task().is_none());

        tm.create_task("Task".to_string(), "Desc".to_string(), None);
        assert!(tm.current_task().is_none()); // still pending

        tm.update_task(
            "task-1",
            TaskUpdateParams {
                status: Some(TaskUpdateStatus::InProgress),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(tm.current_task().is_some());
        assert_eq!(tm.current_task().unwrap().id, "task-1");
    }

    #[test]
    fn test_task_manager_format_summary() {
        let mut tm = TaskManager::new();
        let task = tm.create_task(
            "Fix bug".to_string(),
            "Fix the null pointer".to_string(),
            Some("Fixing bug".to_string()),
        );
        let summary = TaskManager::format_task_summary(task);
        assert!(summary.contains("[ ]")); // pending icon
        assert!(summary.contains("task-1"));
        assert!(summary.contains("Fix bug"));
        assert!(summary.contains("Fixing bug"));
    }

    // ── Phase 2 spec-pinning tests (#552 / spec #537 B-session/task) ─────────

    /// Spec — `TaskStatus::Display` renders the canonical strings used by the
    /// tool layer and the session summary.
    #[test]
    fn task_status_display_strings() {
        assert_eq!(TaskStatus::Pending.to_string(), "pending");
        assert_eq!(TaskStatus::InProgress.to_string(), "in_progress");
        assert_eq!(TaskStatus::Completed.to_string(), "completed");
    }

    /// Spec — creating a task always starts it as `Pending`.
    #[test]
    fn new_task_starts_pending() {
        let mut tm = TaskManager::new();
        let t = tm.create_task("Deploy".to_string(), "desc".to_string(), None);
        assert_eq!(t.status, TaskStatus::Pending);
    }

    /// Spec — setting a second task to `InProgress` demotes the current one to
    /// `Pending`. Enforces the single-in-progress invariant.
    #[test]
    fn single_in_progress_invariant() {
        let mut tm = TaskManager::new();
        tm.create_task("A".to_string(), "d".to_string(), None);
        tm.create_task("B".to_string(), "d".to_string(), None);
        tm.create_task("C".to_string(), "d".to_string(), None);

        tm.update_task(
            "task-1",
            TaskUpdateParams {
                status: Some(TaskUpdateStatus::InProgress),
                ..Default::default()
            },
        )
        .unwrap();
        tm.update_task(
            "task-2",
            TaskUpdateParams {
                status: Some(TaskUpdateStatus::InProgress),
                ..Default::default()
            },
        )
        .unwrap();
        tm.update_task(
            "task-3",
            TaskUpdateParams {
                status: Some(TaskUpdateStatus::InProgress),
                ..Default::default()
            },
        )
        .unwrap();

        // Only task-3 should be InProgress; task-1 and task-2 must be Pending.
        assert_eq!(tm.get_task("task-1").unwrap().status, TaskStatus::Pending);
        assert_eq!(tm.get_task("task-2").unwrap().status, TaskStatus::Pending);
        assert_eq!(
            tm.get_task("task-3").unwrap().status,
            TaskStatus::InProgress
        );

        // `current_task()` reflects the single in-progress entry.
        let cur = tm.current_task().unwrap();
        assert_eq!(cur.id, "task-3");
    }

    /// Spec — updating a non-existent task returns `Err`.
    #[test]
    fn update_nonexistent_task_returns_err() {
        let mut tm = TaskManager::new();
        let res = tm.update_task("task-99", TaskUpdateParams::default());
        assert!(res.is_err());
    }

    /// Spec — `Deleted` status removes the task from the list and returns `Ok(None)`.
    #[test]
    fn delete_removes_task_returns_ok_none() {
        let mut tm = TaskManager::new();
        tm.create_task("X".to_string(), "d".to_string(), None);
        let res = tm.update_task(
            "task-1",
            TaskUpdateParams {
                status: Some(TaskUpdateStatus::Deleted),
                ..Default::default()
            },
        );
        assert!(res.is_ok());
        assert!(res.unwrap().is_none(), "Deleted must return Ok(None)");
        assert!(tm.list_tasks().is_empty(), "task must be gone from list");
    }

    /// Spec — cycle detection: A blocks B, then adding B blocks A must fail.
    #[test]
    fn cycle_detection_rejects_circular_dependency() {
        let mut tm = TaskManager::new();
        tm.create_task("A".to_string(), "d".to_string(), None);
        tm.create_task("B".to_string(), "d".to_string(), None);

        // task-1 blocks task-2
        tm.update_task(
            "task-1",
            TaskUpdateParams {
                add_blocks: Some(vec!["task-2".to_string()]),
                ..Default::default()
            },
        )
        .unwrap();

        // Now try to make task-2 block task-1 — should be rejected as a cycle
        let res = tm.update_task(
            "task-2",
            TaskUpdateParams {
                add_blocks: Some(vec!["task-1".to_string()]),
                ..Default::default()
            },
        );
        assert!(res.is_err(), "circular dependency must be rejected");
        let msg = res.unwrap_err();
        assert!(
            msg.contains("circular") || msg.contains("cycle"),
            "error must mention circularity, got: {msg}"
        );
    }

    /// Spec — `add_blocks` syncs the reverse `blocked_by` on the target task.
    #[test]
    fn add_blocks_syncs_reverse_blocked_by() {
        let mut tm = TaskManager::new();
        tm.create_task("First".to_string(), "d".to_string(), None);
        tm.create_task("Second".to_string(), "d".to_string(), None);

        tm.update_task(
            "task-1",
            TaskUpdateParams {
                add_blocks: Some(vec!["task-2".to_string()]),
                ..Default::default()
            },
        )
        .unwrap();

        let t2 = tm.get_task("task-2").unwrap();
        assert!(
            t2.blocked_by.contains(&"task-1".to_string()),
            "task-2.blocked_by must contain task-1 after add_blocks"
        );
    }

    /// Spec — `format_task_detail` contains all key fields.
    #[test]
    fn format_task_detail_contains_required_fields() {
        let mut tm = TaskManager::new();
        let task = tm.create_task(
            "Write tests".to_string(),
            "Full description here".to_string(),
            Some("Writing tests".to_string()),
        );
        let detail = TaskManager::format_task_detail(task);
        assert!(detail.contains("Write tests"));
        assert!(detail.contains("Full description here"));
        assert!(detail.contains("Writing tests"));
        assert!(detail.contains("pending"), "detail must include status");
    }
}
