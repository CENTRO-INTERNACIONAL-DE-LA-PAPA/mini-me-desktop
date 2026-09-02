//! A positioned dropdown of clickable rows — the `⋮` and right-click menu shape used
//! throughout the app: a card, anchored at a point, dismissed by a click anywhere else.
//!
//! Factored out once a second caller ([`crate::ui::modals::context_menu`], beside the sidebar's
//! own `⋮` menu) needed the exact same shell: `occlude` so a row's click doesn't also land on
//! whatever the menu was drawn over, the left press swallowed so choosing a row doesn't start a
//! text selection in the transcript underneath, and a right-click's dismissal made optional,
//! since a context menu that a right-click elsewhere re-opens must not race that reopen by also
//! closing here.

use gpui::{
    anchored, deferred, div, prelude::*, px, rgb, AnyElement, App, ClickEvent, Div, ElementId,
    IntoElement, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Window,
};

use crate::theme;

/// The card every popup menu is drawn on.
///
/// One definition because the discipline is easy to omit and invisible when it is: a menu must
/// `occlude`, or a click on a row also lands on whatever the menu was drawn over (§163), and it
/// must swallow the left press, or choosing an item starts a text selection in the transcript
/// underneath. The right-click menu learned both the hard way; a second menu written from scratch
/// beside it would have learned them again.
///
/// The lower-level half of [`Menu`] — reach for `Menu` unless something needs the card's chrome
/// without its positioning and dismissal (nothing in the app currently does).
pub fn menu_card() -> Div {
    div()
        .flex()
        .flex_col()
        .min_w(px(190.))
        .py_1()
        .rounded_md()
        .bg(rgb(theme::elevated()))
        .border_1()
        .border_color(rgb(theme::border_strong()))
        .occlude()
        .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
            cx.stop_propagation();
        })
}

/// A row inside a [`Menu`]: a label, an optional trailing hint (a keyboard shortcut, say), and
/// the two states every menu here needed — a destructive item in `error()`, and a disabled one
/// that shows what it would do without letting it happen.
#[derive(IntoElement)]
pub struct MenuItem {
    id: ElementId,
    label: SharedString,
    trailing: Option<SharedString>,
    danger: bool,
    disabled: bool,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl MenuItem {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            trailing: None,
            danger: false,
            disabled: false,
            on_click: None,
        }
    }

    /// A keyboard shortcut, or any other hint shown faint and small at the row's trailing edge.
    pub fn trailing(mut self, trailing: impl Into<SharedString>) -> Self {
        self.trailing = Some(trailing.into());
        self
    }

    /// An irreversible action — Delete, Delete project — in `error()` rather than the usual text
    /// colour.
    pub fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    /// Shown, not hidden: a greyed row that says what it would do is more honest than one that
    /// simply is not there, the same argument [`super::Button::disabled`] makes.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for MenuItem {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let text_colour = if self.disabled {
            theme::text_faint()
        } else if self.danger {
            theme::error()
        } else {
            theme::text()
        };
        let mut row = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .w_full()
            .min_w_0()
            .gap_4()
            .px_3()
            .py_1()
            .text_sm()
            .text_color(rgb(text_colour))
            .child(self.label);
        if let Some(trailing) = self.trailing {
            row = row.child(
                div()
                    .text_color(rgb(theme::text_faint()))
                    .text_xs()
                    .child(trailing),
            );
        }
        if self.disabled {
            return row;
        }
        row = row.hover(|style| style.bg(rgb(theme::accent_soft())).cursor_pointer());
        match self.on_click {
            Some(handler) => row.on_click(move |event, window, cx| handler(event, window, cx)),
            None => row,
        }
    }
}

/// A [`menu_card`], anchored at a point and dismissed by a click anywhere else.
///
/// ```ignore
/// Menu::new(at)
///     .item(MenuItem::new("rename", "Rename").on_click(...))
///     .item(MenuItem::new("delete", "Delete").danger(true).on_click(...))
///     .on_dismiss(cx.listener(|workbench, _event, _window, cx| {
///         workbench.sidebar_menu = None;
///         cx.notify();
///     }))
/// ```
#[derive(IntoElement)]
pub struct Menu {
    at: Point<Pixels>,
    items: Vec<AnyElement>,
    /// A right-click elsewhere re-opens a context menu at the new spot, and that handler is the
    /// only one that should decide whether this menu closes — dismissing here too would race it,
    /// and which one won would depend on paint order, sometimes leaving no menu at all. Off by
    /// default: the sidebar's `⋮` menu has no reopen-elsewhere gesture to race, and most menus
    /// don't.
    ignore_right_click: bool,
    on_dismiss: Option<Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>>,
}

impl Menu {
    pub fn new(at: Point<Pixels>) -> Self {
        Self {
            at,
            items: Vec::new(),
            ignore_right_click: false,
            on_dismiss: None,
        }
    }

    pub fn item(mut self, item: impl IntoElement) -> Self {
        self.items.push(item.into_any_element());
        self
    }

    pub fn ignore_right_click(mut self, ignore_right_click: bool) -> Self {
        self.ignore_right_click = ignore_right_click;
        self
    }

    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Menu {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut card = menu_card();
        for item in self.items {
            card = card.child(item);
        }
        let ignore_right_click = self.ignore_right_click;
        let on_dismiss = self.on_dismiss;
        let card = card.on_mouse_down_out(move |event, window, cx| {
            if ignore_right_click && event.button == MouseButton::Right {
                return;
            }
            if let Some(handler) = &on_dismiss {
                handler(event, window, cx);
            }
        });
        deferred(anchored().position(self.at).snap_to_window().child(card))
    }
}
