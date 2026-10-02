//! A floating window over a dimmed workbench: header, a scrolling middle, pinned actions.

use gpui::{div, prelude::*, rgb, App, Div, IntoElement, SharedString, Window};

use crate::theme;

/// A row of actions that must stay put while the thing above it scrolls.
///
/// Three bugs came from putting the buttons inside the scroll area (§40, §41, §52) — the
/// approve/reject pair scrolled out of reach twice, on two different panes, and the second
/// time the fix was already visible in the same file. `flex_none` is what pins it.
pub fn actions() -> Div {
    div()
        .flex()
        .flex_row()
        .flex_none()
        .gap_3()
        .w_full()
        .min_w_0()
}

/// # The shape is the point
///
/// Approve/Reject scrolled out of reach in §40, Save and Close did the same in §52, and both
/// times the fix was to move one `div` out of the scrolling container. Here the slots are
/// separate arguments, so the mistake has nowhere to happen: whatever goes in [`Modal::body`]
/// scrolls and whatever goes in [`Modal::actions`] is pinned, and a caller cannot swap them
/// without noticing.
///
/// `min_h_0` on the body is the other half. A flex child refuses to shrink below its content,
/// so without it a long body pushes the actions off the bottom instead of scrolling — four
/// bugs (§40, §48, §51, §53), and not something a call site should have to remember.
///
/// The optional [`Modal::nav`] rail is Zed's settings shape: sections down the left, the chosen
/// one on the right. Zed builds it from a two-level `NavBarEntry` tree; this is the same idea
/// with the tree flattened, because two levels is one more than this app has sections for.
#[derive(IntoElement, Default)]
pub struct Modal {
    id: SharedString,
    title: SharedString,
    width: f32,
    /// A fixed height, so the card keeps one size whichever page is showing. `None` sizes it to
    /// its content, up to 720px.
    height: Option<f32>,
    /// The two-panel look from the Figma "Settings Modal": the rail sits on the card's own
    /// `background()` with no title row or divider, and the page sits in its own bordered
    /// `surface()` panel beside it. The rail supplies its own heading.
    framed: bool,
    nav: Option<gpui::AnyElement>,
    /// Somewhere for the keyboard to live on a page with no field of its own.
    ///
    /// Without it, focus stays on whatever was focused before — often an element the page it
    /// belonged to no longer renders — and key bindings simply stop arriving (docs §71).
    focus: Option<gpui::FocusHandle>,
    body: Option<gpui::AnyElement>,
    actions: Option<gpui::AnyElement>,
    footer: Option<gpui::AnyElement>,
}

impl Modal {
    pub fn new(id: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            width: 520.,
            ..Default::default()
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn framed(mut self) -> Self {
        self.framed = true;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// The handle the window itself takes focus with.
    pub fn focus(mut self, handle: &gpui::FocusHandle) -> Self {
        self.focus = Some(handle.clone());
        self
    }

    /// A rail of sections down the left. Fixed width and never scrolled with the content.
    pub fn nav(mut self, nav: impl IntoElement) -> Self {
        self.nav = Some(nav.into_any_element());
        self
    }

    /// The part that scrolls. Everything else is pinned.
    pub fn body(mut self, body: impl IntoElement) -> Self {
        self.body = Some(body.into_any_element());
        self
    }

    /// Buttons along the bottom. Cannot scroll away — that is the whole reason this is a
    /// separate slot rather than the last child of `body`.
    pub fn actions(mut self, actions: impl IntoElement) -> Self {
        self.actions = Some(actions.into_any_element());
        self
    }

    /// A muted line under the actions: where files live, what a key is for.
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }
}

impl RenderOnce for Modal {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        if self.framed {
            let id = self.id.clone();
            return backdrop(&id, self.framed_card().into_any_element()).into_any_element();
        }
        let scrolling = div()
            .id(SharedString::from(format!("{}-body", self.id)))
            .flex()
            .flex_col()
            .flex_grow()
            // Without this the body pushes the actions off the bottom instead of scrolling.
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .children(self.body);

        let mut card = div()
            .when_some(self.focus, |card, handle| card.track_focus(&handle))
            .flex()
            .flex_col()
            .w(gpui::px(self.width))
            .max_h(gpui::px(720.))
            // Fixed, but never taller than a small window: the body scrolls instead.
            .when_some(self.height, |card, height| {
                card.h(gpui::px(height)).max_h(gpui::relative(0.9))
            })
            .min_h_0()
            .rounded_lg()
            .overflow_hidden()
            .bg(rgb(theme::overlay()))
            .border_1()
            .border_color(rgb(theme::border_strong()))
            .child(
                div()
                    .flex_none()
                    .px_4()
                    .pt_4()
                    // Faint, not accent. `SETTINGS` and `PROVENANCE` are labels for a surface
                    // you are already looking at — there is nothing to click. The accent is
                    // reserved for what can be acted on, and a heading wearing it is the
                    // loudest thing on a modal whose actual buttons are at the bottom.
                    .text_color(rgb(theme::text_faint()))
                    .text_xs()
                    .child(self.title),
            );

        // With a rail, the middle of the card is a row: sections left, content right.
        card = match self.nav {
            Some(nav) => card.child(
                div()
                    .flex()
                    .flex_row()
                    .flex_grow()
                    .min_h_0()
                    .min_w_0()
                    .child(nav)
                    .child(scrolling),
            ),
            None => card.child(scrolling),
        };

        for pinned in [self.actions, self.footer].into_iter().flatten() {
            card = card.child(div().flex_none().px_4().pb_3().child(pinned));
        }

        backdrop(&self.id, card.into_any_element()).into_any_element()
    }
}

impl Modal {
    /// The card for [`Modal::framed`]: rail left on `background()`, page right in a `surface()`
    /// panel, with the actions and footer pinned at the bottom of that panel.
    fn framed_card(self) -> Div {
        let scrolling = div()
            .id(SharedString::from(format!("{}-body", self.id)))
            .flex()
            .flex_col()
            .flex_grow()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            // The same padding as every other modal's body.
            .p_4()
            .gap_3()
            .children(self.body);

        let mut page = div()
            .flex()
            .flex_col()
            .flex_grow()
            .min_w_0()
            .min_h_0()
            .rounded_lg()
            .overflow_hidden()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .child(scrolling);
        // The actions belong to the whole window, not the page showing, so with a rail they sit
        // at the foot of it. Without one they stay at the foot of the page.
        let (rail_actions, page_actions) = if self.nav.is_some() {
            (self.actions, None)
        } else {
            (None, self.actions)
        };
        for pinned in [page_actions, self.footer].into_iter().flatten() {
            page = page.child(div().flex_none().px_4().pb_3().child(pinned));
        }

        div()
            .when_some(self.focus, |card, handle| card.track_focus(&handle))
            .flex()
            .flex_row()
            .gap_2p5()
            .w(gpui::px(self.width))
            .max_h(gpui::px(720.))
            .when_some(self.height, |card, height| {
                card.h(gpui::px(height)).max_h(gpui::relative(0.9))
            })
            .min_h_0()
            .p_3()
            .rounded_xl()
            .overflow_hidden()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .children(self.nav.map(|nav| {
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .justify_between()
                    .w(gpui::px(NAV_WIDTH))
                    .min_h_0()
                    // No padding of its own: the card's `p_3` is the only inset, so the rail
                    // lines up with the panel's top edge instead of sitting further in.
                    .child(div().flex_grow().min_h_0().child(nav))
                    .children(rail_actions.map(|actions| div().flex_none().pt_3().child(actions)))
            }))
            .child(page)
    }
}

/// The framed rail's width: enough for the search box and the longest page name with its icon.
const NAV_WIDTH: f32 = 190.;

/// Dimmed, so the conversation stays visible behind it and clicking away is the obvious exit.
fn backdrop(id: &SharedString, card: gpui::AnyElement) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("{}-backdrop", id)))
        .absolute()
        .inset_0()
        // **Visible behind it, not reachable behind it.** GPUI hit-tests every element whose
        // bounds contain the pointer, so an overlay that only *paints* over the workbench
        // leaves it live: a click on Settings also pressed whatever sat under that spot.
        // `occlude` blocks the mouse from everything behind this hitbox. Here rather than in
        // each caller, because every modal in the app is built from this one type and the
        // three that existed all had the defect (docs §163).
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(if theme::is_light(&theme::current()) {
            gpui::rgba(0x33333366)
        } else {
            gpui::rgba(0x00000099)
        })
        .child(card)
}
