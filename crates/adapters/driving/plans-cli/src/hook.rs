//! `taskloom manage hook stamp-plan`: ctxloom's post-file-edit hook. The
//! payload (the engine's post-tool-use JSON, on stdin) names the file just
//! edited, `tool_input.file_path` (`notebook_path` for a notebook); when
//! that is a plan file, the session (CTXLOOM_SESSION_HARP) is written to
//! its frontmatter `sessions:`, once. taskloom is the one writer of that
//! key.
//!
//! A hook must not block the edit it follows, so whatever goes wrong, a
//! payload it cannot read or a file it cannot edit, is said on stderr and
//! the hook ends 0. It prints nothing on stdout.

use std::io::Write;

use angzarr_presentation::Catalog;
use plans_application::Plans;
use serde_json::Value;

use crate::{CliError, Settings};

/// The file a payload names: `tool_input.file_path`, else
/// `tool_input.notebook_path`.
fn edited(payload: &str) -> Result<String, CliError> {
    let value: Value = serde_json::from_str(payload).map_err(|e| CliError::HookPayload {
        detail: e.to_string(),
    })?;
    ["file_path", "notebook_path"]
        .iter()
        .find_map(|key| value["tool_input"][key].as_str())
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .ok_or(CliError::HookNoFile)
}

/// Stamp the session `settings` carries into the plan file `payload`
/// names; the exit code, always 0.
pub fn stamp<P: Plans>(
    payload: &str,
    settings: &Settings,
    catalog: &Catalog,
    plans: &mut P,
    err: &mut dyn Write,
) -> u8 {
    let stamped = edited(payload).and_then(|path| {
        plans
            .stamp(&path, &settings.session)
            .map(|_| ())
            .map_err(|error| CliError::HookFailed { error })
    });
    if let Err(error) = stamped {
        let _ = writeln!(err, "{}: {}", settings.prog, catalog.message(&error));
    }
    0
}

#[cfg(test)]
#[path = "hook.test.rs"]
mod tests;
