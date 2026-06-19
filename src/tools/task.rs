use crate::session::TaskManager;
use crate::tools::args::ToolArgs as _;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::BuildHasher;

/// Execute the `task_create` tool
pub fn execute_task_create<S: BuildHasher>(
    args: &HashMap<String, Value, S>,
    task_mgr: &mut TaskManager,
) -> (String, bool) {
    // crosslink #675: typed accessors. Wording was already canonical
    // ("Missing 'X' argument") so no test churn.
    let subject = match args.arg_str_strict("subject") {
        Ok(s) => s.to_string(),
        Err(e) => return e.into_tool_error(),
    };
    let description = match args.arg_str_strict("description") {
        Ok(d) => d.to_string(),
        Err(e) => return e.into_tool_error(),
    };

    let active_form = match args.arg_str_opt_strict("active_form") {
        Ok(active_form) => active_form.map(std::string::ToString::to_string),
        Err(e) => return e.into_tool_error(),
    };

    let task = task_mgr.create_task(subject, description, active_form);
    let output = format!(
        "Created task: {}\n{}",
        task.id,
        TaskManager::format_task_detail(task)
    );
    (output, false)
}

/// Execute the `task_update` tool
pub fn execute_task_update<S: BuildHasher>(
    args: &HashMap<String, Value, S>,
    task_mgr: &mut TaskManager,
) -> (String, bool) {
    let task_id = match args.arg_str_strict("task_id") {
        Ok(task_id) => task_id,
        Err(e) => return e.into_tool_error(),
    };

    let status = match parse_task_update_status(args.get("status")) {
        Ok(status) => status,
        Err(msg) => return (msg, true),
    };
    let subject = match parse_optional_string_field(args.get("subject"), "subject") {
        Ok(value) => value,
        Err(msg) => return (msg, true),
    };
    let description = match parse_optional_string_field(args.get("description"), "description") {
        Ok(value) => value,
        Err(msg) => return (msg, true),
    };
    let active_form = match parse_optional_string_field(args.get("active_form"), "active_form") {
        Ok(value) => value,
        Err(msg) => return (msg, true),
    };
    let add_blocks = match parse_optional_string_array(args.get("add_blocks"), "add_blocks") {
        Ok(value) => value,
        Err(msg) => return (msg, true),
    };
    let add_blocked_by =
        match parse_optional_string_array(args.get("add_blocked_by"), "add_blocked_by") {
            Ok(value) => value,
            Err(msg) => return (msg, true),
        };

    match task_mgr.update_task(
        task_id,
        crate::session::TaskUpdateParams {
            status,
            subject,
            description,
            active_form,
            add_blocks,
            add_blocked_by,
        },
    ) {
        Ok(Some(task)) => {
            let output = format!(
                "Updated task: {}\n{}",
                task.id,
                TaskManager::format_task_detail(task)
            );
            (output, false)
        }
        Ok(None) => {
            // Task was deleted successfully
            (format!("Task '{task_id}' deleted"), false)
        }
        Err(msg) => (msg, true),
    }
}

fn parse_task_update_status(
    value: Option<&Value>,
) -> Result<Option<crate::session::TaskUpdateStatus>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(status) = value.as_str() else {
        return Err(
            "Invalid task status '<non-string>'. Must be: pending, in_progress, completed, deleted"
                .to_string(),
        );
    };
    crate::session::TaskUpdateStatus::parse(status)
        .map(Some)
        .ok_or_else(|| {
            format!(
                "Invalid task status '{status}'. Must be: pending, in_progress, completed, deleted"
            )
        })
}

fn parse_optional_string_field(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    value.as_str().map(String::from).map(Some).ok_or_else(|| {
        format!("Invalid task_update field '{field}': expected string when supplied")
    })
}

fn parse_optional_string_array(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Option<Vec<String>>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(items) = value.as_array() else {
        return Err(format!(
            "Invalid task_update field '{field}': expected array of strings when supplied"
        ));
    };

    let mut parsed = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let Some(s) = item.as_str() else {
            return Err(format!(
                "Invalid task_update field '{field}[{idx}]': expected string"
            ));
        };
        parsed.push(s.to_string());
    }
    Ok(Some(parsed))
}

/// Execute the `task_get` tool.
///
/// crosslink #588: a missing `task_id` is a successful lookup of "no such
/// task", not an error — match CC's `TaskGetTool`, which resolves with
/// `null` when the id is unknown. Returning an error here would force the
/// model into a recovery path for what is a legitimate, expected outcome
/// (e.g. polling a task that was deleted). The success payload is the
/// literal JSON `null` so structured consumers can branch on it cheaply.
#[must_use]
pub fn execute_task_get<S: BuildHasher>(
    args: &HashMap<String, Value, S>,
    task_mgr: &TaskManager,
) -> (String, bool) {
    let task_id = match args.arg_str_strict("task_id") {
        Ok(task_id) => task_id,
        Err(e) => return e.into_tool_error(),
    };

    task_mgr.get_task(task_id).map_or_else(
        || (Value::Null.to_string(), false),
        |task| (TaskManager::format_task_detail(task), false),
    )
}

/// Execute the `task_list` tool
#[must_use]
pub fn execute_task_list(task_mgr: &TaskManager) -> (String, bool) {
    let tasks = task_mgr.list_tasks();

    if tasks.is_empty() {
        return ("No tasks.".to_string(), false);
    }

    let mut output = String::new();
    for task in tasks {
        output.push_str(&TaskManager::format_task_summary(task));
        output.push('\n');
    }

    let completed = tasks
        .iter()
        .filter(|t| t.status == crate::session::TaskStatus::Completed)
        .count();
    let in_progress = tasks
        .iter()
        .filter(|t| t.status == crate::session::TaskStatus::InProgress)
        .count();
    let pending = tasks
        .iter()
        .filter(|t| t.status == crate::session::TaskStatus::Pending)
        .count();

    let _ = write!(
        output,
        "\n({} total: {} completed, {} in progress, {} pending)",
        tasks.len(),
        completed,
        in_progress,
        pending
    );

    (output, false)
}
