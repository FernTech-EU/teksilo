// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Documentation image subjects for widgets that carry no
//! [`WidgetCatalog`](teksilo_preview::WidgetCatalog) impl.
//!
//! A catalog entry exists to be *driven* — typed knobs, named variants, an
//! editing form in the previewer. A documentation image needs only one
//! representative instance, so widgets that would otherwise go unpictured
//! register a [`doc_snippet!`](teksilo_preview::doc_snippet) here instead:
//! one expression, filed under the widget's own source path (whose stem is
//! the generated catalog page's slug).
//!
//! Two rules decide what belongs in this file.
//!
//! **Only widgets a still frame can describe.** Infrastructure
//! (`data_views`, `tree_source`, the toast host), invisible layout wrappers
//! (`MinSize`, `FixedSize`, `DeadZone`, `Shrinkable`, `Switcher`) and the
//! animation wrappers whose whole subject is *change over time* (`Fade`,
//! `Pulse`, `Collapse`, `Slide`, `Shake`, `Cycle`, `Crossfade`, `Unroll`,
//! `SmoothSize`, `Scale`) are deliberately absent: their picture would be
//! either an empty rectangle or an unremarkable copy of their child, which
//! is worse than no picture at all. `Blur` and `Rotate` are the exceptions
//! — a single frame shows exactly what they do.
//!
//! **Composed, not bare.** A `ScrollBar` alone is a grey stripe; next to
//! the content it scrolls it is a scroll bar. Snippets are free to build a
//! small scene, and several do.
//!
//! Snippets take precedence over a catalog entry for the same file, which
//! is also how a handful of entries whose default variant reads poorly in
//! print (`Grid`, `Spacer`, `IconWidget`, `TreeView`, `StandardListItem`)
//! get a better picture without disturbing the previewer.

use std::rc::Rc;

use jiff::civil::{Date, DateTime, Time};
use teksilo_core::signal::Signal;
use teksilo_core::styles::{ButtonVariant, TextInputVariant};
use teksilo_core::widget::Widget;
use teksilo_data::{ListModel, TreeModel};
use teksilo_i18n::lit;
use teksilo_preview::doc_snippet;
use teksilo_tokens::{Color, CornerRadius, SurfaceRole, TextRole, TextStyleRole};

use crate::primitives::{
    AspectRatio, ColumnFlow, FormLayout, HStack, MasonryLayout, Padding, RectWidget, TextWidget,
    TwistArrow, VStack, ValidationStrip, Wrap,
};
use crate::{
    Banner, Button, Calendar, ColorSwatch, DateEdit, DateRangeEdit, DateTimeEdit, FilePickerField,
    FilePickerKind, ScrollBar, ScrollBarOrientation, SearchField, SpinBox, Spinner, TimeEdit,
};

// =========================================================================
// Helpers
// =========================================================================

/// A short body paragraph, used as filler in the layout-primitive scenes.
fn note(text: &str) -> TextWidget {
    TextWidget::new(lit!(text.to_string())).style(TextStyleRole::Body)
}

/// A titled tile for the reflow / packing layout scenes.
fn tile(title: &str, height: f32) -> impl Widget + 'static {
    crate::Card::new().content(
        Padding::uniform(10.0).child(
            VStack::new()
                .spacing(4.0)
                .child(TextWidget::new(lit!(title.to_string())).style(TextStyleRole::BodyBold))
                .child(
                    crate::primitives::FixedSize::new().height(height).child(
                        RectWidget::new()
                            .background(SurfaceRole::Hover)
                            .corner_radius(CornerRadius::uniform(4.0)),
                    ),
                ),
        ),
    )
}

// =========================================================================
// Fields & pickers
// =========================================================================

doc_snippet!("crates/teksilo-widgets/src/spin_box.rs", {
    Box::new(
        SpinBox::new(Signal::new(12.5_f64), 0.0_f64, 100.0_f64)
            .single_step(0.5)
            .decimals(1)
            .label(lit!("Line height")),
    )
});

doc_snippet!("crates/teksilo-widgets/src/file_picker_field.rs", {
    Box::new(
        FilePickerField::new(Signal::new("/home/ada/notes/chapter-01.md".to_string()))
            .kind(FilePickerKind::OpenFile)
            .label(lit!("Manuscript")),
    )
});

doc_snippet!("crates/teksilo-widgets/src/date_edit.rs", {
    Box::new(
        crate::primitives::MaxSize::width(190.0).child(DateEdit::new(Signal::new(Some(
            Date::constant(2026, 3, 14),
        )))),
    )
});

doc_snippet!("crates/teksilo-widgets/src/time_edit.rs", {
    Box::new(
        crate::primitives::MaxSize::width(130.0).child(TimeEdit::new(Signal::new(Some(
            Time::constant(9, 30, 0, 0),
        )))),
    )
});

doc_snippet!("crates/teksilo-widgets/src/date_time_edit.rs", {
    Box::new(
        crate::primitives::MaxSize::width(260.0).child(DateTimeEdit::new(Signal::new(Some(
            DateTime::constant(2026, 3, 14, 9, 30, 0, 0),
        )))),
    )
});

doc_snippet!("crates/teksilo-widgets/src/date_range_edit.rs", {
    Box::new(
        crate::primitives::MaxSize::width(300.0).child(DateRangeEdit::new(Signal::new(Some(
            crate::DateRange::new(Date::constant(2026, 3, 2), Date::constant(2026, 3, 14)),
        )))),
    )
});

doc_snippet!(
    "crates/teksilo-widgets/src/calendar.rs",
    size = (280.0, 300.0),
    {
        Box::new(
            Calendar::single(Signal::new(Some(Date::constant(2026, 3, 14))))
                .show_today_button(true),
        )
    }
);

// =========================================================================
// Feedback
// =========================================================================

doc_snippet!("crates/teksilo-widgets/src/banner.rs", {
    Box::new(
        Banner::warning(lit!("Your trial ends in 3 days"))
            .description(lit!(
                "Add a licence key to keep syncing projects across devices."
            ))
            .action(Button::new(lit!("Add key")).variant(ButtonVariant::Filled)),
    )
});

doc_snippet!("crates/teksilo-widgets/src/spinner.rs", {
    Box::new(
        HStack::new()
            .spacing(10.0)
            .child(Spinner::new(20.0))
            .child(note("Indexing 1 240 files…")),
    )
});

doc_snippet!(
    "crates/teksilo-widgets/src/primitives/validation_strip.rs",
    {
        Box::new(
            VStack::new()
                .spacing(6.0)
                .child(
                    crate::TextInput::new(Signal::new("ada@".to_string()))
                        .variant(TextInputVariant::Outlined),
                )
                .child(ValidationStrip::new(Signal::new(
                    crate::ValidationFeedback::Invalid {
                        message: lit!("Enter a complete e-mail address."),
                    },
                ))),
        )
    }
);

// =========================================================================
// Chrome & small parts
// =========================================================================

doc_snippet!(
    "crates/teksilo-widgets/src/scroll_bar.rs",
    size = (300.0, 140.0),
    {
        Box::new(
            HStack::new()
                .spacing(8.0)
                .child(
                    crate::primitives::Expand::new().child(
                        VStack::new()
                            .spacing(6.0)
                            .child(note("The bar tracks the viewport"))
                            .child(note("as a fraction of the content,"))
                            .child(note("not as a fixed thumb size."))
                            .child(note("Drag it, or click the trough.")),
                    ),
                )
                .child(ScrollBar::new(
                    ScrollBarOrientation::Vertical,
                    Signal::new(40.0),
                    Signal::new(200.0),
                    Signal::new(0.45),
                )),
        )
    }
);

doc_snippet!("crates/teksilo-widgets/src/primitives/twist_arrow.rs", {
    Box::new(
        HStack::new()
            .spacing(18.0)
            .child(
                HStack::new()
                    .spacing(6.0)
                    .child(TwistArrow::new(12.0, true, false))
                    .child(note("Collapsed")),
            )
            .child(
                HStack::new()
                    .spacing(6.0)
                    .child(TwistArrow::new(12.0, true, true))
                    .child(note("Expanded")),
            ),
    )
});

doc_snippet!("crates/teksilo-widgets/src/color_picker/swatch.rs", {
    Box::new(
        HStack::new()
            .spacing(8.0)
            .child(
                ColorSwatch::new(Color::from_hex("#3584E4"))
                    .size(28.0)
                    .selected(true),
            )
            .child(ColorSwatch::new(Color::from_hex("#2EC27E")).size(28.0))
            .child(ColorSwatch::new(Color::from_hex("#F5C211")).size(28.0))
            .child(ColorSwatch::new(Color::from_hex("#E01B24")).size(28.0)),
    )
});

// =========================================================================
// Layout primitives — each shown with content that makes the rule visible
// =========================================================================

doc_snippet!(
    "crates/teksilo-widgets/src/primitives/aspect_ratio.rs",
    size = (280.0, 170.0),
    {
        Box::new(
            AspectRatio::new(16.0 / 9.0).child(
                crate::primitives::ZStack::new()
                    .child(
                        RectWidget::new()
                            .background(SurfaceRole::Hover)
                            .corner_radius(CornerRadius::uniform(6.0)),
                    )
                    .child(crate::primitives::Center::new().child(note("16 : 9"))),
            ),
        )
    }
);

doc_snippet!(
    "crates/teksilo-widgets/src/primitives/wrap.rs",
    size = (360.0, 120.0),
    {
        let mut wrap = Wrap::new().spacing(8.0).line_spacing(8.0);
        for tag in [
            "draft", "chapter", "revision", "outline", "notes", "archive", "pinned",
        ] {
            wrap = wrap.child(crate::Badge::new(lit!(tag.to_string())));
        }
        Box::new(wrap)
    }
);

doc_snippet!(
    "crates/teksilo-widgets/src/primitives/column_flow.rs",
    size = (520.0, 220.0),
    {
        let mut flow = ColumnFlow::new()
            .min_column_width(150.0)
            .column_spacing(16.0)
            .item_spacing(8.0);
        for (title, height) in [
            ("Prologue", 26.0),
            ("The crossing", 40.0),
            ("Winter camp", 20.0),
            ("The letter", 34.0),
            ("Return", 24.0),
            ("Epilogue", 18.0),
        ] {
            flow = flow.child(tile(title, height));
        }
        Box::new(flow)
    }
);

doc_snippet!(
    "crates/teksilo-widgets/src/primitives/masonry.rs",
    size = (460.0, 240.0),
    {
        let mut masonry = MasonryLayout::new(3)
            .column_spacing(12.0)
            .item_spacing(12.0);
        for (title, height) in [
            ("Cover", 60.0),
            ("Map", 28.0),
            ("Portrait", 44.0),
            ("Sketch", 22.0),
            ("Timeline", 36.0),
            ("Notes", 18.0),
        ] {
            masonry = masonry.child(tile(title, height));
        }
        Box::new(masonry)
    }
);

doc_snippet!(
    "crates/teksilo-widgets/src/primitives/form_layout.rs",
    size = (420.0, 170.0),
    {
        Box::new(
            FormLayout::new()
                .row_spacing(10.0)
                .line(
                    TextWidget::new(lit!("Title")).style(TextStyleRole::Small),
                    crate::TextInput::new(Signal::new("The Long Crossing".to_string())),
                )
                .line(
                    TextWidget::new(lit!("Author")).style(TextStyleRole::Small),
                    crate::TextInput::new(Signal::new("Ada Bellwether".to_string())),
                )
                .line(
                    TextWidget::new(lit!("Words")).style(TextStyleRole::Small),
                    SpinBox::new(Signal::new(82_400.0_f64), 0.0_f64, 1_000_000.0_f64),
                ),
        )
    }
);

doc_snippet!("crates/teksilo-widgets/src/primitives/rect_widget.rs", {
    Box::new(
        HStack::new()
            .spacing(10.0)
            .child(
                crate::primitives::FixedSize::new()
                    .width(72.0_f32)
                    .height(48.0_f32)
                    .child(
                        RectWidget::new()
                            .background(SurfaceRole::AccentSubtle)
                            .corner_radius(CornerRadius::uniform(8.0)),
                    ),
            )
            .child(
                crate::primitives::FixedSize::new()
                    .width(72.0_f32)
                    .height(48.0_f32)
                    .child(
                        RectWidget::new()
                            .background(SurfaceRole::Hover)
                            .border_width(1.0)
                            .border_color(teksilo_tokens::BorderRole::Default)
                            .corner_radius(CornerRadius::uniform(8.0)),
                    ),
            ),
    )
});

// =========================================================================
// Catalog overrides — entries whose default variant reads poorly in print
// =========================================================================

doc_snippet!("crates/teksilo-widgets/src/primitives/spacer.rs", {
    Box::new(
        crate::primitives::FixedSize::new().width(360.0_f32).child(
            HStack::new()
                .child(Button::new(lit!("Back")))
                .child(crate::primitives::Spacer::new())
                .child(Button::new(lit!("Continue")).variant(ButtonVariant::Filled)),
        ),
    )
});

doc_snippet!("crates/teksilo-widgets/src/primitives/icon_widget.rs", {
    use crate::primitives::IconWidget;
    Box::new(
        HStack::new()
            .spacing(16.0)
            .child(IconWidget::checkmark(22.0).color(TextRole::Accent))
            .child(IconWidget::chevron_right(22.0).color(TextRole::Secondary))
            .child(IconWidget::chevron_down(22.0).color(TextRole::Secondary))
            .child(IconWidget::radio_dot(22.0).color(TextRole::Primary)),
    )
});

doc_snippet!(
    "crates/teksilo-widgets/src/tree_view.rs",
    size = (300.0, 190.0),
    {
        let model = TreeModel::<String>::new();
        let manuscript = model.insert_root(0, "Manuscript".to_string());
        model.insert_child(manuscript, 0, "01 — Prologue".to_string());
        model.insert_child(manuscript, 1, "02 — The crossing".to_string());
        model.insert_child(manuscript, 2, "03 — Winter camp".to_string());
        let notes = model.insert_root(1, "Notes".to_string());
        model.insert_child(notes, 0, "Characters".to_string());
        model.insert_child(notes, 1, "Places".to_string());
        let view =
            crate::TreeView::new_with_context(model, |item: &String, entry, selected, ctx| {
                Box::new(
                    crate::StandardTreeItem::new(lit!(item.clone()))
                        .from_entry(entry)
                        .selected(selected)
                        .on_toggle_rc(ctx.toggle_callback()),
                )
            });
        // The slice starts collapsed; a picture of one closed root says nothing.
        view.expand_all();
        let _ = (manuscript, notes);
        Box::new(view)
    }
);

doc_snippet!(
    "crates/teksilo-widgets/src/standard_item.rs",
    size = (360.0, 60.0),
    {
        Box::new(
            crate::StandardListItem::new(lit!("The Long Crossing"))
                .subtitle(lit!("Edited 3 minutes ago · 82 400 words"))
                .trailing_slot(crate::Badge::new(lit!("draft"))),
        )
    }
);

// =========================================================================
// Dialogs, menus, overlay surfaces
// =========================================================================

doc_snippet!("crates/teksilo-widgets/src/dialog.rs", {
    // `Dialog` itself is the *trigger* button; the subject of the page is
    // the panel it presents, so the snippet composes the panel directly.
    Box::new(
        crate::dialog::ModalContainer::new(
            crate::dialog::DialogContent::new()
                .title(lit!("Discard changes?"))
                .supporting_text(lit!(
                    "“Chapter 3 — Winter camp” has unsaved edits from the last 12 minutes."
                ))
                .footer(
                    HStack::new()
                        .spacing(8.0)
                        .child(crate::primitives::Spacer::new())
                        .child(Button::new(lit!("Keep editing")))
                        .child(Button::new(lit!("Discard")).variant(ButtonVariant::Destructive)),
                ),
        )
        .min_width(380.0),
    )
});

doc_snippet!("crates/teksilo-widgets/src/message_box.rs", {
    Box::new(
        crate::MessageBox::question(lit!("Replace existing export?"))
            .informative_text(lit!("A file named “crossing.epub” already exists."))
            .buttons(crate::MessageBoxButtons::YesNo),
    )
});

doc_snippet!(
    "crates/teksilo-widgets/src/menu_item.rs",
    size = (240.0, 180.0),
    {
        Box::new(
            crate::MenuList::new()
                .item(crate::MenuItem::new(lit!("&New project")).shortcut_label("Ctrl+N"))
                .item(crate::MenuItem::new(lit!("&Open…")).shortcut_label("Ctrl+O"))
                .separator()
                .item(crate::MenuItem::new(lit!("Show &line numbers")).checked(Signal::new(true)))
                .item(crate::MenuItem::new(lit!("Show &minimap")).checked(Signal::new(false)))
                .separator()
                .item(crate::MenuItem::submenu(lit!("&Export"), || {
                    Box::new(
                        crate::MenuList::new()
                            .item(crate::MenuItem::new(lit!("EPUB…")))
                            .item(crate::MenuItem::new(lit!("PDF…"))),
                    )
                })),
        )
    }
);

doc_snippet!("crates/teksilo-widgets/src/menu_bar.rs", {
    Box::new(
        crate::MenuBar::new()
            .no_dispatcher_install()
            .menu(lit!("&File"), || {
                Box::new(
                    crate::MenuList::new()
                        .item(crate::MenuItem::new(lit!("&New")).shortcut_label("Ctrl+N"))
                        .item(crate::MenuItem::new(lit!("&Open…")).shortcut_label("Ctrl+O"))
                        .separator()
                        .item(crate::MenuItem::new(lit!("&Quit")).shortcut_label("Ctrl+Q")),
                )
            })
            .menu(lit!("&Edit"), || {
                Box::new(
                    crate::MenuList::new()
                        .item(crate::MenuItem::new(lit!("&Undo")).shortcut_label("Ctrl+Z"))
                        .item(crate::MenuItem::new(lit!("&Redo")).shortcut_label("Ctrl+Y")),
                )
            })
            .menu(lit!("&View"), || {
                Box::new(crate::MenuList::new().item(crate::MenuItem::new(lit!("&Zoom in"))))
            })
            .menu(lit!("&Help"), || {
                Box::new(crate::MenuList::new().item(crate::MenuItem::new(lit!("&About"))))
            }),
    )
});

doc_snippet!("crates/teksilo-widgets/src/tooltip.rs", {
    Box::new(crate::TooltipWidget::new(lit!("Rebuild the search index")))
});

doc_snippet!("crates/teksilo-widgets/src/command_link_button.rs", {
    Box::new(
        VStack::new()
            .spacing(8.0)
            .child(
                crate::CommandLinkButton::new(lit!("Start a new manuscript"))
                    .description(lit!("Creates a project with one empty chapter.")),
            )
            .child(
                crate::CommandLinkButton::new(lit!("Import from Markdown"))
                    .description(lit!("Splits headings into chapters.")),
            ),
    )
});

// =========================================================================
// Data views
// =========================================================================

/// One row of the table / tree-table sample scenes.
struct Row {
    name: &'static str,
    words: u32,
    status: &'static str,
}

fn sample_rows() -> ListModel<Row> {
    let model = ListModel::new();
    for (name, words, status) in [
        ("01 — Prologue", 2_140_u32, "Final"),
        ("02 — The crossing", 8_920, "Revising"),
        ("03 — Winter camp", 6_305, "Draft"),
        ("04 — The letter", 4_780, "Draft"),
        ("05 — Return", 3_190, "Outline"),
    ] {
        model.push(Row {
            name,
            words,
            status,
        });
    }
    model
}

doc_snippet!(
    "crates/teksilo-widgets/src/table_view.rs",
    size = (520.0, 220.0),
    {
        use crate::table_view::{Column, ColumnWidth, TableView};
        Box::new(
            TableView::new(sample_rows())
                .add_column(
                    Column::new("name", lit!("Chapter"), |r: &Row, _cx| {
                        Box::new(TextWidget::new(lit!(r.name)))
                    })
                    .width(ColumnWidth::Flex(1.0)),
                )
                .add_column(
                    Column::new("words", lit!("Words"), |r: &Row, _cx| {
                        Box::new(TextWidget::new(lit!(r.words.to_string())))
                    })
                    .width(ColumnWidth::Fixed(90.0)),
                )
                .add_column(
                    Column::new("status", lit!("Status"), |r: &Row, _cx| {
                        Box::new(crate::Badge::new(lit!(r.status)))
                    })
                    .width(ColumnWidth::Fixed(110.0)),
                )
                .alternating_rows(true),
        )
    }
);

doc_snippet!(
    "crates/teksilo-widgets/src/tree_table_view.rs",
    size = (520.0, 220.0),
    {
        use crate::table_view::{Column, ColumnWidth};
        use crate::tree_table_view::TreeTableView;
        let model = TreeModel::<Row>::new();
        let part_one = model.insert_root(
            0,
            Row {
                name: "Part one — The crossing",
                words: 17_365,
                status: "Revising",
            },
        );
        model.insert_child(
            part_one,
            0,
            Row {
                name: "01 — Prologue",
                words: 2_140,
                status: "Final",
            },
        );
        model.insert_child(
            part_one,
            1,
            Row {
                name: "02 — The crossing",
                words: 8_920,
                status: "Revising",
            },
        );
        model.insert_child(
            part_one,
            2,
            Row {
                name: "03 — Winter camp",
                words: 6_305,
                status: "Draft",
            },
        );
        let view = TreeTableView::new(model)
            .add_column(
                Column::new("name", lit!("Chapter"), |r: &Row, _cx| {
                    Box::new(TextWidget::new(lit!(r.name)))
                })
                .width(ColumnWidth::Flex(1.0)),
            )
            .add_column(
                Column::new("words", lit!("Words"), |r: &Row, _cx| {
                    Box::new(TextWidget::new(lit!(r.words.to_string())))
                })
                .width(ColumnWidth::Fixed(90.0)),
            );
        view.expand_all();
        Box::new(view)
    }
);

doc_snippet!(
    "crates/teksilo-widgets/src/code_editor/log_view.rs",
    size = (520.0, 190.0),
    { Box::new(SeededLogView::default()) }
);

// =========================================================================
// Drop targets
// =========================================================================

doc_snippet!(
    "crates/teksilo-widgets/src/drop_zone.rs",
    size = (360.0, 150.0),
    {
        Box::new(
            crate::DropZone::new(lit!("Drop manuscripts here"))
                .subtitle(lit!("Markdown, ODT or plain text"))
                .accept_extensions(["md", "odt", "txt"]),
        )
    }
);

doc_snippet!(
    "crates/teksilo-widgets/src/drop_target.rs",
    size = (300.0, 120.0),
    {
        Box::new(
            crate::DropTarget::new()
                .variant(crate::DropTargetVariant::Prominent)
                .child(
                    crate::Card::new().content(
                        Padding::uniform(14.0).child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new(lit!("Playlist"))
                                        .style(TextStyleRole::BodyBold),
                                )
                                .child(note("Drag tracks onto this card")),
                        ),
                    ),
                ),
        )
    }
);

// =========================================================================
// Settings & flows
// =========================================================================

doc_snippet!(
    "crates/teksilo-widgets/src/stepper.rs",
    size = (460.0, 90.0),
    {
        use crate::{Step, StepStatus, Stepper};
        // Every `Step` must carry a content factory, even when the picture is
        // only of the indicator strip.
        let step = |title: &'static str, status: StepStatus| {
            Step::new(lit!(title))
                .status(status)
                .content(move || TextWidget::new(lit!(title)))
        };
        Box::new(
            Stepper::new()
                .step(step("Account", StepStatus::Complete))
                .step(step("Library", StepStatus::Active))
                .step(step("Sync", StepStatus::Upcoming))
                .step(step("Done", StepStatus::Upcoming)),
        )
    }
);

doc_snippet!(
    "crates/teksilo-widgets/src/shortcut_settings.rs",
    size = (460.0, 220.0),
    { Box::new(crate::ShortcutSettings::new()) }
);

// =========================================================================
// Notifications
// =========================================================================

doc_snippet!(
    "crates/teksilo-widgets/src/notification/center_button.rs",
    {
        let archive = Rc::new(crate::NotificationArchiveModel::in_memory());
        archive.push(sample_notification(1, "Export finished", None));
        archive.push(sample_notification(2, "2 unresolved references", None));
        Box::new(crate::NotificationCenterButton::new(archive))
    }
);

doc_snippet!(
    "crates/teksilo-widgets/src/notification/log.rs",
    size = (400.0, 260.0),
    {
        let archive = Rc::new(crate::NotificationArchiveModel::in_memory());
        archive.push(sample_notification(
            1,
            "Export finished",
            Some("out/crossing.epub — 312 KB"),
        ));
        archive.push(sample_notification(
            2,
            "2 unresolved references",
            Some("chapter-03.md lines 44 and 91"),
        ));
        archive.push(sample_notification(
            3,
            "Sync complete",
            Some("14 files uploaded"),
        ));
        Box::new(crate::NotificationLog::new(archive).preferred_width(360.0))
    }
);

// =========================================================================
// Motion wrappers a still frame can actually describe
// =========================================================================

doc_snippet!(
    "crates/teksilo-widgets/src/animations/blur.rs",
    size = (300.0, 120.0),
    {
        Box::new(
            HStack::new()
                .spacing(16.0)
                .child(tile("Sharp", 30.0))
                .child(crate::Blur::new(3.0).child(tile("Blurred", 30.0))),
        )
    }
);

doc_snippet!("crates/teksilo-widgets/src/animations/rotate.rs", {
    Box::new(
        HStack::new()
            .spacing(24.0)
            .child(crate::primitives::IconWidget::chevron_right(24.0))
            .child(
                crate::Rotate::new(std::f32::consts::FRAC_PI_2)
                    .child(crate::primitives::IconWidget::chevron_right(24.0)),
            ),
    )
});

// =========================================================================
// Grid — the catalog default is two bare cells; show the track model
// =========================================================================

doc_snippet!(
    "crates/teksilo-widgets/src/primitives/grid.rs",
    size = (360.0, 130.0),
    {
        use crate::primitives::{Grid, TrackSize};
        let cell = |text: &'static str| {
            crate::Card::new().content(Padding::uniform(10.0).child(TextWidget::new(lit!(text))))
        };
        Box::new(
            Grid::new()
                .columns(vec![
                    TrackSize::Fixed(90.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                ])
                .rows(vec![TrackSize::Auto, TrackSize::Auto])
                .column_gap(8.0)
                .row_gap(8.0)
                .child(cell("Fixed 90"))
                .child(cell("Flex 1"))
                .child(cell("Flex 1"))
                .child(cell("Row 2"))
                .child(cell("Row 2"))
                .child(cell("Row 2")),
        )
    }
);

/// A ready-made archive row for the notification scenes. `NotificationEntry`
/// is a plain serde struct with no builder, so the field list lives here once.
fn sample_notification(id: u64, title: &str, body: Option<&str>) -> crate::NotificationEntry {
    crate::NotificationEntry {
        id,
        severity: crate::BannerSeverity::Info,
        priority: crate::ToastPriority::Normal,
        title: title.to_string(),
        body: body.map(str::to_string),
        actions: Vec::new(),
        timestamp: jiff::Timestamp::now(),
        group: None,
        source: None,
        read: false,
        dedup_id: None,
        updates: Vec::new(),
        route: crate::ToastRoute::Broadcast,
    }
}

/// A `LogView` with a few lines already in it.
///
/// The view's append queue only exists once the widget is mounted (the
/// handle is a no-op before then), so the sample lines are pushed from
/// `build`, right after the child is inserted.
#[derive(Debug, Default)]
struct SeededLogView {
    child: Option<teksilo_core::widget_id::WidgetId>,
}

impl Widget for SeededLogView {
    fn build(
        &mut self,
        ctx: &mut teksilo_core::build_context::BuildContext,
    ) -> Vec<teksilo_core::widget_id::WidgetId> {
        let view = crate::LogView::new().font_family("monospace");
        let handle = view.handle();
        let id = ctx.add(view);
        handle.append_lines([
            "12:04:01  INFO   watching 1 240 files",
            "12:04:01  INFO   index rebuilt in 312 ms",
            "12:04:07  WARN   chapter-03.md: 2 unresolved references",
            "12:04:07  INFO   export started (epub)",
            "12:04:09  ERROR  export failed: missing cover image",
            "12:04:12  INFO   retrying with placeholder cover",
            "12:04:13  INFO   export finished → out/crossing.epub",
        ]);
        self.child = Some(id);
        vec![id]
    }

    fn layout_response(
        &self,
        proposal: teksilo_canvas::SizeProposal,
        ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.child
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn children(&self) -> Vec<teksilo_core::widget_id::WidgetId> {
        self.child.into_iter().collect()
    }
}

// =========================================================================
// Text surfaces
// =========================================================================

doc_snippet!(
    "crates/teksilo-widgets/src/rich_text.rs",
    size = (460.0, 180.0),
    {
        let doc = teksilo_text::text_document::TextDocument::new();
        doc.set_plain_text(
            "The Long Crossing\n\n\
         They left the winter camp at first light, four sledges and eleven dogs, \
         with the barometer still falling.\n\n\
         — Chapter three, second draft",
        )
        .expect("seed the sample document");
        Box::new(crate::rich_text::RichTextEditor::read_only(doc).content_padding(12.0))
    }
);

doc_snippet!(
    "crates/teksilo-widgets/src/code_editor.rs",
    size = (480.0, 200.0),
    {
        let doc = teksilo_text::text_document::TextDocument::new();
        doc.set_plain_text(
        "fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {\n\
        \x20   let child = self.child_id?;\n\
        \x20   let inner = proposal.deflate(self.insets);\n\
        \x20   let size = ctx.child_size(child, inner)?;\n\
        \x20   LayoutResponse::from(size.inflate(self.insets))\n\
         }\n",
    )
    .expect("seed the sample document");
        Box::new(crate::CodeEditor::read_only(doc).font_family("monospace"))
    }
);

// =========================================================================
// Shell
// =========================================================================

doc_snippet!(
    "crates/teksilo-widgets/src/docking.rs",
    size = (800.0, 380.0),
    {
        use crate::docking::{
            DockOpenLocation, DockSide, DockWidget, DockWidgetId, DockingLayout, DockingModel,
        };
        let model = DockingModel::new();
        model.set_side_rail(DockSide::Leading, 44.0);
        let explorer = DockWidgetId::fresh();
        let outline = DockWidgetId::fresh();
        let problems = DockWidgetId::fresh();
        let layout = DockingLayout::new(model.clone())
            .center(panel_scene(
                "Editor",
                &["fn main() {", "    teksilo::run();", "}"],
            ))
            .dock(DockWidget::new(explorer, lit!("Explorer"), |_| {
                panel_scene("Files", &["src", "crates", "docs", "Cargo.toml"])
            }))
            .dock(DockWidget::new(outline, lit!("Outline"), |_| {
                panel_scene("Outline", &["main", "build", "layout_response"])
            }))
            .dock(DockWidget::new(problems, lit!("Problems"), |_| {
                panel_scene("Problems", &["2 warnings", "0 errors"])
            }));
        // `.dock(...)` only registers; a side stays empty until its dock is
        // opened — same order the `docking` example uses.
        model.open_dock(explorer, DockOpenLocation::side(DockSide::Leading));
        model.open_dock(outline, DockOpenLocation::side(DockSide::Trailing));
        model.open_dock(problems, DockOpenLocation::side(DockSide::Bottom));
        Box::new(layout)
    }
);

/// A titled list panel, used as filler content inside the docking scene.
fn panel_scene(title: &'static str, lines: &'static [&'static str]) -> impl Widget + 'static {
    let mut column = VStack::new().spacing(4.0).child(
        TextWidget::new(lit!(title))
            .style(TextStyleRole::BodyBold)
            .color(TextRole::Primary),
    );
    for line in lines {
        column = column.child(note(line));
    }
    crate::Panel::new()
        .background(SurfaceRole::Raised)
        .padding(10.0)
        .child(column)
}

// =========================================================================
// Registry-backed widgets — seeded from a host so the list isn't empty
// =========================================================================

doc_snippet!(
    "crates/teksilo-widgets/src/shortcut_settings.rs",
    size = (460.0, 200.0),
    { Box::new(SeededShortcutSettings::default()) }
);

/// `ShortcutSettings` renders the tree's live `ShortcutRegistry`, which is
/// empty in a freshly-built tree — the page would get a picture of a
/// heading and nothing else. This host registers a handful of realistic
/// bindings first.
#[derive(Debug, Default)]
struct SeededShortcutSettings {
    child: Option<teksilo_core::widget_id::WidgetId>,
}

impl Widget for SeededShortcutSettings {
    fn build(
        &mut self,
        ctx: &mut teksilo_core::build_context::BuildContext,
    ) -> Vec<teksilo_core::widget_id::WidgetId> {
        use teksilo_core::event::Key;
        use teksilo_core::shortcut::{KeyStroke, Shortcut};
        for (id, name, stroke) in [
            ("app.new", "New project", KeyStroke::ctrl(Key::N)),
            ("app.open", "Open…", KeyStroke::ctrl(Key::O)),
            ("app.save", "Save", KeyStroke::ctrl(Key::S)),
            ("app.find", "Find in project", KeyStroke::ctrl(Key::F)),
        ] {
            ctx.register_shortcut_global(Shortcut::new(id).name(name).primary(stroke).build());
        }
        let id = ctx.add(crate::ShortcutSettings::new());
        self.child = Some(id);
        vec![id]
    }

    fn layout_response(
        &self,
        proposal: teksilo_canvas::SizeProposal,
        ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        self.child
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn children(&self) -> Vec<teksilo_core::widget_id::WidgetId> {
        self.child.into_iter().collect()
    }
}

// =========================================================================
// More catalog overrides
// =========================================================================

doc_snippet!(
    "crates/teksilo-widgets/src/accordion.rs",
    size = (360.0, 150.0),
    {
        Box::new(
            crate::Accordion::new(lit!("Export options"), Signal::new(true)).content(
                VStack::new()
                    .spacing(8.0)
                    .child(crate::Checkbox::new(Signal::new(true)).label(lit!("Embed fonts")))
                    .child(crate::Checkbox::new(Signal::new(false)).label(lit!("Include notes")))
                    .child(
                        crate::Checkbox::new(Signal::new(true))
                            .label(lit!("Split chapters into files")),
                    ),
            ),
        )
    }
);

doc_snippet!("crates/teksilo-widgets/src/search_field.rs", {
    Box::new(
        crate::primitives::MaxSize::width(320.0).child(
            SearchField::new(Signal::new("bézier".to_string()))
                .placeholder(lit!("Search the catalog…")),
        ),
    )
});

doc_snippet!("crates/teksilo-widgets/src/hex_color_input.rs", {
    Box::new(
        crate::primitives::MaxSize::width(200.0).child(crate::HexColorInput::new(Signal::new(
            Color::from_hex("#3584E4"),
        ))),
    )
});
