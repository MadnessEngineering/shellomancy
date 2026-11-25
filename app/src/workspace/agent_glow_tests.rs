use warpui::{App, EntityId, ModelHandle};

use super::*;
use crate::terminal::cli_agent_sessions::BlockedSource;

fn blocked() -> CLIAgentSessionStatus {
    CLIAgentSessionStatus::Blocked {
        message: None,
        source: BlockedSource::PermissionRequest,
    }
}

fn note(
    model: &ModelHandle<AgentGlowModel>,
    app: &mut App,
    id: EntityId,
    status: CLIAgentSessionStatus,
    in_view: bool,
) {
    model.update(app, |model, ctx| {
        model.note_status(id, &status, in_view, ctx)
    });
}

#[test]
fn each_status_glows_its_own_way() {
    App::test((), |mut app| async move {
        let model = app.add_model(|_| AgentGlowModel::default());
        let [working, done, failed, waiting, cancelled] = [(); 5].map(|_| EntityId::new());
        note(
            &model,
            &mut app,
            working,
            CLIAgentSessionStatus::InProgress,
            false,
        );
        note(
            &model,
            &mut app,
            done,
            CLIAgentSessionStatus::Success,
            false,
        );
        let failure = CLIAgentSessionStatus::Failed {
            error_type: None,
            message: None,
        };
        note(&model, &mut app, failed, failure, false);
        note(&model, &mut app, waiting, blocked(), true);
        note(
            &model,
            &mut app,
            cancelled,
            CLIAgentSessionStatus::Cancelled,
            false,
        );

        model.read(&app, |model, _| {
            assert_eq!(model.glow(working), Some(TabGlow::Working));
            assert_eq!(model.glow(done), Some(TabGlow::Done));
            assert_eq!(model.glow(failed), Some(TabGlow::Failed));
            assert_eq!(model.glow(waiting), Some(TabGlow::NeedsYou));
            assert_eq!(model.glow(cancelled), None);
        });
    });
}

#[test]
fn a_turn_that_ends_in_view_is_already_seen() {
    App::test((), |mut app| async move {
        let model = app.add_model(|_| AgentGlowModel::default());
        let id = EntityId::new();
        note(
            &model,
            &mut app,
            id,
            CLIAgentSessionStatus::InProgress,
            true,
        );
        note(&model, &mut app, id, CLIAgentSessionStatus::Success, true);
        model.read(&app, |model, _| assert_eq!(model.glow(id), None));
    });
}

#[test]
fn looking_at_a_tab_clears_finished_turns_only() {
    App::test((), |mut app| async move {
        let model = app.add_model(|_| AgentGlowModel::default());
        let [done, waiting, working] = [(); 3].map(|_| EntityId::new());
        note(
            &model,
            &mut app,
            done,
            CLIAgentSessionStatus::Success,
            false,
        );
        note(&model, &mut app, waiting, blocked(), false);
        note(
            &model,
            &mut app,
            working,
            CLIAgentSessionStatus::InProgress,
            false,
        );

        model.update(&mut app, |model, ctx| {
            model.mark_seen([done, waiting, working], ctx)
        });
        model.read(&app, |model, _| {
            assert_eq!(model.glow(done), None);
            assert_eq!(model.glow(waiting), Some(TabGlow::NeedsYou));
            assert_eq!(model.glow(working), Some(TabGlow::Working));
        });

        model.update(&mut app, |model, ctx| model.forget(waiting, ctx));
        model.read(&app, |model, _| assert_eq!(model.glow(waiting), None));
    });
}

#[test]
fn needing_you_outranks_everything() {
    let mut glows = vec![
        TabGlow::Done,
        TabGlow::NeedsYou,
        TabGlow::Working,
        TabGlow::Failed,
    ];
    glows.sort();
    assert_eq!(
        glows,
        vec![
            TabGlow::Working,
            TabGlow::Done,
            TabGlow::Failed,
            TabGlow::NeedsYou
        ]
    );
}
