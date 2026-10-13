//! The Setup modal: one row per [`preflight::Check`], each with a single result.
//!
//! **Check, install, result.** Each row is checked once when Setup opens and shows either ✓ or
//! the one button that fixes it. Pressing it shows progress in the row and ends in ✓, or in ✗
//! with a plain reason and Retry. A successful fix is not followed by a re-check — see
//! [`Workbench::start_fix`] — and its output is never shown; a failed one keeps its output
//! behind "Show details".
//!
//! **Nothing can be pressed mid-install.** While a fix or a check is in flight
//! ([`Workbench::setup_busy`]) every button that would move on or start something else is
//! disabled, so Setup cannot be left half-done.
#![allow(unused_imports)]

use crate::*;
use crate::ui::{common::*, modals::*};
use gpui::{
    div, prelude::*, px, rgb, ClipboardItem, Context, IntoElement, SharedString,
};

/// What a row is showing right now. Derived on every render from the report and the fix in
/// flight, never stored, so it cannot drift from either.
#[derive(Clone, Copy, PartialEq)]
enum RowState {
    InProgress,
    Failed,
    RestartRequired,
    Done,
    NeedsAction,
    Skipped,
}

impl Workbench {
    /// The onboarding modal itself: frame, scrolling body, pinned actions and footer.
    pub(crate) fn onboarding_modal(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        ui::Modal::new("onboarding", "GET STARTED")
            .focus(&self.onboarding_focus)
            .width(560.)
            .body(self.onboarding_body(cx))
            .actions(self.onboarding_actions(cx))
            .footer(self.onboarding_footer(cx))
            .into_any_element()
    }

    /// The scrolling part: the machine summary, then one row per check.
    fn onboarding_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut pane = div().flex().flex_col().w_full().min_w_0().gap_3();

        // Said out loud, because it is invisible and load-bearing: a backend already running
        // at launch may be an older copy of the app's Python overlay.
        if self.backend_start == Some(backend::Started::Attached) {
            pane = pane.child(
                div()
                    .w_full()
                    .min_w_0()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(theme::warning()))
                    .text_color(rgb(theme::warning()))
                    .text_xs()
                    .child(
                        "This backend was already running when the app started, so it may be \
                         running an older version of the app's Python overlay. If something \
                         new does nothing, restart it below.",
                    ),
            );
        }

        let Some(report) = &self.report else {
            return pane.child(
                div()
                    .text_color(rgb(theme::text_muted()))
                    .text_sm()
                    .child("Checking this machine…"),
            );
        };

        pane = pane.child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_color(rgb(if report.ready() {
                            theme::text_muted()
                        } else {
                            theme::error()
                        }))
                        .text_sm()
                        .child(if self.checking {
                            "Checking…".to_string()
                        } else if report.ready() {
                            format!("Ready to run · {}", report.summary())
                        } else {
                            format!("Not ready yet · {}", report.summary())
                        }),
                )
                .child(
                    div()
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        .child(format!("{} · {}", report.location, report.execution)),
                )
                .child(div().text_color(rgb(theme::text_muted())).text_xs().child(
                    if report.owned {
                        "Installed and maintained by this app."
                    } else {
                        "Your own checkout — the app runs it but never modifies it."
                    },
                )),
        );

        let checks = report.checks.clone();
        for check in &checks {
            pane = pane.child(self.onboarding_step_row(check, cx));
        }

        pane
    }

    /// The fix in flight or last finished for this row, if any.
    fn row_fix(&self, check_id: &str) -> Option<&RunningFix> {
        self.running_fix.as_ref().filter(|fix| fix.check_id == check_id)
    }

    fn row_state(&self, check: &preflight::Check) -> RowState {
        if let Some(fix) = self.row_fix(check.id) {
            if !fix.done {
                return RowState::InProgress;
            }
            if !fix.ok {
                return RowState::Failed;
            }
        }
        if check.id == "runtime" && self.restart_required {
            return RowState::RestartRequired;
        }
        match check.state {
            // A warning never blocks a turn (`Report::ready`), so it reads as done, with its
            // detail saying what is missing.
            preflight::State::Pass | preflight::State::Warn => RowState::Done,
            preflight::State::Fail => RowState::NeedsAction,
            preflight::State::Skip => RowState::Skipped,
        }
    }

    /// One step: glyph, label, one line of detail, and the single action that applies now.
    fn onboarding_step_row(
        &self,
        check: &preflight::Check,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = self.row_state(check);
        let busy = self.setup_busy();
        let fix = self.row_fix(check.id);
        let color = match state {
            RowState::InProgress => theme::accent(),
            RowState::Failed | RowState::NeedsAction => theme::error(),
            RowState::RestartRequired => theme::warning(),
            RowState::Done if check.state == preflight::State::Warn => theme::accent(),
            RowState::Done => theme::text(),
            RowState::Skipped => theme::border(),
        };
        let glyph = match state {
            RowState::Failed | RowState::NeedsAction => "✗",
            RowState::RestartRequired => "!",
            RowState::Done => "✓",
            RowState::Skipped => "–",
            RowState::InProgress => "",
        };
        let detail = match (state, fix) {
            (RowState::InProgress, Some(fix)) => self.progress_detail(fix),
            (RowState::Failed, Some(fix)) => failure_summary(check.id, &fix.lines, fix.stopping, fix.timed_out),
            (RowState::RestartRequired, _) => {
                "Installed. Restart your computer to finish, then open Mini-Me again.".to_string()
            }
            _ => check.detail.clone(),
        };

        let mut row = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1()
            .pl_2()
            .border_l_1()
            .border_color(rgb(color))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .child(if state == RowState::InProgress {
                        ui::Spinner::new(SharedString::from(format!("spinner-{}", check.id)))
                            .into_any_element()
                    } else {
                        div()
                            .text_color(rgb(color))
                            .text_sm()
                            .child(glyph)
                            .into_any_element()
                    })
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .text_color(rgb(color))
                            .text_sm()
                            .child(check.label),
                    )
                    .when(check.optional, |header| {
                        // A turn works forever without this step (no check is optional
                        // today). Said beside the label so skipping it reads as a choice,
                        // not neglect.
                        header.child(
                            div()
                                .flex_none()
                                .px_2()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(theme::border()))
                                .text_color(rgb(theme::text_muted()))
                                .text_xs()
                                .child("Optional"),
                        )
                    }),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(if state == RowState::Failed {
                        theme::error()
                    } else {
                        theme::text_muted()
                    }))
                    .text_xs()
                    .child(detail),
            );

        match state {
            RowState::NeedsAction => {
                for action in &check.fixes {
                    match action {
                        preflight::Fix::Run { label, argv, note } => {
                            row = row
                                .child(
                                    div()
                                        .text_color(rgb(theme::text_muted()))
                                        .text_xs()
                                        .child(*note),
                                )
                                .child(
                                    ui::actions().child(
                                        ui::Button::new(SharedString::from(format!(
                                            "run-{}",
                                            check.id
                                        )))
                                        .text(*label)
                                        .style(ui::ButtonStyle::Primary)
                                        .disabled(busy)
                                        .on_click(cx.listener({
                                            let argv = argv.clone();
                                            let label = label.to_string();
                                            let check_id = check.id;
                                            move |workbench, _event, _window, cx| {
                                                workbench.start_fix(
                                                    label.clone(),
                                                    argv.clone(),
                                                    check_id,
                                                    cx,
                                                );
                                            }
                                        })),
                                    ),
                                );
                            // One action per row: the first fix is the one that makes the row
                            // done, and a second button beside it only asks the researcher to
                            // choose between things they cannot judge.
                            break;
                        }
                        preflight::Fix::Manual(instruction) => {
                            row = row.child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .text_color(rgb(theme::text_muted()))
                                    .text_xs()
                                    .child(instruction.clone()),
                            );
                        }
                        preflight::Fix::EnterKey => {
                            row = row.child(self.onboarding_key_entry(check.id, cx));
                        }
                    }
                }
            }
            RowState::InProgress => {
                if let Some(fix) = fix {
                    row = row.child(self.onboarding_fix_progress(fix, cx));
                }
            }
            RowState::Failed => {
                if let Some(fix) = fix {
                    let check_id = check.id;
                    row = row.child(
                        ui::actions()
                            .gap_2()
                            .child(
                                ui::Button::new(SharedString::from(format!("retry-{check_id}")))
                                    .text("Retry")
                                    .style(ui::ButtonStyle::Primary)
                                    .disabled(busy)
                                    .on_click(cx.listener(move |workbench, _event, _window, cx| {
                                        workbench.retry_fix(check_id, cx);
                                    })),
                            )
                            .child(
                                ui::Button::new(SharedString::from(format!(
                                    "details-{check_id}"
                                )))
                                .text(if fix.show_details { "Hide details" } else { "Show details" })
                                .on_click(cx.listener(|workbench, _event, _window, cx| {
                                    if let Some(fix) = workbench.running_fix.as_mut() {
                                        fix.show_details = !fix.show_details;
                                    }
                                    cx.notify();
                                })),
                            ),
                    );
                    if fix.show_details {
                        row = row.child(self.onboarding_fix_log(fix, cx));
                    }
                }
            }
            RowState::RestartRequired | RowState::Done | RowState::Skipped => {}
        }

        if self.onboarding_skipped.contains(&check.id) && check.state != preflight::State::Pass {
            row = row.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child("Skipped — Mini-Me will ask again next time it opens"),
            );
        }

        row
    }

    /// One line saying what the running fix is doing, for its row's detail.
    fn progress_detail(&self, fix: &RunningFix) -> String {
        if fix.stopping {
            format!("{} — stopping…", fix.label)
        } else if fix.scan_sign_in && fix.link.is_some() {
            "Waiting for you to approve the sign-in in your browser…".to_string()
        } else if fix.scan_sign_in {
            "Installing Asta if needed, then starting sign-in…".to_string()
        } else {
            format!("{}…", fix.label)
        }
    }

    /// The key step's own controls: the masked API key field Settings uses, Save, and Skip.
    ///
    /// In place rather than "open Settings", because the modal covers Settings and a
    /// first-time user had no way to reach it.
    fn onboarding_key_entry(
        &self,
        check_id: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let provider = settings::provider(&self.draft.provider)
            .map_or(self.draft.provider.as_str(), |provider| provider.label);
        let mut entry = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_2()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(format!(
                        "Paste your {provider} API key — it goes into this computer's keychain, \
                         never into a file. To use a different provider, change it in Settings \
                         afterwards."
                    )),
            );
        if let Some((_, composer)) = self.fields.iter().find(|(field, _)| *field == Field::ApiKey)
        {
            let testing = matches!(self.key_test, Some(KeyTest::Running));
            let busy = self.setup_busy();
            entry = entry.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .p_2()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(theme::border()))
                            .track_focus(&composer.focus_handle(cx))
                            .in_focus(|style| style.border_color(rgb(theme::accent())))
                            .child(composer.clone()),
                    )
                    .child(
                        div().flex_none().child(
                            ui::Button::new("onboarding-test-key")
                                .text(if testing { "Testing…" } else { "Test key" })
                                .disabled(testing || busy)
                                .on_click(cx.listener(|workbench, _event, _window, cx| {
                                    workbench.test_onboarding_key(cx);
                                })),
                        ),
                    ),
            );
        }
        // Only while it still describes the key and provider on screen.
        let current = self.key_test_is_current(cx);
        if let Some(result) = self.key_test.as_ref().filter(|_| current) {
            let (text, colour) = match result {
                KeyTest::Running => ("asking the provider…".to_string(), theme::text_muted()),
                KeyTest::Passed(message) => (format!("✓ {message}"), theme::text()),
                KeyTest::Failed(message) => (format!("✗ {message}"), theme::error()),
            };
            entry = entry.child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(colour))
                    .text_xs()
                    .child(text),
            );
        }
        let skipped = self.onboarding_skipped.contains(&check_id);
        entry.child(
            ui::actions()
                .gap_2()
                .child(
                    ui::Button::new("onboarding-save-key")
                        .text("Save key")
                        .style(ui::ButtonStyle::Primary)
                        .disabled(self.setup_busy())
                        .on_click(cx.listener(|workbench, _event, _window, cx| {
                            workbench.save_onboarding_key(cx);
                        })),
                )
                .when(!skipped, |actions| {
                    actions.child(
                        ui::Button::new("onboarding-skip-key").text("Skip this step").on_click(
                            cx.listener(move |workbench, _event, _window, cx| {
                                workbench.skip_onboarding_step(check_id, cx);
                            }),
                        ),
                    )
                }),
        )
    }

    /// What a running fix shows under its row: Stop, and the sign-in code if it printed one.
    /// Its output stays hidden — it only matters if the fix fails.
    fn onboarding_fix_progress(&self, fix: &RunningFix, cx: &mut Context<Self>) -> impl IntoElement {
        let mut panel = div().flex().flex_col().w_full().min_w_0().gap_2();

        if let Some(link) = &fix.link {
            if let Some(code) = device_code(link) {
                panel = panel.child(
                    div().w_full().min_w_0().text_color(rgb(theme::accent())).text_lg().child(code),
                );
            }
            panel = panel.child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        ui::Button::new("open-signin")
                            .text("Open the sign-in page")
                            .style(ui::ButtonStyle::Primary)
                            .on_click(cx.listener({
                                let link = link.clone();
                                move |workbench, _event, _window, cx| {
                                    workbench.status = match open_in_browser(&link) {
                                        Ok(()) => "opened the sign-in page in your browser".to_string(),
                                        Err(error) => format!("could not open a browser: {error}"),
                                    };
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        ui::Button::new("copy-signin").text("Copy ⧉").on_click(cx.listener({
                            let link = link.clone();
                            move |workbench, _event, _window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
                                workbench.say("sign-in link copied", cx);
                                cx.notify();
                            }
                        })),
                    ),
            );
        }

        panel.child(
            ui::actions().child(
                ui::Button::new("stop-fix")
                    .text("Stop")
                    .style(ui::ButtonStyle::Danger)
                    .disabled(fix.stopping || !fix.cancel.armed())
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.stop_fix(cx);
                    })),
            ),
        )
    }

    /// A failed fix's output, shown only on "Show details": the tail of its lines, then the
    /// app's own notes about the run.
    fn onboarding_fix_log(&self, fix: &RunningFix, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut log = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .flex_none()
            .gap_2()
            .p_2()
            .rounded_lg()
            .border_1()
            .border_color(rgb(theme::border()));

        let mut output = div()
            .id("fix-output")
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .flex_none()
            .max_h(px(200.))
            .overflow_y_scroll();
        for line in &fix.lines {
            output = output.child(
                div().w_full().min_w_0().text_color(rgb(theme::text_muted())).text_xs().child(line.clone()),
            );
        }
        if fix.lines.is_empty() {
            output = output.child(
                div().text_color(rgb(theme::text_muted())).text_xs().child(
                    // What a command that ran in a window of its own looks like — the elevated
                    // WSL install is one — as well as a genuinely silent one.
                    "Nothing to show here — check the sidecar log below, or a window the \
                     command may have opened of its own.",
                ),
            );
        }
        log = log.child(output);

        let tone = self.fix_tone(fix);
        for note in &fix.notes {
            log = log.child(
                div().w_full().min_w_0().text_color(rgb(tone)).text_xs().child(note.clone()),
            );
        }
        log
    }

    /// Get started, enabled only once every required step is done and nothing is running.
    fn onboarding_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.setup_busy();
        let installing = self.running_fix.as_ref().is_some_and(|fix| !fix.done);
        let primary = ui::Button::new("onboarding-primary").style(ui::ButtonStyle::Primary);
        let primary = match &self.report {
            None => primary.text("Checking…").disabled(true),
            Some(_) if installing => primary.text("Installing…").disabled(true),
            Some(_) if busy => primary.text("Checking…").disabled(true),
            Some(_) if self.restart_required => primary.text("Restart to finish").disabled(true),
            Some(report) if report.ready() => primary.text("Get started").on_click(cx.listener(
                |workbench, _event, _window, cx| {
                    workbench.mark_onboarded();
                    workbench.onboarding_open = false;
                    workbench.restore_focus = true;
                    cx.notify();
                },
            )),
            Some(_) => primary.text("Get started").disabled(true),
        };

        ui::actions().gap_2().child(primary).child(
            ui::Button::new("onboarding-skip")
                .text("Skip for now")
                .disabled(busy)
                .on_click(cx.listener(|workbench, _event, _window, cx| {
                    workbench.mark_onboarded();
                    workbench.onboarding_open = false;
                    workbench.restore_focus = true;
                    cx.notify();
                })),
        )
    }

    /// The muted line under the actions: log paths, and the escape hatch of last resort.
    fn onboarding_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .gap_1()
            .child(
                ui::Button::new("onboarding-restart-backend")
                    .text("Restart backend")
                    .disabled(self.setup_busy())
                    .on_click(
                        cx.listener(|workbench, _event, _window, cx| workbench.restart_backend(cx)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(format!("Sidecar log: {}", self.sidecar.log_path())),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(format!("App log: {}", app_log_path().display())),
            )
            // A third, and the one hardest to guess at: the update helper runs *after* this
            // app has exited, so when a swap goes wrong there is nothing else left to have
            // written anything down. Listing it here is the difference between a diagnosable
            // failure and a researcher whose app did not come back.
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    .child(format!(
                        "Update log: {}",
                        std::env::temp_dir().join("mini-me-desktop-update.log").display()
                    )),
            )
    }
}
