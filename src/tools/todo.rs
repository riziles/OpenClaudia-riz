use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Mutex;

/// Todo item for task tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: String,
    #[serde(rename = "activeForm")]
    pub active_form: String,
}

/// Global todo list storage
static TODO_LIST: std::sync::LazyLock<Mutex<Vec<TodoItem>>> =
    std::sync::LazyLock::new(|| Mutex::new(Vec::new()));

/// Write/update the todo list
pub fn execute_todo_write(args: &HashMap<String, Value>) -> (String, bool) {
    let Some(todos_value) = args.get("todos") else {
        return ("Missing 'todos' argument".to_string(), true);
    };

    let Some(todos_array) = todos_value.as_array() else {
        return ("'todos' must be an array".to_string(), true);
    };

    let mut new_todos: Vec<TodoItem> = Vec::new();
    let mut in_progress_count = 0;

    for (i, item) in todos_array.iter().enumerate() {
        let content = match item.get("content").and_then(|v| v.as_str()) {
            Some(c) if c.len() > 2000 => {
                return (
                    format!("Todo {i} content exceeds maximum length of 2000 characters"),
                    true,
                );
            }
            Some(c) => c.to_string(),
            None => return (format!("Todo {i} missing 'content' field"), true),
        };

        let status = match item.get("status").and_then(|v| v.as_str()) {
            Some(s) => {
                if !["pending", "in_progress", "completed"].contains(&s) {
                    return (
                        format!(
                            "Todo {i} has invalid status '{s}'. Must be: pending, in_progress, completed"
                        ),
                        true,
                    );
                }
                if s == "in_progress" {
                    in_progress_count += 1;
                }
                s.to_string()
            }
            None => return (format!("Todo {i} missing 'status' field"), true),
        };

        let active_form = match item.get("activeForm").and_then(|v| v.as_str()) {
            Some(a) => a.to_string(),
            None => return (format!("Todo {i} missing 'activeForm' field"), true),
        };

        new_todos.push(TodoItem {
            content,
            status,
            active_form,
        });
    }

    // Warn if more than one task is in_progress
    let warning = if in_progress_count > 1 {
        format!(
            "\nWarning: {in_progress_count} tasks marked as in_progress. Best practice is to have only one."
        )
    } else {
        String::new()
    };

    // Claude Code parity: when every item is `completed`, clear the list
    // instead of keeping a list of done items. Keeps the session cleanup
    // clean and signals the agent to stop referring back to finished work.
    // See claude-code/tools/TodoWriteTool/TodoWriteTool.ts (`allDone` branch).
    let all_done = !new_todos.is_empty()
        && new_todos.iter().all(|t| t.status == "completed");
    let stored_todos = if all_done {
        Vec::new()
    } else {
        new_todos.clone()
    };

    // Update the global todo list
    match TODO_LIST.lock() {
        Ok(mut list) => {
            list.clone_from(&stored_todos);
        }
        Err(e) => return (format!("Failed to update todo list: {e}"), true),
    }

    if all_done {
        return (
            format!(
                "Todos have been modified successfully — all {} items completed, list cleared.",
                new_todos.len()
            ),
            false,
        );
    }

    // Format output for the non-all-done case.
    let completed = new_todos.iter().filter(|t| t.status == "completed").count();
    let in_progress = new_todos
        .iter()
        .filter(|t| t.status == "in_progress")
        .count();
    let pending = new_todos.iter().filter(|t| t.status == "pending").count();

    let mut output = format!(
        "Todo list updated: {} total ({} completed, {} in progress, {} pending){}",
        new_todos.len(),
        completed,
        in_progress,
        pending,
        warning
    );

    // Show current in-progress task if any
    if let Some(current) = new_todos.iter().find(|t| t.status == "in_progress") {
        let _ = write!(output, "\n\nCurrently: {}", current.active_form);
    }

    (output, false)
}

/// Read the current todo list
pub fn execute_todo_read() -> (String, bool) {
    let todos = match TODO_LIST.lock() {
        Ok(list) => list.clone(),
        Err(e) => return (format!("Failed to read todo list: {e}"), true),
    };

    if todos.is_empty() {
        return ("No todos in list.".to_string(), false);
    }

    let mut output = String::new();
    for (i, todo) in todos.iter().enumerate() {
        let status_icon = match todo.status.as_str() {
            "completed" => "[x]",
            "in_progress" => "[>]",
            "pending" => "[ ]",
            _ => "[?]",
        };
        let _ = writeln!(output, "{}. {} {}", i + 1, status_icon, todo.content);
    }

    // Summary
    let completed = todos.iter().filter(|t| t.status == "completed").count();
    let in_progress = todos.iter().filter(|t| t.status == "in_progress").count();
    let pending = todos.iter().filter(|t| t.status == "pending").count();

    let _ = write!(
        output,
        "\n({completed} completed, {in_progress} in progress, {pending} pending)"
    );

    (output, false)
}

/// Get the current todo list (for external use)
pub fn get_todo_list() -> Vec<TodoItem> {
    TODO_LIST.lock().map(|l| l.clone()).unwrap_or_default()
}

/// Clear the todo list
pub fn clear_todo_list() {
    if let Ok(mut list) = TODO_LIST.lock() {
        list.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// The task list is process-global state (matches the original
    /// design), so these tests serialize on a shared mutex to avoid
    /// interleaving under `cargo test`'s parallel runner.
    fn task_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn args_with(v: Value) -> HashMap<String, Value> {
        let mut m = HashMap::new();
        m.insert("todos".to_string(), v);
        m
    }

    #[test]
    fn all_done_clears_the_list() {
        let _lock = task_lock();
        clear_todo_list();

        let args = args_with(json!([
            {"content": "one", "status": "completed", "activeForm": "Doing one"},
            {"content": "two", "status": "completed", "activeForm": "Doing two"},
        ]));
        let (msg, err) = execute_todo_write(&args);
        assert!(!err);
        assert!(msg.contains("all 2 items completed"));
        assert!(get_todo_list().is_empty());
    }

    #[test]
    fn mixed_statuses_are_preserved() {
        let _lock = task_lock();
        clear_todo_list();

        let args = args_with(json!([
            {"content": "one", "status": "completed", "activeForm": "Doing one"},
            {"content": "two", "status": "in_progress", "activeForm": "Doing two"},
        ]));
        let (_, err) = execute_todo_write(&args);
        assert!(!err);
        let stored = get_todo_list();
        assert_eq!(stored.len(), 2, "partial completion must keep the list");
    }

    #[test]
    fn empty_input_is_not_treated_as_all_done() {
        let _lock = task_lock();
        clear_todo_list();

        let args = args_with(json!([]));
        let (msg, err) = execute_todo_write(&args);
        assert!(!err);
        // Empty input must NOT trigger the "all done" cleared-list
        // message — that message implies the agent finished actual work.
        assert!(!msg.contains("all 0 items completed"));
        assert!(get_todo_list().is_empty());
    }
}
