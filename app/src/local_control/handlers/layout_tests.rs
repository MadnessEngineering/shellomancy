use ::local_control::protocol::TargetSelector;
use ::local_control::{ErrorCode, InstanceId};
use warpui::App;

use settings::Setting as _;
use warpui::SingletonEntity as _;

use super::create_tab;
use crate::features::FeatureFlag;
use crate::local_control::LocalControlBridge;
use crate::settings::{LocalControlMode, LocalControlSettings};
use crate::workspace::view::tests::{initialize_app, mock_workspace};

#[test]
fn tab_create_handler_adds_and_activates_terminal_tab() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let previous_count = workspace.read(&app, |workspace, _| workspace.tab_count());
        let bridge = app.add_singleton_model(LocalControlBridge::new);
        let instance_id = InstanceId("inst_test".to_owned());

        let response = bridge.update(&mut app, |bridge, ctx| {
            bridge.set_instance_id(instance_id.clone());
            create_tab(
                &Some(instance_id.clone()),
                &serde_json::json!({}),
                &TargetSelector::default(),
                ctx,
            )
            .expect("tab.create handler succeeds")
        });

        workspace.read(&app, |workspace, _| {
            assert_eq!(workspace.tab_count(), previous_count + 1);
            assert_eq!(workspace.active_tab_index(), previous_count);
        });
        assert_eq!(response["action"], "tab.create");
        assert_eq!(response["created"], true);
        assert_eq!(response["instance_id"], "inst_test");
        assert_eq!(response["tab"]["previous_count"], previous_count);
        assert_eq!(response["tab"]["count"], previous_count + 1);
        assert_eq!(response["tab"]["active_index"], previous_count);
        assert!(response["tab"]["id"].is_string());
    });
}

#[test]
fn tab_create_rejects_shell_parameter() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let _workspace = mock_workspace(&mut app);
        let bridge = app.add_singleton_model(LocalControlBridge::new);
        let instance_id = InstanceId("inst_test".to_owned());

        let err = bridge.update(&mut app, |bridge, ctx| {
            bridge.set_instance_id(instance_id.clone());
            create_tab(
                &Some(instance_id.clone()),
                &serde_json::json!({ "shell": "zsh" }),
                &TargetSelector::default(),
                ctx,
            )
            .expect_err("shell parameter must be rejected")
        });

        assert_eq!(err.code, ErrorCode::InvalidParams);
    });
}

/// Shellomancy: `tab.create` with launch fields queues the command in a new,
/// titled terminal tab using the tab-config pending-command path.
#[test]
fn tab_create_with_command_queues_it_in_a_new_titled_tab() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let previous_count = workspace.read(&app, |workspace, _| workspace.tab_count());
        let bridge = app.add_singleton_model(LocalControlBridge::new);
        let cwd = std::env::temp_dir().canonicalize().expect("temp dir");

        let response = bridge.update(&mut app, |_, ctx| {
            create_tab(
                &None,
                &serde_json::json!({
                    "command": "echo shellomancy",
                    "cwd": cwd.to_string_lossy(),
                    "title": "agent",
                    "color": "cyan",
                }),
                &TargetSelector::default(),
                ctx,
            )
            .expect("tab.create with a command succeeds")
        });

        assert_eq!(response["command_queued"], true);
        assert_eq!(response["tab"]["count"], previous_count + 1);
        workspace.read(&app, |workspace, ctx| {
            assert_eq!(workspace.tab_count(), previous_count + 1);
            let tab = workspace
                .get_pane_group_view(workspace.active_tab_index())
                .expect("new tab is active");
            assert_eq!(tab.id().to_string(), response["tab"]["id"]);
            let tab = tab.as_ref(ctx);
            assert_eq!(tab.custom_title(ctx).as_deref(), Some("agent"));
            let session = tab.active_session_view(ctx).expect("terminal session");
            // Bootstrap never completes in tests, so the command stays pending.
            assert!(session.as_ref(ctx).has_pending_command_or_awaiting_completion(ctx));
        });
    });
}

#[test]
fn tab_create_with_focus_false_keeps_the_previous_tab_active() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let (previous_count, previous_active) = workspace.read(&app, |workspace, _| {
            (workspace.tab_count(), workspace.active_tab_index())
        });
        let bridge = app.add_singleton_model(LocalControlBridge::new);

        let response = bridge.update(&mut app, |_, ctx| {
            create_tab(
                &None,
                &serde_json::json!({ "command": "true", "focus": false }),
                &TargetSelector::default(),
                ctx,
            )
            .expect("background tab.create succeeds")
        });

        assert_eq!(response["tab"]["count"], previous_count + 1);
        workspace.read(&app, |workspace, _| {
            assert_eq!(workspace.tab_count(), previous_count + 1);
            assert_eq!(workspace.active_tab_index(), previous_active);
        });
    });
}

#[test]
fn tab_create_rejects_invalid_launch_fields_without_creating_a_tab() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let previous_count = workspace.read(&app, |workspace, _| workspace.tab_count());
        let bridge = app.add_singleton_model(LocalControlBridge::new);

        for params in [
            serde_json::json!({ "command": "" }),
            serde_json::json!({ "command": "echo one\necho two" }),
            serde_json::json!({ "command": "true", "tab_type": "agent" }),
            serde_json::json!({ "cwd": "relative/dir" }),
            serde_json::json!({ "cwd": "/definitely/not/a/real/dir" }),
            serde_json::json!({ "title": "   " }),
            serde_json::json!({ "color": "chartreuse" }),
        ] {
            let err = bridge.update(&mut app, |_, ctx| {
                create_tab(&None, &params, &TargetSelector::default(), ctx)
                    .expect_err("invalid launch field must be rejected")
            });
            assert_eq!(err.code, ErrorCode::InvalidParams, "params: {params}");
        }
        workspace.read(&app, |workspace, _| {
            assert_eq!(workspace.tab_count(), previous_count);
        });
    });
}

#[test]
fn tab_create_command_requires_the_command_execution_mode() {
    let _flag = FeatureFlag::WarpControlCli.override_enabled(true);
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let previous_count = workspace.read(&app, |workspace, _| workspace.tab_count());
        app.update(|ctx| {
            LocalControlSettings::handle(ctx).update(ctx, |settings, ctx| {
                settings
                    .local_control_mode
                    .set_value(LocalControlMode::Enabled, ctx)
            })
        })
        .expect("local control mode updates");
        let bridge = app.add_singleton_model(LocalControlBridge::new);

        let err = bridge.update(&mut app, |_, ctx| {
            create_tab(
                &None,
                &serde_json::json!({ "command": "true" }),
                &TargetSelector::default(),
                ctx,
            )
            .expect_err("plain Enabled must not run commands")
        });
        assert_eq!(err.code, ErrorCode::InsufficientPermissions);

        // Launch fields without a command are still allowed.
        bridge.update(&mut app, |_, ctx| {
            create_tab(
                &None,
                &serde_json::json!({ "title": "plain" }),
                &TargetSelector::default(),
                ctx,
            )
            .expect("title-only tab.create is allowed without command execution")
        });
        workspace.read(&app, |workspace, _| {
            assert_eq!(workspace.tab_count(), previous_count + 1);
        });
    });
}

#[test]
fn tab_create_rejects_agent_tabs_when_agent_mode_is_off() {
    let _flag = FeatureFlag::AgentMode.override_enabled(false);
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let previous_count = workspace.read(&app, |workspace, _| workspace.tab_count());
        let bridge = app.add_singleton_model(LocalControlBridge::new);

        let err = bridge.update(&mut app, |_, ctx| {
            create_tab(
                &None,
                &serde_json::json!({ "tab_type": "agent" }),
                &TargetSelector::default(),
                ctx,
            )
            .expect_err("agent tabs need Agent Mode")
        });
        assert_eq!(err.code, ErrorCode::UnsupportedAction);
        workspace.read(&app, |workspace, _| {
            assert_eq!(workspace.tab_count(), previous_count);
        });
    });
}
