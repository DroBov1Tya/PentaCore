// Budgets and pages of `resume` and `task_summary`.

// Largest `task_summary` reply, in characters. `resume` has no such limit.
pub const TASK_SUMMARY_MAX_CHARS: usize = 64 * 1024;
pub const SECTION_ROWS: i64 = 100;
pub const TRUNCATION_MARK: &str = " ... [truncated]";
pub const DEFAULT_CHILDREN: usize = 20;
pub const MAX_CHILDREN: usize = 100;

// `resume` cuts no text; this stands for "all of it" where a cut length is asked for.
pub const WHOLE_TEXT: usize = i32::MAX as usize;
