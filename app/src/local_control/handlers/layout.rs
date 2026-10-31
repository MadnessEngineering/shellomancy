//! Layout mutation handlers for local-control actions.
#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
use std::path::PathBuf;
use std::str::FromStr as _;

use ::local_control::protocol::{TabCreateParams, TabType, TargetSelector};
use ::local_control::{ActionKind, ControlError, ErrorCode, InstanceId};
use serde::Serialize;
use warp_core::ui::theme::AnsiColorIdentifier;
use warpui::{ModelContext, SingletonEntity as _, TypedActionView};

use super::app_state::validate_staged_input_text;
use crate::local_control::LocalControlBridge;
use crate::local_control::permissions::ensure_settings_allow_command_execution;
use crate::settings::LocalControlSettings;
use crate::local_control::resolver::{
    decode_params, target_window_id_for_target, validate_tab_create_target, workspace_for_window,
};
use crate::workspace::WorkspaceAction;
#[derive(Serialize)]
struct TabCreateResponse<'a> {
    action: &'static str,
    created: bool,
    instance_id: Option<&'a str>,
    window: TargetWindowResponse,
    tab: TabCountsResponse,
    /// Shellomancy: set when a command was queued to run once the shell is ready.
    #[serde(skip_serializing_if = "Option::is_none")]
    command_queued: Option<bool>,
}

/// Shellomancy: longest command `tab.create` accepts.
const MAX_COMMAND_LEN: usize = 4096;

/// Shellomancy: validated launch fields for `tab.create`.
struct TabLaunch {
    command: Option<String>,
    cwd: Option<PathBuf>,
    title: Option<String>,
    color: Option<AnsiColorIdentifier>,
    focus: bool,
}

#[derive(Serialize)]
struct TargetWindowResponse {
    selector: &'static str,
    id: String,
}

#[derive(Serialize)]
struct TabCountsResponse {
    id: String,
    previous_count: usize,
    count: usize,
    active_index: usize,
}

pub(crate) fn create_tab(
    instance_id: &Option<InstanceId>,
    params: &serde_json::Value,
    target: &TargetSelector,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<serde_json::Value, ControlError> {
    validate_tab_create_target(target)?;
    let params = decode_params::<TabCreateParams>(params)?;
    // Validate before touching the UI so a rejected request creates nothing.
    let launch = if params.has_launch_fields() {
        Some(tab_launch(&params, LocalControlSettings::as_ref(ctx))?)
    } else {
        None
    };
    let window_id = target_window_id_for_target(ctx, target, ActionKind::TabCreate)?;
    let workspace = workspace_for_window(window_id, ActionKind::TabCreate, ctx)?;
    let command_queued = launch.as_ref().map(|launch| launch.command.is_some());
    let focus = launch.as_ref().is_none_or(|launch| launch.focus);
    let (tab_id, previous_tab_count, tab_count, active_tab_index) = match launch {
        Some(launch) => workspace.update(ctx, |workspace, ctx| {
            let previous_tab_count = workspace.tab_count();
            let tab_id = workspace.add_terminal_tab_with_startup_command(
                launch.command,
                launch.cwd,
                launch.title,
                launch.color,
                launch.focus,
                ctx,
            );
            Ok::<_, ControlError>((
                tab_id.to_string(),
                previous_tab_count,
                workspace.tab_count(),
                workspace.active_tab_index(),
            ))
        })?,
        None => {
            let action = tab_create_action(&params)?;
            workspace.update(ctx, |workspace, ctx| {
                let previous_tab_count = workspace.tab_count();
                workspace.handle_action(&action, ctx);
                let tab_id = workspace
                    .get_pane_group_view(workspace.active_tab_index())
                    .map(|tab| tab.id().to_string())
                    .ok_or_else(|| {
                        ControlError::new(
                            ErrorCode::Internal,
                            "tab.create did not produce an active tab identifier",
                        )
                    })?;
                Ok((
                    tab_id,
                    previous_tab_count,
                    workspace.tab_count(),
                    workspace.active_tab_index(),
                ))
            })?
        }
    };
    // Shellomancy: launch requests raise the app unless focus is false.
    if focus && command_queued.is_some() {
        ctx.windows().show_window_and_focus_app(window_id);
    }
    serde_json::to_value(TabCreateResponse {
        action: ActionKind::TabCreate.as_str(),
        created: true,
        instance_id: instance_id.as_ref().map(|id| id.0.as_str()),
        window: TargetWindowResponse {
            selector: "target",
            id: window_id.to_string(),
        },
        tab: TabCountsResponse {
            id: tab_id,
            previous_count: previous_tab_count,
            count: tab_count,
            active_index: active_tab_index,
        },
        command_queued,
    })
    .map_err(|err| {
        ControlError::with_details(
            ErrorCode::Internal,
            "failed to serialize local-control tab.create response",
            err.to_string(),
        )
    })
}

/// Shellomancy: validates `tab.create` launch fields.
fn tab_launch(
    params: &TabCreateParams,
    settings: &LocalControlSettings,
) -> Result<TabLaunch, ControlError> {
    let invalid = |message: String| ControlError::new(ErrorCode::InvalidParams, message);
    if !matches!(params.tab_type, None | Some(TabType::Terminal)) {
        return Err(invalid(
            "command, cwd, title, color and focus require a terminal tab".to_owned(),
        ));
    }
    let command = match params.command.as_deref().map(str::trim) {
        None => None,
        Some("") => return Err(invalid("command must not be empty".to_owned())),
        Some(command) => {
            ensure_settings_allow_command_execution(settings)?;
            if command.len() > MAX_COMMAND_LEN {
                return Err(invalid(format!(
                    "command is longer than {MAX_COMMAND_LEN} bytes"
                )));
            }
            validate_staged_input_text(ActionKind::TabCreate, command)?;
            Some(command.to_owned())
        }
    };
    let cwd = match params.cwd.as_deref() {
        None => None,
        Some(cwd) => {
            let path = PathBuf::from(cwd);
            if !path.is_absolute() || !path.is_dir() {
                return Err(invalid(format!("cwd {cwd} is not an existing absolute directory")));
            }
            Some(path)
        }
    };
    let title = match params.title.as_deref().map(str::trim) {
        None => None,
        Some("") => return Err(invalid("title must not be empty".to_owned())),
        Some(title) => Some(title.to_owned()),
    };
    let color = match params.color.as_deref() {
        None => None,
        Some(color) => Some(
            AnsiColorIdentifier::from_str(color)
                .map_err(|_| invalid(format!("{color} is not a supported tab color")))?,
        ),
    };
    Ok(TabLaunch {
        command,
        cwd,
        title,
        color,
        focus: params.focus.unwrap_or(true),
    })
}

fn tab_create_action(params: &TabCreateParams) -> Result<WorkspaceAction, ControlError> {
    match params.tab_type {
        None | Some(TabType::Terminal) => Ok(WorkspaceAction::AddTerminalTab {
            hide_homepage: false,
        }),
        Some(TabType::Agent) => Ok(WorkspaceAction::AddAgentTab),
        Some(TabType::Default) => Ok(WorkspaceAction::AddDefaultTab),
        Some(TabType::CloudAgent) => Err(ControlError::new(
            ErrorCode::UnsupportedAction,
            "tab.create does not support cloud-agent tabs",
        )),
    }
}
