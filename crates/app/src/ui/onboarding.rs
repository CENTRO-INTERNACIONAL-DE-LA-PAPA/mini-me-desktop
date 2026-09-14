//! The onboarding modal: a step-by-step replacement for the old flat Setup checklist.
//!
//! **One button, not one button per row.** The Setup pane made a first-time researcher find
//! and click every failing check's fix in turn; this walks the same list of
//! [`preflight::Check`]s automatically from a single "Get Started" press, stopping only where
//! a person genuinely has to act (a sign-in, an adoption, a manual step) — see
//! [`Workbench::onboarding_advance`] in `main.rs` for the sequencer this renders.
//!
//! **Log in place, not log at the bottom.** The old pane kept one scrolling output box below
//! the whole list, disconnected from whichever row it belonged to. Here the currently-running
//! (or last-failed) step's log renders directly under its own row, and a step that has moved
//! on collapses to a one-line verdict — see `Workbench::onboarding_history`.
#![allow(unused_imports)]

use crate::*;
use crate::ui::{common::*, modals::*};
use gpui::{
    div, prelude::*, px, rgb, ClipboardItem, Context, IntoElement, SharedString,
};

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
                            "Re-checking…".to_string()
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

    /// One step: glyph, label, detail, and whatever belongs under it right now — the live
    /// log if it is the active row, a collapsed verdict if it already finished and moved on,
    /// a manual/adopt prompt if the sequence is waiting on the user, or nothing at all once
    /// it has passed and been forgotten.
    fn onboarding_step_row(
        &self,
        check: &preflight::Check,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // A fix that reported success against *this* check, on a check that still isn't
        // passing, is not a failure — it's `judge_finished_fix`'s "installed, but Windows
        // needs a restart" case. Left as a plain `Fail` this drew a red ✗ with an "Install
        // Ubuntu" button sitting right above a log that already explained the real state,
        // which read as the app being confused about its own result.
        let pending_restart = check.state != preflight::State::Pass
            && self
                .running_fix
                .as_ref()
                .is_some_and(|fix| fix.check_id == check.id && fix.done && fix.ok);
        let color = if pending_restart {
            theme::warning()
        } else {
            match check.state {
                preflight::State::Pass => theme::text_muted(),
                preflight::State::Warn => theme::accent(),
                preflight::State::Fail => theme::error(),
                preflight::State::Skip => theme::border(),
            }
        };
        let is_active = self.onboarding_step == Some(check.id);
        let running = is_active
            && self.running_fix.as_ref().is_some_and(|fix| fix.check_id == check.id);

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
                    .child(if running && self.running_fix.as_ref().is_some_and(|fix| !fix.done) {
                        ui::Spinner::new(SharedString::from(format!("spinner-{}", check.id)))
                            .into_any_element()
                    } else {
                        div()
                            .text_color(rgb(if check.state == preflight::State::Pass {
                                theme::text()
                            } else {
                                color
                            }))
                            .text_sm()
                            // Not `check.state.glyph()` here: that would print the ✗ of an
                            // ordinary failure, and this row is the opposite of one — the
                            // fix already succeeded, and a restart is the only thing left.
                            .child(if pending_restart { "!" } else { check.state.glyph() })
                            .into_any_element()
                    })
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .text_color(rgb(if check.state == preflight::State::Pass {
                                theme::text()
                            } else {
                                color
                            }))
                            .text_sm()
                            .child(check.label),
                    )
                    .when(check.optional, |header| {
                        // A turn works forever without this step — Asta and CIP Dataverse,
                        // never anything a Fail-capable check would need to borrow. Said
                        // beside the label so skipping it reads as a choice, not neglect.
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
                    .text_color(rgb(theme::text_muted()))
                    .text_xs()
                    // `check.detail` still reads "isn't responding" here — true of the probe,
                    // but read by a person as "it didn't install," which is the opposite of
                    // what happened. Restate it as the plain fact: installed, restart pending.
                    .child(if pending_restart {
                        "Installed — restart your computer, then reopen Mini-Me for this to \
                         turn green."
                            .to_string()
                    } else {
                        check.detail.clone()
                    }),
            );

        // A step that already ran and is no longer the one being driven: one line, not the
        // full log it printed.
        if !is_active {
            if let Some(result) = self
                .onboarding_history
                .iter()
                .find(|result| result.check_id == check.id)
            {
                row = row.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_color(rgb(theme::text_muted()))
                        .text_xs()
                        .child(format!(
                            "{} — {}",
                            result.label,
                            if result.ok { "done" } else { "failed" }
                        )),
                );
                return row;
            }
        }

        // Waiting on the user: a `Fix::Manual`/`Fix::Adopt` step the auto-run reached and
        // paused on, or the same rendered before the sequence has started at all.
        //
        // Not when `pending_restart`, though — the auto-run parks there with
        // `onboarding_awaiting_manual == Some(check.id)` too, on purpose (see the comment
        // in `onboarding_advance`), and re-offering "Install Ubuntu" here would sit a
        // button that re-runs an already-successful install directly above the log
        // explaining that it already worked and only a restart is left. That combination
        // is exactly what read as confusing.
        if !pending_restart
            && (self.onboarding_awaiting_manual == Some(check.id)
                || (!self.onboarding_auto_running
                    && self.running_fix.is_none()
                    && check.state != preflight::State::Pass))
        {
            for fix in &check.fixes {
                match fix {
                    preflight::Fix::Run { label, argv, note } => {
                        let command = preflight::display_argv(argv);
                        let busy = self.running_fix.as_ref().is_some_and(|fix| !fix.done);
                        row = row
                            .child(div().text_color(rgb(theme::text_muted())).text_xs().child(*note))
                            .child(
                                ui::actions()
                                    .gap_2()
                                    .child(
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
                                    )
                                    .child(
                                        ui::Button::new(SharedString::from(format!(
                                            "copy-{}",
                                            check.id
                                        )))
                                        .text("Copy ⧉")
                                        .on_click(cx.listener({
                                            let command = command.clone();
                                            move |workbench, _event, _window, cx| {
                                                cx.write_to_clipboard(ClipboardItem::new_string(
                                                    command.clone(),
                                                ));
                                                workbench.say("command copied", cx);
                                                cx.notify();
                                            }
                                        })),
                                    ),
                            );
                    }
                    preflight::Fix::Adopt { label, dir } => {
                        row = row
                            .child(
                                div()
                                    .text_color(rgb(theme::text_muted()))
                                    .text_xs()
                                    .child(
                                        "this step needs you — adopt the checkout you already \
                                         have, then press Continue below",
                                    ),
                            )
                            .child(
                                ui::Button::new(SharedString::from(format!(
                                    "adopt-{}",
                                    check.id
                                )))
                                .text(*label)
                                .style(ui::ButtonStyle::Primary)
                                .on_click(cx.listener({
                                    let dir = dir.clone();
                                    move |workbench, _event, _window, cx| {
                                        workbench.adopt_checkout(dir.clone(), cx);
                                    }
                                })),
                            );
                    }
                    preflight::Fix::Manual(instruction) => {
                        row = row.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_color(rgb(theme::text_muted()))
                                .text_xs()
                                .child(format!(
                                    "{instruction} — this step needs you; press Continue below \
                                     once it's done"
                                )),
                        );
                    }
                }
            }
        }

        // The active row's live output — the same box the old Setup pane showed at the
        // bottom, now anchored under the step it belongs to.
        if running {
            if let Some(fix) = &self.running_fix {
                row = row.child(self.onboarding_fix_log(fix, cx));
            }
        }

        row
    }

    /// The live/finished output of whichever fix is running — Stop, sign-in link, the
    /// scrolling tail of its lines, and the app's own verdict below them.
    fn onboarding_fix_log(&self, fix: &RunningFix, cx: &mut Context<Self>) -> impl IntoElement {
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
            .border_color(rgb(if !fix.done {
                theme::accent()
            } else if fix.ok {
                theme::border()
            } else {
                theme::error()
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w_full()
                    .min_w_0()
                    .child(
                        div()
                            .flex_grow()
                            .min_w_0()
                            .text_color(rgb(theme::text()))
                            .text_sm()
                            .child(if fix.done {
                                format!("{} — {}", fix.label, if fix.ok { "done" } else { "failed" })
                            } else if fix.stopping {
                                format!("{} — stopping…", fix.label)
                            } else {
                                format!("{}…", fix.label)
                            }),
                    )
                    .when(!fix.done, |header| {
                        header.child(
                            ui::Button::new("stop-fix")
                                .text("Stop")
                                .style(ui::ButtonStyle::Danger)
                                .disabled(fix.stopping || !fix.cancel.armed())
                                .on_click(cx.listener(|workbench, _event, _window, cx| {
                                    workbench.stop_fix(cx);
                                })),
                        )
                    }),
            );

        if let Some(link) = &fix.link {
            if let Some(code) = device_code(link) {
                log = log.child(
                    div().w_full().min_w_0().text_color(rgb(theme::accent())).text_lg().child(code),
                );
            }
            log = log.child(
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
                div().text_color(rgb(theme::text_muted())).text_xs().child(if fix.done {
                    // Also what a genuinely silent command looks like, but that's rare —
                    // most either print something or, like the elevated WSL install,
                    // open a real window of their own this app can never read (docs §XX).
                    "Nothing to show here — check the sidecar log below, or a window the \
                     command may have opened of its own."
                } else {
                    "starting… if this opens its own window, watch that instead"
                }),
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

    /// The primary action, whose label and handler track exactly where the sequence is.
    fn onboarding_actions(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut actions = ui::actions().gap_2();

        let primary = match &self.report {
            None => ui::Button::new("onboarding-primary").text("Checking…").disabled(true),
            // Checked before `ready()`: a Fail-free report can still be mid-sequence on an
            // *optional* step (Asta, Dataverse) the auto-run is trying anyway, and that must
            // read as "working", not as done early.
            Some(_) if self.onboarding_auto_running => {
                ui::Button::new("onboarding-primary").text("Working…").disabled(true)
            }
            // `ready()` alone, not "nothing left at all" and not "nothing waiting on you":
            // once every non-optional check passes, a stuck *optional* one (Asta needs a
            // login it can't get, Dataverse needs an account this machine doesn't have)
            // must never be the one thing standing between a researcher and "Done" — that
            // is exactly what marking it optional means. Checked ahead of the
            // Continue/Retry arms below for the same reason.
            Some(report) if report.ready() => ui::Button::new("onboarding-primary")
                .text("Done")
                .style(ui::ButtonStyle::Primary)
                .on_click(cx.listener(|workbench, _event, _window, cx| {
                    workbench.mark_onboarded();
                    workbench.onboarding_open = false;
                    workbench.restore_focus = true;
                    cx.notify();
                })),
            Some(_) if self.onboarding_awaiting_manual.is_some() => {
                ui::Button::new("onboarding-primary")
                    .text("Continue")
                    .style(ui::ButtonStyle::Primary)
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.resume_onboarding(cx);
                    }))
            }
            Some(_) if self.running_fix.as_ref().is_some_and(|fix| fix.done && !fix.ok) => {
                ui::Button::new("onboarding-primary")
                    .text("Retry")
                    .style(ui::ButtonStyle::Primary)
                    .on_click(cx.listener(|workbench, _event, _window, cx| {
                        workbench.retry_onboarding_step(cx);
                    }))
            }
            Some(_) => ui::Button::new("onboarding-primary")
                .text("Get Started")
                .style(ui::ButtonStyle::Primary)
                .on_click(cx.listener(|workbench, _event, _window, cx| {
                    workbench.start_onboarding(cx);
                })),
        };
        actions = actions.child(primary);

        // Beside Retry only — once `report.ready()` the primary button is already "Done",
        // and a second button offering to re-check a failed *optional* fix would be
        // redundant with it.
        let not_ready = self.report.as_ref().is_some_and(|report| !report.ready());
        if not_ready && self.running_fix.as_ref().is_some_and(|fix| fix.done && !fix.ok) {
            actions = actions.child(
                ui::Button::new("onboarding-recheck").text("Re-check instead").on_click(
                    cx.listener(|workbench, _event, _window, cx| workbench.run_preflight(cx)),
                ),
            );
        }

        actions.child(
            ui::Button::new("onboarding-skip").text("Skip for now").on_click(cx.listener(
                |workbench, _event, _window, cx| {
                    workbench.onboarding_auto_running = false;
                    workbench.mark_onboarded();
                    workbench.onboarding_open = false;
                    workbench.restore_focus = true;
                    cx.notify();
                },
            )),
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
                ui::Button::new("onboarding-restart-backend").text("Restart backend").on_click(
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
