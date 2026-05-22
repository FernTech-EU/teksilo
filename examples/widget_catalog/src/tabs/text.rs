//! Text tab — TextInput, SpinBox, SearchField, PasswordField, FilePickerField, InputDialog.

use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, Divider, FilePickerField, FilePickerKind, InputDialog, PasswordField,
    SearchField, SpinBox, TextInput, TextWidget, VStack,
};

use crate::shared::{Signals, section, tab_header};

/// Demo suggestion list — same fruit set as `new_widgets_kit` so the
/// SearchField shows a working autocomplete dropdown the moment the user
/// types one character.
const FRUITS: &[&str] = &[
    "Apple",
    "Apricot",
    "Avocado",
    "Banana",
    "Blackberry",
    "Blueberry",
    "Cherry",
    "Coconut",
    "Cranberry",
    "Date",
    "Dragonfruit",
    "Elderberry",
    "Fig",
    "Grape",
    "Grapefruit",
    "Guava",
    "Honeydew",
    "Kiwi",
    "Lemon",
    "Lime",
    "Lychee",
    "Mango",
    "Melon",
    "Nectarine",
    "Orange",
    "Papaya",
    "Passionfruit",
    "Peach",
    "Pear",
    "Persimmon",
    "Pineapple",
    "Plum",
    "Pomegranate",
    "Quince",
    "Raspberry",
    "Strawberry",
    "Tangerine",
    "Watermelon",
];

fn search_field(search_text: Signal<String>) -> SearchField {
    SearchField::new(search_text.clone())
        .placeholder(tr!(txt_search_placeholder()))
        .with_suggestions(|prefix| {
            let p = prefix.to_lowercase();
            FRUITS
                .iter()
                .filter(|f| f.to_lowercase().starts_with(&p))
                .map(|f| (*f).to_string())
                .collect()
        })
        .on_select(|value, _ctx| println!("picked suggestion: {value}"))
        .on_submit_fn(move |_| {
            println!("submit search: {:?}", search_text.get());
        })
}

pub fn title() -> LocalizedString {
    tr!(tab_text_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_text_refs())
}

pub fn classic(ctx: &mut BuildContext, sigs: &Signals) -> WidgetId {
    let header = tab_header(ctx, title(), refs());
    let text_input = section(
        ctx,
        lit!("TextInput"),
        VStack::new()
            .spacing(6.0)
            .child(
                TextInput::new(sigs.username_text.clone())
                    .label(tr!(txt_username_label()))
                    .placeholder(tr!(txt_username_placeholder())),
            )
            .child(
                TextInput::new(sigs.readonly_text.clone())
                    .label(tr!(txt_readonly_label()))
                    .read_only(true),
            ),
    );
    let spin_box = section(
        ctx,
        lit!("SpinBox"),
        SpinBox::new(sigs.spin_value.clone(), 0.0_f64, 100.0_f64)
            .single_step(1.0)
            .decimals(2),
    );
    let search = section(
        ctx,
        lit!("SearchField"),
        search_field(sigs.search_text.clone()),
    );
    let password_signal = ctx.signal(String::new());
    let password = section(
        ctx,
        lit!("PasswordField"),
        PasswordField::new(password_signal)
            .label(tr!(txt_password_label()))
            .placeholder(tr!(txt_password_placeholder()))
            .validator({
                let msg = tr!(txt_password_validation()).resolve_now();
                move |s| {
                    if s.chars().count() >= 8 {
                        bastyde::widgets::ValidationOutcome::Valid
                    } else {
                        bastyde::widgets::ValidationOutcome::Invalid {
                            message: msg.clone(),
                        }
                    }
                }
            }),
    );
    let file_path_signal = ctx.signal(String::new());
    let file_picker = section(
        ctx,
        lit!("FilePickerField"),
        FilePickerField::new(file_path_signal)
            .kind(FilePickerKind::OpenFile)
            .label(tr!(txt_file_label()))
            .placeholder(tr!(txt_file_placeholder())),
    );
    let trigger = Button::new(tr!(txt_input_dialog_trigger()))
        .variant(ButtonVariant::Filled)
        .on_activate_fn(|ctx| {
            InputDialog::new(tr!(txt_input_dialog_title()))
                .prompt(tr!(txt_input_dialog_prompt()))
                .placeholder(tr!(txt_input_dialog_placeholder()))
                .on_result(|value, _ctx| {
                    if let Some(name) = value {
                        println!("user entered: {name}");
                    }
                })
                .present(ctx);
        });
    let input_dialog = section(ctx, lit!("InputDialog"), trigger);

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(text_input)
            .add_child(spin_box)
            .add_child(search)
            .add_child(password)
            .add_child(file_picker)
            .add_child(input_dialog),
    )
}

pub fn bati(ctx: &mut BuildContext, sigs: &Signals) -> WidgetId {
    let username_sig = sigs.username_text.clone();
    let readonly_sig = sigs.readonly_text.clone();
    let spin_sig = sigs.spin_value.clone();
    let file_path_signal = ctx.signal(String::new());
    // Pre-register: `with_suggestions` / `on_select` / `on_submit_fn`
    // each take a non-trivial closure; the bati! property syntax handles
    // single-line closures fine but the suggestion closure is multi-line.
    let search_id = ctx.add(search_field(sigs.search_text.clone()));
    let password_signal = ctx.signal(String::new());
    let password_id = ctx.add(
        PasswordField::new(password_signal)
            .label(tr!(txt_password_label()))
            .placeholder(tr!(txt_password_placeholder()))
            .validator({
                let msg = tr!(txt_password_validation()).resolve_now();
                move |s| {
                    if s.chars().count() >= 8 {
                        bastyde::widgets::ValidationOutcome::Valid
                    } else {
                        bastyde::widgets::ValidationOutcome::Invalid {
                            message: msg.clone(),
                        }
                    }
                }
            }),
    );

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_text_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_text_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("TextInput")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                VStack {
                    spacing: 6.0
                    TextInput::new(username_sig) {
                        label: tr!(txt_username_label())
                        placeholder: tr!(txt_username_placeholder())
                    }
                    TextInput::new(readonly_sig) {
                        label: tr!(txt_readonly_label())
                        read_only: true
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("SpinBox")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                SpinBox::new(spin_sig, 0.0_f64, 100.0_f64) {
                    single_step: 1.0
                    decimals: 2
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("SearchField")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ search_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("PasswordField")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                #{ password_id }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("FilePickerField")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                FilePickerField::new(file_path_signal) {
                    kind: FilePickerKind::OpenFile
                    label: tr!(txt_file_label())
                    placeholder: tr!(txt_file_placeholder())
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(lit!("InputDialog")) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                Button::new(tr!(txt_input_dialog_trigger())) {
                    variant: ButtonVariant::Filled
                    on_activate_fn: |ctx| {
                        InputDialog::new(tr!(txt_input_dialog_title()))
                            .prompt(tr!(txt_input_dialog_prompt()))
                            .placeholder(tr!(txt_input_dialog_placeholder()))
                            .on_result(|value, _ctx| {
                                if let Some(name) = value {
                                    println!("user entered: {name}");
                                }
                            })
                            .present(ctx);
                    }
                }
            }
        }
    )
}
