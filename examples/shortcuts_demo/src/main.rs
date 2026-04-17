//! Shortcuts Demo — end-to-end proof of the Shortcut / Action /
//! ShortcutSettings stack.
//!
//! Run with: `cargo run -p shortcuts-demo`
//!
//! What this example shows, in one window:
//!
//! - A **menu bar** where every item auto-renders its keystroke via
//!   `MenuItem::for_shortcut(id)` — rebinds done in the settings
//!   panel refresh the labels live.
//! - A **button with a rich tooltip** that also tracks the live
//!   shortcut via `TooltipContent::for_shortcut(id)`.
//! - A **ShortcutSettings panel** listing every registered shortcut
//!   with `Rebind / Rebind 2nd / Reset` controls. `Esc` cancels
//!   capture, `Del` / `Backspace` unbind a slot explicitly,
//!   conflicts auto-unbind.
//! - Several **dummy Actions** printing to stdout whenever their
//!   intent fires — proving shortcut → intent → action dispatch
//!   works for keyboard chords, menu clicks, the tooltip-bearing
//!   button, and `ctx.send_intent(...)` calls from arbitrary
//!   widgets.
//!
//! Try: press Ctrl+S, click menu → Edit → Bold, drag a shortcut to a
//! different chord in the settings panel, then press it.

use fern_ui::core::Action;
use fern_ui::core::intent::Intent;
use fern_ui::core::shortcut::{KeyStroke, Shortcut};
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Button, ButtonVariant, HStack, MenuBar, MenuItem, MenuList, Padding, Panel, ShortcutSettings,
    Spacer, TextWidget, VStack, tooltip::TooltipContent,
};

// ---------------------------------------------------------------------------
// Application commands — mostly used as sinks for the `on_command` handler,
// which prints them to stdout so the user can see activity.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    Save,
    Open,
    Quit,
    ToggleBold,
    ToggleItalic,
    FindInDocument,
    ShowHelp,
}

impl AppCommand for Cmd {}

// ---------------------------------------------------------------------------
// Root composite: registers shortcuts + actions on build(), lays out the UI.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Root {
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        Self { root_child_id: None }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // ---- Shortcut catalog ----
        //
        // Registered every build; the registry upserts by id so the
        // user's overrides in the settings panel survive re-renders.
        let catalog: Vec<Shortcut> = vec![
            Shortcut::new("app.save")
                .name("Save")
                .category("File")
                .primary(KeyStroke::ctrl(Key::S))
                .build(),
            Shortcut::new("app.open")
                .name("Open")
                .category("File")
                .primary(KeyStroke::ctrl(Key::O))
                .build(),
            Shortcut::new("app.quit")
                .name("Quit")
                .category("File")
                .primary(KeyStroke::ctrl(Key::Q))
                .build(),
            Shortcut::new("edit.format.bold")
                .name("Bold")
                .category("Edit")
                .primary(KeyStroke::ctrl(Key::B))
                .build(),
            Shortcut::new("edit.format.italic")
                .name("Italic")
                .category("Edit")
                .primary(KeyStroke::ctrl(Key::I))
                .build(),
            Shortcut::new("edit.find")
                .name("Find…")
                .category("Edit")
                .primary(KeyStroke::ctrl(Key::F))
                .build(),
            Shortcut::new("help.show")
                .name("Help")
                .category("Help")
                .primary(KeyStroke::new(Key::F1, Modifiers::NONE))
                .build(),
        ];
        for shortcut in catalog {
            ctx.register_shortcut_global(shortcut);
        }

        // ---- Dummy Actions wired to the intents ----
        //
        // Each one prints when the intent fires, so the console is
        // the proof of life. The same handlers also emit a typed
        // `Cmd` so `FernAppBuilder::on_command` sees the activity
        // and can run app-level logic.
        let action_intents = [
            ("app.save", Cmd::Save),
            ("app.open", Cmd::Open),
            ("app.quit", Cmd::Quit),
            ("edit.format.bold", Cmd::ToggleBold),
            ("edit.format.italic", Cmd::ToggleItalic),
            ("edit.find", Cmd::FindInDocument),
            ("help.show", Cmd::ShowHelp),
        ];
        for (intent_name, cmd) in action_intents {
            let cmd = cmd.clone();
            ctx.register_action(Action::new(intent_name).on_invoke(
                move |intent, event_ctx| {
                    println!(
                        "[action] intent {:?} fired → emitting {:?}",
                        intent.name, cmd
                    );
                    event_ctx.emit(cmd.clone());
                },
            ));
        }

        // ---- UI layout ----
        let theme = ctx.theme().clone();

        // Menu bar — every shortcut label is auto-resolved from the registry.
        let menu_bar = MenuBar::new()
            .menu_literal("File", || {
                Box::new(
                    MenuList::new()
                        .item(
                            MenuItem::new_literal("Open…")
                                .for_shortcut("app.open")
                                .on_activate(Cmd::Open),
                        )
                        .item(
                            MenuItem::new_literal("Save")
                                .for_shortcut("app.save")
                                .on_activate(Cmd::Save),
                        )
                        .separator()
                        .item(
                            MenuItem::new_literal("Quit")
                                .for_shortcut("app.quit")
                                .on_activate(Cmd::Quit),
                        ),
                )
            })
            .menu_literal("Edit", || {
                Box::new(
                    MenuList::new()
                        .item(
                            MenuItem::new_literal("Bold")
                                .for_shortcut("edit.format.bold")
                                .on_activate(Cmd::ToggleBold),
                        )
                        .item(
                            MenuItem::new_literal("Italic")
                                .for_shortcut("edit.format.italic")
                                .on_activate(Cmd::ToggleItalic),
                        )
                        .separator()
                        .item(
                            MenuItem::new_literal("Find…")
                                .for_shortcut("edit.find")
                                .on_activate(Cmd::FindInDocument),
                        ),
                )
            })
            .menu_literal("Help", || {
                Box::new(
                    MenuList::new().item(
                        MenuItem::new_literal("Show Help")
                            .for_shortcut("help.show")
                            .on_activate(Cmd::ShowHelp),
                    ),
                )
            });

        // A button demonstrating a rich tooltip whose chip stays in
        // sync with the registry.
        let tooltip = TooltipContent::new(
            "save-tip",
            fern_ui::i18n::LocalizedString::literal("Save the current document."),
        )
        .for_shortcut("app.save");
        let save_button = Button::new_literal("Save (button)")
            .style(ButtonVariant::Default)
            .on_activate(Cmd::Save)
            .rich_tooltip_content(tooltip);

        // A button that programmatically dispatches an intent instead
        // of activating a typed command directly — same result, since
        // the `edit.find` action ends up emitting Cmd::FindInDocument.
        let find_button = Button::new_literal("Fire intent: edit.find")
            .on_activate_fn(|ctx: &mut EventContext| {
                ctx.send_intent(Intent::new("edit.find"));
            });

        let header = Panel::new()
            .background(theme.colors.surface_content)
            .padding(12.0)
            .child(
                VStack::new()
                    .spacing(8.0)
                    .child(menu_bar)
                    .child(
                        HStack::new()
                            .spacing(12.0)
                            .child(save_button)
                            .child(find_button)
                            .child(Spacer::new())
                            .child(
                                TextWidget::new_literal(
                                    "Hover the Save button to see its registry-linked tooltip.",
                                )
                                .color(theme.colors.text_secondary),
                            ),
                    ),
            );

        let settings_heading = TextWidget::new_literal("Shortcut settings")
            .style(theme.typography.body_bold.clone())
            .color(theme.colors.text_primary);
        let settings_panel = Panel::new()
            .background(theme.colors.surface_main)
            .corner_radius(8.0)
            .padding(16.0)
            .child(
                VStack::new()
                    .spacing(12.0)
                    .child(settings_heading)
                    .child(
                        TextWidget::new_literal(
                            "Click Rebind, press a chord. Esc cancels, Del/Backspace unbinds. \
                             Conflicts auto-resolve.",
                        )
                        .color(theme.colors.text_secondary),
                    )
                    .child(ShortcutSettings::new()),
            );

        let root = ctx.add(
            Padding::uniform(16.0).child(
                VStack::new()
                    .spacing(16.0)
                    .child(header)
                    .child(settings_panel),
            ),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
    }
}

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("FernUI — Shortcuts Demo")
        .window_size(1000, 700)
        .on_command(|cmd: &Cmd, _ctx| {
            println!("[command] {:?}", cmd);
        })
        .root(|tree| tree.add(Root::new()))
        .run();
}
