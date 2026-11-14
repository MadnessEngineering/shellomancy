//! Shellomancy: the Code Maps section at the top of the code page, where
//! Warp's codebase indexing (which needs Warp's servers) used to be.

use warp_util::path::user_friendly_path;
use warpui::elements::{
    Container, CrossAxisAlignment, Element, Flex, MainAxisAlignment, MainAxisSize,
    MouseStateHandle, ParentElement, Shrinkable,
};
use warpui::fonts::Weight;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::ui_components::switch::SwitchStateHandle;
use warpui::{AppContext, SingletonEntity};

use super::code_indexing_page::CodeIndexingPageAction;
use super::settings_page::render_body_item;
use super::{LocalOnlyIconState, ToggleState};
use crate::appearance::Appearance;
use crate::code_maps::{CodeMaps, MappedFolder, UploadOutcome};
use crate::madness_account::{MadnessAccount, MadnessAccountState};
use crate::settings::code_maps::CodeMapsSettings;

#[derive(Clone, Default)]
pub(super) struct CodeMapsRowMouseStates {
    map_now: MouseStateHandle,
    forget: MouseStateHandle,
    open: MouseStateHandle,
}

#[derive(Clone, Default)]
pub(super) struct CodeMapsMouseStates {
    pub map_folder: MouseStateHandle,
    pub upload_switch: SwitchStateHandle,
    pub auto_remap_switch: SwitchStateHandle,
    pub rows: Vec<CodeMapsRowMouseStates>,
}

impl CodeMapsMouseStates {
    pub fn resize(&mut self, app: &AppContext) {
        let count = CodeMaps::as_ref(app).folders().len();
        if self.rows.len() != count {
            self.rows.resize_with(count, Default::default);
        }
    }
}

pub(super) fn render_code_maps(
    mouse: &CodeMapsMouseStates,
    appearance: &Appearance,
    app: &AppContext,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui = appearance.ui_builder();
    let muted = UiComponentStyles {
        font_color: Some(theme.sub_text_color(theme.background()).into_solid()),
        ..Default::default()
    };

    let mut column = Flex::column();
    column.add_child(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Shrinkable::new(
                    1.,
                    ui.wrappable_text(madness_themes::t("shellomancy.codeMaps.description"), true)
                        .with_style(UiComponentStyles {
                            margin: Some(Coords::default().right(16.)),
                            ..muted.clone()
                        })
                        .build()
                        .finish(),
                )
                .finish(),
            )
            .with_child(
                ui.button(ButtonVariant::Accent, mouse.map_folder.clone())
                    .with_text_label(madness_themes::t("shellomancy.codeMaps.mapFolder").to_owned())
                    .build()
                    .on_click(|ctx, _, _| {
                        ctx.dispatch_typed_action(CodeIndexingPageAction::CodeMapsAddFolder);
                    })
                    .finish(),
            )
            .finish(),
    );

    let signed_in = matches!(
        MadnessAccount::as_ref(app).state(),
        MadnessAccountState::SignedIn { .. }
    );
    let upload_enabled = *CodeMapsSettings::as_ref(app).upload_enabled;
    let upload_description = if signed_in {
        madness_themes::t("shellomancy.codeMaps.uploadDescription")
    } else {
        madness_themes::t("shellomancy.codeMaps.uploadSignedOut")
    };
    column.add_child(
        Container::new(render_body_item::<CodeIndexingPageAction>(
            madness_themes::t("shellomancy.codeMaps.uploadLabel").to_owned(),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            ui.switch(mouse.upload_switch.clone())
                .check(upload_enabled)
                .build()
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(CodeIndexingPageAction::CodeMapsToggleUpload);
                })
                .finish(),
            Some(upload_description.to_owned()),
        ))
        .with_margin_top(16.)
        .finish(),
    );
    column.add_child(render_body_item::<CodeIndexingPageAction>(
        madness_themes::t("shellomancy.codeMaps.autoRemapLabel").to_owned(),
        None,
        LocalOnlyIconState::Hidden,
        ToggleState::Enabled,
        appearance,
        ui.switch(mouse.auto_remap_switch.clone())
            .check(*CodeMapsSettings::as_ref(app).auto_remap_enabled)
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(CodeIndexingPageAction::CodeMapsToggleAutoRemap);
            })
            .finish(),
        Some(madness_themes::t("shellomancy.codeMaps.autoRemapDescription").to_owned()),
    ));

    let code_maps = CodeMaps::as_ref(app);
    let folders = code_maps.folders();
    if folders.is_empty() {
        column.add_child(
            ui.wrappable_text(madness_themes::t("shellomancy.codeMaps.empty"), true)
                .with_style(muted)
                .build()
                .finish(),
        );
    }
    let home_dir = dirs::home_dir().and_then(|home| home.to_str().map(str::to_owned));
    for (folder, row_mouse) in folders.iter().zip(&mouse.rows) {
        column.add_child(render_folder(
            folder,
            code_maps.is_mapping(&folder.path),
            home_dir.as_deref(),
            row_mouse,
            appearance,
            app,
        ));
    }

    Container::new(column.finish())
        .with_margin_bottom(24.)
        .finish()
}

fn render_folder(
    folder: &MappedFolder,
    mapping: bool,
    home_dir: Option<&str>,
    mouse: &CodeMapsRowMouseStates,
    appearance: &Appearance,
    app: &AppContext,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let ui = appearance.ui_builder();
    let muted = UiComponentStyles {
        font_color: Some(theme.sub_text_color(theme.background()).into_solid()),
        ..Default::default()
    };
    let path = folder.path.to_string_lossy();
    let mut shown_path = user_friendly_path(path.as_ref(), home_dir).into_owned();
    if let Some(git) = &folder.git
        && *CodeMapsSettings::as_ref(app).auto_remap_enabled
    {
        shown_path.push_str(" · ");
        shown_path.push_str(&madness_themes::tf(
            "shellomancy.codeMaps.watching",
            &[("branch", &git.default_branch)],
        ));
    }

    let mut details = Flex::column()
        .with_child(
            ui.wrappable_text(folder.name.clone(), true)
                .with_style(UiComponentStyles {
                    font_weight: Some(Weight::Semibold),
                    ..Default::default()
                })
                .build()
                .finish(),
        )
        .with_child(
            ui.wrappable_text(shown_path, true)
                .with_style(muted.clone())
                .build()
                .finish(),
        );
    if let Some(summary) = &folder.summary {
        details.add_child(
            ui.wrappable_text(
                madness_themes::tf(
                    "shellomancy.codeMaps.summary",
                    &[
                        ("classes", &summary.classes.to_string()),
                        ("packages", &summary.packages.to_string()),
                        ("critical", &summary.critical.to_string()),
                        ("high", &summary.high.to_string()),
                        ("unused", &summary.unused_exports.to_string()),
                    ],
                ),
                true,
            )
            .build()
            .finish(),
        );
    }
    let status = if mapping {
        Some(madness_themes::t("shellomancy.codeMaps.mapping").to_owned())
    } else {
        folder
            .mapped_at
            .map(|at| status_line(at, folder.upload.as_ref()))
    };
    if let Some(status) = status {
        details.add_child(
            ui.wrappable_text(status, true)
                .with_style(muted.clone())
                .build()
                .finish(),
        );
    }
    if let (Some(error), false) = (&folder.error, mapping) {
        details.add_child(
            ui.wrappable_text(error.clone(), true)
                .with_style(UiComponentStyles {
                    font_color: Some(theme.ui_error_color()),
                    ..Default::default()
                })
                .build()
                .finish(),
        );
    }
    if let Some(UploadOutcome::Uploaded {
        view_url: Some(url),
        ..
    }) = &folder.upload
    {
        let url = url.clone();
        details.add_child(
            ui.link(
                madness_themes::t("shellomancy.codeMaps.openInInventorium").to_owned(),
                None,
                Some(Box::new(move |ctx| {
                    ctx.dispatch_typed_action(CodeIndexingPageAction::CodeMapsOpenUrl(url.clone()));
                })),
                mouse.open.clone(),
            )
            .build()
            .finish(),
        );
    }

    let map_path = folder.path.clone();
    let forget_path = folder.path.clone();
    let mut buttons = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
    if !mapping {
        buttons.add_child(
            Container::new(
                ui.button(ButtonVariant::Secondary, mouse.map_now.clone())
                    .with_text_label(madness_themes::t("shellomancy.codeMaps.mapNow").to_owned())
                    .build()
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(CodeIndexingPageAction::CodeMapsMapNow(
                            map_path.clone(),
                        ));
                    })
                    .finish(),
            )
            .with_margin_right(8.)
            .finish(),
        );
    }
    buttons.add_child(
        ui.button(ButtonVariant::Secondary, mouse.forget.clone())
            .with_text_label(madness_themes::t("shellomancy.codeMaps.forget").to_owned())
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(CodeIndexingPageAction::CodeMapsForget(
                    forget_path.clone(),
                ));
            })
            .finish(),
    );

    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(Shrinkable::new(1., details.finish()).finish())
            .with_child(buttons.finish())
            .finish(),
    )
    .with_margin_top(16.)
    .finish()
}

/// "Mapped Nov 13, 14:02 · sent to Inventorium as version 7"
fn status_line(mapped_at: i64, upload: Option<&UploadOutcome>) -> String {
    let when = chrono::DateTime::from_timestamp(mapped_at, 0)
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%b %-d, %H:%M")
                .to_string()
        })
        .unwrap_or_default();
    let mapped = madness_themes::tf("shellomancy.codeMaps.mappedAt", &[("when", &when)]);
    let upload = match upload {
        Some(UploadOutcome::Uploaded { version, .. }) => madness_themes::tf(
            "shellomancy.codeMaps.uploaded",
            &[("version", &version.to_string())],
        ),
        Some(UploadOutcome::Failed { error }) => {
            madness_themes::tf("shellomancy.codeMaps.uploadFailed", &[("error", error)])
        }
        Some(UploadOutcome::KeptLocal) | None => {
            madness_themes::t("shellomancy.codeMaps.keptLocal").to_owned()
        }
    };
    format!("{mapped} · {upload}")
}
