use warp::settings::AISettings;
use warp::tui_export::{
    ImageContext, ParsedSlashCommandInput, PendingAttachment, SlashCommandDataSource as _,
    register_tui_session_view_test_singletons, slash_commands,
};
use warp_core::features::FeatureFlag;
use warp_core::settings::Setting as _;
use warp_editor::model::CoreEditorModel;
use warpui::platform::WindowStyle;
use warpui::{
    AddWindowOptions, App, SingletonEntity, TuiView as _, ViewHandle, WindowInvalidation,
};
use warpui_core::elements::tui::{TuiBuffer, TuiBufferExt, TuiRect};
use warpui_core::presenter::tui::TuiPresenter;

use super::TuiTerminalSessionView;
use crate::autoupdate::TuiAutoupdater;
use crate::orchestration_model::TuiOrchestrationModel;
use crate::root_view::RootTuiView;
use crate::session_registry::TuiSessions;
use crate::test_fixtures::{add_test_semantic_selection, add_test_terminal_session};

struct Fixture {
    view: ViewHandle<TuiTerminalSessionView>,
    window_id: warpui_core::WindowId,
}

fn fixture(app: &mut App) -> Fixture {
    register_tui_session_view_test_singletons(app);
    add_test_semantic_selection(app);
    app.update(TuiAutoupdater::register);
    app.update(crate::keybindings::init);
    let (window_id, _) = app.update(|ctx| {
        ctx.add_tui_window(
            AddWindowOptions {
                window_style: WindowStyle::NotStealFocus,
                ..Default::default()
            },
            RootTuiView::new,
        )
    });
    let sessions = app.add_singleton_model(|_| TuiSessions::new_for_test());
    let orchestration = app.update(TuiOrchestrationModel::register);
    app.update(|ctx| TuiSessions::wire_orchestration(&sessions, &orchestration, ctx));
    let (view, manager) = add_test_terminal_session(app, window_id);
    app.update(|ctx| {
        TuiSessions::register_session(&sessions, view.clone(), manager, true, ctx);
    });
    Fixture { view, window_id }
}

fn compose_unavailable_handoff(app: &mut App, fixture: &Fixture, text: &str) {
    fixture.view.update(app, |view, ctx| {
        view.input_view.update(ctx, |input, ctx| {
            input.set_text(text, ctx);
        });
        ctx.focus(&view.input_view);
        assert!(!matches!(
            view.slash_commands_source
                .as_ref(ctx)
                .parse_input(text, ctx),
            ParsedSlashCommandInput::SlashCommand(_)
        ));
        assert!(view.active_handoff(ctx).is_none());
    });
}

fn input_text(app: &App, fixture: &Fixture) -> String {
    fixture.view.read(app, |view, ctx| {
        view.input_view
            .as_ref(ctx)
            .model()
            .as_ref(ctx)
            .content()
            .as_ref(ctx)
            .text()
            .into_string()
    })
}

fn render_session(app: &mut App, fixture: &Fixture) -> TuiBuffer {
    let mut presenter = TuiPresenter::new();
    app.update(|ctx| {
        let mut invalidation = WindowInvalidation::default();
        invalidation.updated.insert(fixture.view.id());
        let session = fixture.view.as_ref(ctx);
        invalidation.updated.extend(session.child_view_ids(ctx));
        invalidation
            .updated
            .extend(session.transcript.as_ref(ctx).child_view_ids(ctx));
        if let Some(handoff) = session.active_handoff(ctx) {
            invalidation
                .updated
                .extend(handoff.as_ref(ctx).child_view_ids(ctx));
        }
        presenter.invalidate(&invalidation, ctx, fixture.window_id);
        presenter
            .present(ctx, &fixture.view, TuiRect::new(0, 0, 100, 40))
            .buffer
    })
}

#[test]
fn slash_menu_selection_inserts_handoff_for_optional_prompt_composition() {
    let _oz_handoff = FeatureFlag::OzHandoff.override_enabled(true);
    let _local_cloud = FeatureFlag::HandoffLocalCloud.override_enabled(true);
    App::test((), |mut app| async move {
        let fixture = fixture(&mut app);
        fixture.view.update(&mut app, |view, ctx| {
            view.select_tui_slash_command(&slash_commands::MOVE_TO_CLOUD, ctx);
        });
        assert_eq!(input_text(&app, &fixture), "/handoff ");
    });
}

#[test]
fn disabled_handoff_preserves_the_full_draft_and_images_without_a_card() {
    let _oz_handoff = FeatureFlag::OzHandoff.override_enabled(true);
    let _local_cloud = FeatureFlag::HandoffLocalCloud.override_enabled(true);
    App::test((), |mut app| async move {
        let fixture = fixture(&mut app);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .should_force_disable_cloud_handoff
                .set_value(true, ctx)
                .expect("test setting persists");
        });
        fixture.view.update(&mut app, |view, ctx| {
            view.ai_context_model.update(ctx, |context, ctx| {
                context.append_pending_attachments(
                    vec![PendingAttachment::Image(ImageContext {
                        data: "aW1hZ2U=".to_owned(),
                        mime_type: "image/png".to_owned(),
                        file_name: "context.png".to_owned(),
                        is_figma: false,
                    })],
                    ctx,
                );
            });
        });
        compose_unavailable_handoff(&mut app, &fixture, "/handoff finish the task");
        fixture.view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(
                &slash_commands::MOVE_TO_CLOUD,
                Some(&"finish the task".to_owned()),
                ctx,
            );
        });

        assert_eq!(input_text(&app, &fixture), "/handoff finish the task");
        fixture.view.read(&app, |view, ctx| {
            let attachments = view.ai_context_model.as_ref(ctx).pending_attachments();
            assert_eq!(attachments.len(), 1);
            assert!(matches!(
                &attachments[0],
                PendingAttachment::Image(image)
                    if image.data == "aW1hZ2U=" && image.file_name == "context.png"
            ));
            assert!(view.active_handoff(ctx).is_none());
            assert_eq!(
                view.transient_hint.current().map(|(text, _)| text),
                Some("Cloud handoff is unavailable."),
            );
            assert!(
                !view
                    .session_state(ctx)
                    .expect("session state resolves")
                    .has_blocking_interaction()
            );
        });
        let rendered = render_session(&mut app, &fixture).to_lines().join("\n");
        assert!(rendered.contains("/handoff finish the task"), "{rendered}");
        assert!(!rendered.contains("Hand off to cloud"), "{rendered}");
        assert!(
            !rendered.contains("A cloud environment is required"),
            "{rendered}"
        );
    });
}

#[test]
fn settings_invalidation_and_repeated_handoff_rejection_preserve_the_draft() {
    let _oz_handoff = FeatureFlag::OzHandoff.override_enabled(true);
    let _local_cloud = FeatureFlag::HandoffLocalCloud.override_enabled(true);
    App::test((), |mut app| async move {
        let fixture = fixture(&mut app);
        compose_unavailable_handoff(&mut app, &fixture, "/handoff preserve me");
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .should_force_disable_cloud_handoff
                .set_value(true, ctx)
                .expect("test setting persists");
        });
        assert_eq!(input_text(&app, &fixture), "/handoff preserve me");
        fixture.view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(
                &slash_commands::MOVE_TO_CLOUD,
                Some(&"preserve me".to_owned()),
                ctx,
            );
            view.execute_tui_slash_command(
                &slash_commands::MOVE_TO_CLOUD,
                Some(&"second".to_owned()),
                ctx,
            );
        });
        assert_eq!(input_text(&app, &fixture), "/handoff preserve me");
        fixture.view.read(&app, |view, ctx| {
            assert!(view.active_handoff(ctx).is_none());
            assert_eq!(
                view.transient_hint.current().map(|(text, _)| text),
                Some("Cloud handoff is unavailable."),
            );
            assert!(!matches!(
                view.slash_commands_source
                    .as_ref(ctx)
                    .parse_input("/handoff another", ctx),
                ParsedSlashCommandInput::SlashCommand(_)
            ));
        });
    });
}

#[test]
fn privacy_invalidation_preserves_the_draft_and_keeps_handoff_unavailable() {
    let _oz_handoff = FeatureFlag::OzHandoff.override_enabled(true);
    let _local_cloud = FeatureFlag::HandoffLocalCloud.override_enabled(true);
    App::test((), |mut app| async move {
        let fixture = fixture(&mut app);
        compose_unavailable_handoff(&mut app, &fixture, "/handoff preserve privacy draft");

        warp::settings::PrivacySettings::handle(&app).update(&mut app, |privacy_settings, ctx| {
            privacy_settings.is_cloud_conversation_storage_enabled = false;
            ctx.emit(
                warp::settings::PrivacySettingsChangedEvent::UpdateIsCloudConversationStorageEnabled {
                    old_value: true,
                    new_value: false,
                },
            );
        });

        fixture.view.update(&mut app, |view, ctx| {
            view.execute_tui_slash_command(
                &slash_commands::MOVE_TO_CLOUD,
                Some(&"preserve privacy draft".to_owned()),
                ctx,
            );
        });
        assert_eq!(
            input_text(&app, &fixture),
            "/handoff preserve privacy draft"
        );
        fixture.view.read(&app, |view, ctx| {
            assert!(view.active_handoff(ctx).is_none());
            assert!(!matches!(
                view.slash_commands_source
                    .as_ref(ctx)
                    .parse_input("/handoff another", ctx),
                ParsedSlashCommandInput::SlashCommand(_)
            ));
        });
    });
}

#[test]
fn long_running_command_rejection_preserves_the_full_local_draft() {
    let _oz_handoff = FeatureFlag::OzHandoff.override_enabled(true);
    let _local_cloud = FeatureFlag::HandoffLocalCloud.override_enabled(true);
    App::test((), |mut app| async move {
        let fixture = fixture(&mut app);
        fixture.view.update(&mut app, |view, ctx| {
            view.terminal_model
                .lock()
                .simulate_long_running_block("sleep 30", "");
            view.input_view.update(ctx, |input, ctx| {
                input.set_text("/handoff keep this prompt", ctx);
            });
            view.execute_tui_slash_command(
                &slash_commands::MOVE_TO_CLOUD,
                Some(&"keep this prompt".to_owned()),
                ctx,
            );
        });
        assert_eq!(input_text(&app, &fixture), "/handoff keep this prompt");
        fixture.view.read(&app, |view, ctx| {
            assert!(view.active_handoff(ctx).is_none());
            assert!(
                view.transient_hint
                    .current()
                    .is_some_and(|(message, _)| message.contains("command is running"))
            );
        });
    });
}
