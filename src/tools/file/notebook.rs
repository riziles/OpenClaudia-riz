use super::{resolve_path, READ_TRACKER};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;

/// Split source text into a JSON array of line strings for notebook cell source format.
/// Each line except possibly the last ends with '\n'.
pub fn source_to_line_array(source: &str) -> Value {
    if source.is_empty() {
        return json!([]);
    }
    let lines: Vec<&str> = source.split('\n').collect();
    let mut result: Vec<Value> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        if i < lines.len() - 1 {
            // Not the last line: append \n
            result.push(json!(format!("{}\n", line)));
        } else {
            // Last line: include as-is (no trailing \n unless empty)
            if !line.is_empty() {
                result.push(json!(*line));
            }
        }
    }
    result.into()
}

/// Look up a cell's position in the array by its stable `id` field
/// (set by modern Jupyter clients in each cell's top-level metadata).
/// Returns `None` when no cell matches.
fn find_cell_by_id(cells: &[Value], cell_id: &str) -> Option<usize> {
    cells.iter().position(|c| {
        c.get("id")
            .and_then(|v| v.as_str())
            .is_some_and(|id| id == cell_id)
    })
}

/// Edit a Jupyter notebook cell.
///
/// Accepts either `cell_id` (Claude Code-compatible — matches the `id`
/// field Jupyter clients write into each cell's top-level metadata) or
/// `cell_number` (legacy 0-indexed position, kept for back-compat). At
/// least one of the two must be present for `replace` and `delete`.
/// For `insert`, `cell_id` means "insert AFTER the cell with this id";
/// omitting both inserts at position 0.
#[allow(clippy::too_many_lines)]
pub fn execute_notebook_edit(args: &HashMap<String, Value>) -> (String, bool) {
    let Some(raw_path) = args.get("notebook_path").and_then(|v| v.as_str()) else {
        return ("Missing 'notebook_path' argument".to_string(), true);
    };

    let resolved = match resolve_path(raw_path) {
        Ok(p) => p,
        Err(e) => return (e, true),
    };

    let cell_id_arg = args
        .get("cell_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let cell_number_arg = args
        .get("cell_number")
        .and_then(serde_json::Value::as_u64)
        .map(|n| usize::try_from(n).unwrap_or(usize::MAX));

    let Some(new_source) = args.get("new_source").and_then(|v| v.as_str()) else {
        return ("Missing 'new_source' argument".to_string(), true);
    };

    let cell_type = args.get("cell_type").and_then(|v| v.as_str());
    let edit_mode = args
        .get("edit_mode")
        .and_then(|v| v.as_str())
        .unwrap_or("replace");

    // Validate edit_mode
    if !["replace", "insert", "delete"].contains(&edit_mode) {
        return (
            format!("Invalid edit_mode '{edit_mode}'. Must be 'replace', 'insert', or 'delete'."),
            true,
        );
    }

    // Enforce read-before-edit
    if !READ_TRACKER.has_been_read(&resolved) {
        return (
            format!(
                "You must read '{}' before editing it. Use read_file first to see the actual contents.",
                resolved.display()
            ),
            true,
        );
    }

    // Blast radius check
    // Resolve symlinks to prevent path traversal
    let notebook_path = match std::fs::canonicalize(&resolved) {
        Ok(canon) => canon.to_string_lossy().to_string(),
        Err(_) => {
            return (
                format!("Cannot resolve notebook path '{}'", resolved.display()),
                true,
            );
        }
    };
    let notebook_path = notebook_path.as_str();

    if let Err(msg) = crate::guardrails::check_file_access(notebook_path) {
        return (msg, true);
    }

    // Read and parse the notebook
    let content = match fs::read_to_string(notebook_path) {
        Ok(c) => c,
        Err(e) => {
            return (
                format!("Failed to read notebook '{notebook_path}': {e}"),
                true,
            )
        }
    };

    let mut notebook: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            return (
                format!("Failed to parse notebook '{notebook_path}' as JSON: {e}"),
                true,
            )
        }
    };

    let Some(cells) = notebook.get_mut("cells").and_then(|c| c.as_array_mut()) else {
        return ("Notebook has no 'cells' array.".to_string(), true);
    };

    // Resolve the target index from whichever of cell_id / cell_number
    // the caller supplied. `cell_id` wins when both are present — it's
    // the stable identifier, `cell_number` shifts whenever a cell gets
    // inserted above it.
    let resolved_index: Option<usize> = if let Some(id) = cell_id_arg.as_deref() {
        match find_cell_by_id(cells, id) {
            Some(idx) => Some(idx),
            None => {
                return (
                    format!("No cell with id '{id}' found in notebook."),
                    true,
                );
            }
        }
    } else {
        cell_number_arg
    };
    // Display text for error messages — "id 'abc'" when id was provided,
    // "number N" otherwise.
    let target_desc = if let Some(id) = cell_id_arg.as_deref() {
        format!("id '{id}'")
    } else if let Some(n) = cell_number_arg {
        format!("number {n}")
    } else {
        "<unspecified>".to_string()
    };

    // Index used when we print "Replaced cell ..." / "Deleted cell ..."
    // in the summary. Filled in per-branch; stays None for the "insert
    // at the beginning" case where no prior cell_id was passed.
    let mut summary_index: Option<usize> = resolved_index;

    match edit_mode {
        "replace" => {
            let Some(index) = resolved_index else {
                return (
                    "replace requires either 'cell_id' or 'cell_number'.".to_string(),
                    true,
                );
            };
            if index >= cells.len() {
                return (
                    format!(
                        "Cell {target_desc} is out of bounds. Notebook has {} cells (valid range: 0-{}).",
                        cells.len(),
                        cells.len().saturating_sub(1)
                    ),
                    true,
                );
            }
            cells[index]["source"] = source_to_line_array(new_source);
            if let Some(ct) = cell_type {
                cells[index]["cell_type"] = json!(ct);
            }
        }
        "insert" => {
            let Some(ct) = cell_type else {
                return (
                    "cell_type is required when inserting a new cell. Use 'code' or 'markdown'."
                        .to_string(),
                    true,
                );
            };

            // Semantics diverge from replace/delete here: Claude Code's
            // cell_id means "insert AFTER this cell", so the insertion
            // position is index+1. Legacy cell_number still means "at
            // this position". Omitting both inserts at the beginning.
            let insert_at = match (cell_id_arg.as_deref(), cell_number_arg) {
                (Some(_), _) => resolved_index.map_or(0, |i| i + 1),
                (None, Some(n)) => n,
                (None, None) => 0,
            };

            if insert_at > cells.len() {
                return (
                    format!(
                        "Cell {target_desc} is out of bounds for insertion. Notebook has {} cells (valid range: 0-{}).",
                        cells.len(),
                        cells.len()
                    ),
                    true,
                );
            }

            let mut new_cell = json!({
                "cell_type": ct,
                "metadata": {},
                "source": source_to_line_array(new_source)
            });
            if ct == "code" {
                new_cell["outputs"] = json!([]);
                new_cell["execution_count"] = Value::Null;
            }
            cells.insert(insert_at, new_cell);
            summary_index = Some(insert_at);
        }
        "delete" => {
            let Some(index) = resolved_index else {
                return (
                    "delete requires either 'cell_id' or 'cell_number'.".to_string(),
                    true,
                );
            };
            if index >= cells.len() {
                return (
                    format!(
                        "Cell {target_desc} is out of bounds. Notebook has {} cells (valid range: 0-{}).",
                        cells.len(),
                        cells.len().saturating_sub(1)
                    ),
                    true,
                );
            }
            cells.remove(index);
        }
        _ => unreachable!(),
    }

    // Write back with pretty formatting
    let old_lines = u32::try_from(content.lines().count()).unwrap_or(u32::MAX);
    match serde_json::to_string_pretty(&notebook) {
        Ok(pretty) => {
            let new_lines = u32::try_from(pretty.lines().count()).unwrap_or(u32::MAX);
            match fs::write(notebook_path, &pretty) {
                Ok(()) => {
                    crate::guardrails::record_file_modification(
                        notebook_path,
                        new_lines,
                        old_lines,
                    );
                    let where_str = summary_index
                        .map_or_else(|| target_desc.clone(), |idx| format!("{idx}"));
                    let action = match edit_mode {
                        "replace" => format!("Replaced cell {where_str} contents"),
                        "insert" => format!(
                            "Inserted new {} cell at position {}",
                            cell_type.unwrap_or("unknown"),
                            where_str
                        ),
                        "delete" => format!("Deleted cell {where_str}"),
                        _ => unreachable!(),
                    };
                    let mut result = format!(
                        "Successfully edited '{}'. {}. Notebook now has {} cells.",
                        notebook_path,
                        action,
                        notebook
                            .get("cells")
                            .and_then(|c| c.as_array())
                            .map_or(0, std::vec::Vec::len)
                    );
                    if let Some(warning) = crate::guardrails::check_diff_thresholds() {
                        let _ = write!(result, "\n\nWarning: {}", warning.message);
                    }
                    (result, false)
                }
                Err(e) => (
                    format!("Failed to write notebook '{notebook_path}': {e}"),
                    true,
                ),
            }
        }
        Err(e) => (format!("Failed to serialize notebook: {e}"), true),
    }
}
