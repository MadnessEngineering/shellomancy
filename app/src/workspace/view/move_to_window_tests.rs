use warpui::{App, TypedActionView};

use super::super::tests::{initialize_app, mock_workspace};
use super::*;

fn window_of(workspace: &ViewHandle<Workspace>, app: &App) -> WindowId {
    workspace.read(app, |workspace, _| workspace.window_id)
}

#[test]
fn a_moved_tab_lands_active_at_the_end_of_the_other_window() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let source = mock_workspace(&mut app);
        let target = mock_workspace(&mut app);
        let target_window = window_of(&target, &app);

        let moved = source.update(&mut app, |workspace, ctx| {
            workspace.add_terminal_tab(false, ctx);
            let moved = workspace.tabs[1].pane_group.id();
            workspace.handle_action(
                &WorkspaceAction::MoveTabToWindow {
                    tab_index: 1,
                    destination: TabWindowDestination::Window(target_window),
                },
                ctx,
            );
            moved
        });

        source.read(&app, |workspace, _| {
            assert_eq!(workspace.tabs.len(), 1);
            assert!(
                workspace
                    .tabs
                    .iter()
                    .all(|tab| tab.pane_group.id() != moved)
            );
        });
        target.read(&app, |workspace, _| {
            assert_eq!(workspace.tabs.len(), 2);
            assert_eq!(workspace.tabs[1].pane_group.id(), moved);
            assert_eq!(workspace.active_tab_index(), 1);
        });
    });
}

#[test]
fn a_windows_only_tab_still_moves() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let source = mock_workspace(&mut app);
        let target = mock_workspace(&mut app);
        let target_window = window_of(&target, &app);

        let moved = source.update(&mut app, |workspace, ctx| {
            let moved = workspace.tabs[0].pane_group.id();
            assert_eq!(
                workspace.move_tab_to_window(0, TabWindowDestination::Window(target_window), ctx),
                Some(target_window)
            );
            moved
        });

        target.read(&app, |workspace, _| {
            assert_eq!(workspace.tabs.len(), 2);
            assert_eq!(workspace.tabs[1].pane_group.id(), moved);
        });
    });
}

#[test]
fn moving_to_its_own_window_changes_nothing() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);
        let window = window_of(&workspace, &app);

        workspace.update(&mut app, |workspace, ctx| {
            workspace.add_terminal_tab(false, ctx);
            assert_eq!(
                workspace.move_tab_to_window(1, TabWindowDestination::Window(window), ctx),
                None
            );
            assert_eq!(workspace.tabs.len(), 2);
        });
    });
}

#[test]
fn a_windows_only_tab_cannot_leave_for_a_new_window() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let workspace = mock_workspace(&mut app);

        workspace.update(&mut app, |workspace, ctx| {
            assert_eq!(
                workspace.move_tab_to_window(0, TabWindowDestination::NewWindow, ctx),
                None
            );
            assert_eq!(workspace.tabs.len(), 1);
        });
    });
}

#[test]
fn destinations_are_the_other_windows_then_a_new_window() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let first = mock_workspace(&mut app);

        // A lone tab in the only window has nowhere to go while quake mode is off.
        first.read(&app, |workspace, ctx| {
            assert!(workspace.move_to_window_destinations(0, ctx).is_empty());
        });

        let second = mock_workspace(&mut app);
        let first_window = window_of(&first, &app);
        let second_window = window_of(&second, &app);
        first.update(&mut app, |workspace, ctx| {
            workspace.add_terminal_tab(false, ctx)
        });

        first.read(&app, |workspace, ctx| {
            assert_eq!(
                workspace.move_to_window_destinations(0, ctx),
                vec![
                    TabWindowDestination::Window(second_window),
                    TabWindowDestination::NewWindow,
                ]
            );
        });
        second.read(&app, |workspace, ctx| {
            assert_eq!(
                workspace.move_to_window_destinations(0, ctx),
                vec![TabWindowDestination::Window(first_window)]
            );
        });
    });
}

#[test]
fn a_window_with_more_tabs_says_how_many() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let source = mock_workspace(&mut app);
        let target = mock_workspace(&mut app);
        let target_window = window_of(&target, &app);
        target.update(&mut app, |workspace, ctx| {
            workspace.add_terminal_tab(false, ctx);
            workspace.add_terminal_tab(false, ctx);
        });

        source.read(&app, |_, ctx| {
            let label = destination_label(TabWindowDestination::Window(target_window), ctx);
            assert!(label.ends_with(" +2"), "unexpected label {label:?}");
        });
    });
}

#[test]
fn shorten_keeps_the_end_of_long_names() {
    assert_eq!(shorten("~/lab", 8), "~/lab");
    assert_eq!(shorten("~/lab/madness/shellomancy", 12), "…shellomancy");
    assert_eq!(shorten("~/lab/madness/shellomancy", 12).chars().count(), 12);
}
