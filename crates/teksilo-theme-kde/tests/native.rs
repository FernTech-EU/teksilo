// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The live desktop: `from_desktop` follows a KDE desktop's fonts,
//! animation speed and `breezerc`, and `native_theme` answers only for a
//! desktop styled with Breeze, through `teksilo-app`'s native-theme seam.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_app::{NativeThemeFactory, NativeThemeRequest, resolve_native_theme};
use teksilo_canvas::{MockTextBackend, RenderFrame, SizeProposal};
use teksilo_core::styles::Theme;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;
use teksilo_core::widget_tree::WidgetTree;
use teksilo_i18n::lit;
use teksilo_platform::os_theme::NativeDesktop;
use teksilo_theme_kde::scheme::{KdeColor, KdeColorScheme, KdeDesktop, KdeFont, ShadowSize};
use teksilo_theme_kde::styles::popup::{BreezePopupColors, BreezePopupKind};
use teksilo_theme_kde::{self as kde, BreezeStyleConfig, KdeFontRoles, KdePalette};
use teksilo_tokens::{Color, FontWeight, TargetDensity};
use teksilo_widgets::primitives::{FixedSize, HStack};
use teksilo_widgets::{Button, MenuItem, MenuList, ProgressBar, VStack};

fn light_desktop() -> KdeDesktop {
    KdeDesktop::new(KdeColorScheme::breeze_light())
}

fn font(qt: &str) -> KdeFont {
    KdeFont::parse_qfont(qt).unwrap_or_else(|| panic!("{qt} does not parse"))
}

fn line_box(style: &teksilo_tokens::TextStyle) -> f32 {
    style.size * style.line_height
}

fn tree_with(theme: Theme) -> WidgetTree {
    WidgetTree::new()
        .with_theme(theme)
        .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
}

fn rgba8(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn ms(d: Duration) -> u128 {
    d.as_millis()
}

// ── Fonts ────────────────────────────────────────────────────────────────

/// A desktop whose five fonts are all set: a Qt 6 general font of a family
/// the preset has no metrics for, a 9 pt smallest readable font, an 11 pt
/// Hack, a 12 pt menu font and a Qt 5 toolbar font.
fn desktop_with_fonts() -> KdeDesktop {
    let mut d = light_desktop();
    d.fonts.general = font("Cantarell,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1");
    d.fonts.small = font("Noto Sans,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1");
    d.fonts.fixed = font("Hack,11,-1,7,400,0,0,0,1,0,0,0,0,0,0,1");
    d.fonts.menu = font("Noto Sans,12,-1,5,400,0,0,0,0,0,0,0,0,0,0,1");
    d.fonts.toolbar = font("Ubuntu,10,-1,5,50,0,0,0,0,0");
    d
}

/// Each text style takes its font's family and the whole pixel size Qt
/// draws it at, on Qt's line box for Noto Sans and Hack and on
/// `round(P x 1.362)` for any other family; the weights stay the slots'.
#[test]
fn the_desktops_fonts_become_the_text_styles() {
    let t = kde::from_desktop(&desktop_with_fonts(), TargetDensity::Compact).typography;
    // Cantarell 11 pt: a 15 px face, 15 x 1.362 = 20.43.
    for (style, weight) in [
        (&t.body, FontWeight::REGULAR),
        (&t.body_bold, FontWeight::SEMI_BOLD),
    ] {
        assert_eq!(style.family, "Cantarell");
        assert_eq!(style.size, 15.0);
        assert!((line_box(style) - 20.0).abs() < 1e-4);
        assert_eq!(style.weight, weight);
        assert_eq!(style.letter_spacing, 0.0);
    }
    // Noto Sans 9 pt: a 12 px face on Qt's 17 px line.
    for style in [&t.small, &t.small_bold, &t.tiny] {
        assert_eq!(style.family, "Noto Sans");
        assert_eq!(style.size, 12.0);
        assert!((line_box(style) - 17.0).abs() < 1e-4);
    }
    assert_eq!(t.small_bold.weight, FontWeight::SEMI_BOLD);
    // Hack 11 pt: a 15 px face on Qt's 18 px line.
    assert_eq!(t.mono.family, "Hack");
    assert_eq!(t.mono.size, 15.0);
    assert!((line_box(&t.mono) - 18.0).abs() < 1e-4);
}

/// The menu and toolbar fonts, which have no text style, ride on the theme
/// as `KdeFontRoles`; a Qt 5 font string is read as Plasma 5 wrote it.
#[test]
fn the_menu_and_toolbar_fonts_ride_on_the_theme() {
    let theme = kde::from_desktop(&desktop_with_fonts(), TargetDensity::Compact);
    let roles = theme.extension::<KdeFontRoles>().expect("the font roles");
    assert_eq!(roles.menu.family, "Noto Sans");
    assert_eq!(roles.menu.size, 16.0);
    assert!((line_box(&roles.menu) - 22.0).abs() < 1e-4);
    assert_eq!(roles.toolbar.family, "Ubuntu");
    assert_eq!(roles.toolbar.size, 13.0);
    assert!((line_box(&roles.toolbar) - 18.0).abs() < 1e-4);
    assert_eq!(roles.toolbar.weight, FontWeight::REGULAR);
}

/// The control heights follow the general font's line box, and a menu row
/// the taller of the menu font's and the body's.
#[test]
fn the_controls_follow_the_desktops_fonts() {
    let theme = kde::from_desktop(&desktop_with_fonts(), TargetDensity::Compact);
    // A 22 px menu line: max(22, 20, 16) + 1 + 2 x 4.
    let menu = theme.style_slots.menu_item.as_ref().expect("a menu style");
    assert_eq!(menu.metrics().item_height, 31.0);
    // A 20 px body line: the push button is H + 16.
    let mut tree = tree_with(theme);
    let row = tree.add(HStack::new().child(Button::new(lit!("OK"))));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let button = tree.children(row)[0];
    assert_eq!(tree.bounds(button).height, 36.0);

    // A menu font smaller than the body leaves the rows sized for the body,
    // which their labels are written in: a 20 px line gives 29.
    let mut d = desktop_with_fonts();
    d.fonts.menu = font("Noto Sans,7,-1,5,400,0,0,0,0,0,0,0,0,0,0,1");
    let theme = kde::from_desktop(&d, TargetDensity::Compact);
    let menu = theme.style_slots.menu_item.as_ref().unwrap();
    assert_eq!(menu.metrics().item_height, 29.0);
}

/// The density the theme is asked at reprojects the rows built for the
/// desktop's menu font too.
#[test]
fn the_menu_font_survives_a_density_projection() {
    let theme = kde::from_desktop(&desktop_with_fonts(), TargetDensity::Touch);
    assert_eq!(theme.input.density, TargetDensity::Touch);
    let back = theme.with_density(TargetDensity::Compact);
    let menu = back.style_slots.menu_item.as_ref().unwrap();
    assert_eq!(menu.metrics().item_height, 31.0);
}

// ── Animation speed ──────────────────────────────────────────────────────

/// A factor of 2 doubles Kirigami's long duration and everything derived
/// from it, and leaves the busy period and the tooltip delays alone.
#[test]
fn a_factor_of_two_slows_the_transitions() {
    let mut d = light_desktop();
    d.animation_duration_factor = Some(2.0);
    let m = kde::from_desktop(&d, TargetDensity::Compact).motion;
    assert_eq!(ms(m.duration_fast), 200);
    assert_eq!(ms(m.duration_normal), 400);
    assert_eq!(ms(m.duration_slow), 800);
    assert_eq!(ms(m.duration_collapse), 400);
    assert_eq!(ms(m.duration_indeterminate_sweep), 800);
    assert_eq!(ms(m.tooltip_delay), 700);
    assert_eq!(ms(m.tooltip_delay_heavy), 700);
    assert_eq!(m.duration_instant, Duration::ZERO);
    assert_eq!(m.easing_standard, kde::light().motion.easing_standard);
}

/// A factor of 0 is the desktop asking for reduced motion: the theme
/// reports it, and its transitions are 0 or 1 ms long.
#[test]
fn a_factor_of_zero_is_reduced_motion() {
    let mut d = light_desktop();
    d.animation_duration_factor = Some(0.0);
    let theme = kde::from_desktop(&d, TargetDensity::Compact);
    let config = theme.extension::<BreezeStyleConfig>().expect("the config");
    assert!(config.prefers_reduced_motion());
    assert!(d.prefers_reduced_motion());
    let m = &theme.motion;
    assert_eq!(ms(m.duration_fast), 0);
    assert_eq!(ms(m.duration_normal), 1);
    assert_eq!(ms(m.duration_slow), 2);
    assert_eq!(ms(m.duration_collapse), 1);
    assert_eq!(ms(m.duration_indeterminate_sweep), 800);
    assert_eq!(ms(m.tooltip_delay), 700);
}

/// A desktop that sets no factor animates at Breeze's own speed, and does
/// not ask for reduced motion.
#[test]
fn no_factor_is_the_default_speed() {
    let theme = kde::from_desktop(&light_desktop(), TargetDensity::Compact);
    assert_eq!(theme.motion, kde::light().motion);
    let config = theme.extension::<BreezeStyleConfig>().unwrap();
    assert!(!config.prefers_reduced_motion());
    assert_eq!(config.animation_factor, None);
}

// ── breezerc ─────────────────────────────────────────────────────────────

/// A menu under `MenuOpacity` 80 is filled at 80 % and outlined in the
/// window text at 25 %; at the default 100 it is the opaque panel.
#[test]
fn a_translucent_menu_follows_menu_opacity() {
    let mut d = light_desktop();
    d.breeze.menu_opacity = 80;
    let theme = kde::from_desktop(&d, TargetDensity::Compact);
    let config = theme.extension::<BreezeStyleConfig>().unwrap();
    assert_eq!(config.menu_opacity(), 0.8);
    let colors = BreezePopupColors::resolve(&theme, BreezePopupKind::Menu);
    let fill = rgba8(colors.fill.to_array());
    assert_eq!(fill, [244, 245, 245, 204]);
    let outline = theme.colors.text_primary.with_alpha(0.25);
    assert_eq!(colors.outline, outline);

    // Painted: the menu's panel carries those colours.
    let mut tree = tree_with(theme.clone());
    tree.add(
        VStack::new().child(
            FixedSize::new()
                .width(200.0)
                .child(MenuList::new().item(MenuItem::new(lit!("Open")))),
        ),
    );
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let frame = tree.render();
    let shapes: Vec<([u8; 4], f32)> = frame
        .shapes
        .iter()
        .map(|s| (rgba8(s.color), s.stroke_width))
        .collect();
    assert!(
        shapes.contains(&(fill, 0.0)),
        "no translucent panel in {shapes:?}"
    );
    assert!(
        shapes.contains(&(rgba8(outline.to_array()), 1.0)),
        "no window-text outline in {shapes:?}"
    );

    // A popover is not a menu, and stays opaque.
    let popover = BreezePopupColors::resolve(&theme, BreezePopupKind::Panel);
    assert_eq!(popover.fill.a(), 1.0);
    // The stock opacity paints the opaque panel.
    let stock = kde::from_desktop(&light_desktop(), TargetDensity::Compact);
    assert_eq!(
        BreezePopupColors::resolve(&stock, BreezePopupKind::Menu),
        BreezePopupColors::resolve(&stock, BreezePopupKind::Panel)
    );
}

/// The window shadow under a popup is the size, colour and strength
/// `breezerc` asks for.
#[test]
fn the_window_shadow_follows_breezerc() {
    let mut d = light_desktop();
    d.breeze.shadow_size = ShadowSize::Small;
    d.breeze.shadow_strength = 102;
    d.breeze.shadow_color = KdeColor::rgb8(0, 0, 128);
    let shape = kde::from_desktop(&d, TargetDensity::Compact).shape;
    let navy = KdeColor::rgb8(0, 0, 128).to_color();
    assert_eq!(
        (shape.shadow_lg.offset_y, shape.shadow_lg.blur),
        (3.0, 12.0)
    );
    assert!((shape.shadow_lg.color.a() - 0.26 * 0.4).abs() < 1e-6);
    assert_eq!(shape.shadow_lg.color.with_alpha(1.0), navy);
    assert_eq!(
        (shape.shadow_inner_lg.offset_y, shape.shadow_inner_lg.blur),
        (1.0, 6.0)
    );
    assert!((shape.shadow_inner_lg.color.a() - 0.16 * 0.4).abs() < 1e-6);
    // The rest of the shape is Breeze's.
    let stock = kde::light().shape;
    assert_eq!(shape.radius_control, stock.radius_control);
    assert_eq!(shape.shadow_xs, stock.shadow_xs);

    d.breeze.shadow_size = ShadowSize::None;
    let bare = kde::from_desktop(&d, TargetDensity::Compact).shape;
    assert_eq!(bare.shadow_lg.color.a(), 0.0);
    assert_eq!(bare.shadow_inner_lg.color.a(), 0.0);
    assert_eq!(
        kde::from_desktop(&light_desktop(), TargetDensity::Compact).shape,
        stock
    );
}

/// A tree holding `w` on the simulated clock, laid out and rendered once:
/// the tree and the widget's id.
fn on_the_clock(theme: Theme, w: impl Widget + 'static) -> (WidgetTree, WidgetId) {
    let mut tree = tree_with(theme);
    tree.advance_time(Duration::ZERO);
    let column = tree.add(VStack::new().child(HStack::new().child(w)));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    let row = tree.children(column)[0];
    let id = tree.children(row)[0];
    tree.render();
    (tree, id)
}

fn tick(tree: &mut WidgetTree, ms: u64) -> Rc<RenderFrame> {
    tree.advance_time(Duration::from_millis(ms));
    tree.layout(SizeProposal::exact(400.0, 300.0));
    tree.render()
}

/// The leading edges of a busy bar's highlight stripes, from the bar's.
fn stripe_starts(frame: &RenderFrame, highlight: Color, x0: f32) -> Vec<f32> {
    let highlight = rgba8(highlight.to_array());
    frame
        .shapes
        .iter()
        .filter(|s| s.stroke_width == 0.0 && rgba8(s.color) == highlight)
        .map(|s| s.screen[0] - x0)
        .collect()
}

/// `ProgressBarBusyStepDuration` is the stripes' period, and
/// `ProgressBarAnimated` off stands them still.
#[test]
fn the_busy_bar_follows_breezerc() {
    let mut d = light_desktop();
    d.breeze.progress_bar_busy_step_duration_ms = 400;
    let theme = kde::from_desktop(&d, TargetDensity::Compact);
    assert_eq!(ms(theme.motion.duration_indeterminate_sweep), 400);
    let highlight = theme.colors.accent;
    let bar = || {
        FixedSize::new()
            .width(200.0)
            .child(ProgressBar::indeterminate())
    };
    let (mut tree, id) = on_the_clock(theme.clone(), bar());
    let x0 = tree.bounds(id).x;
    // A quarter of the 400 ms period on, the stripes have moved 7 px.
    let quarter = stripe_starts(&tick(&mut tree, 100), highlight, x0);
    assert!(
        (quarter[0] - 7.0).abs() < 1.0,
        "a quarter period on: {quarter:?}"
    );

    d.breeze.progress_bar_animated = false;
    let theme = kde::from_desktop(&d, TargetDensity::Compact);
    let (mut still, id) = on_the_clock(theme.clone(), bar());
    let x0 = still.bounds(id).x;
    let at_rest = stripe_starts(&tick(&mut still, 16), highlight, x0);
    assert_eq!(at_rest[0], 0.0);
    assert_eq!(
        stripe_starts(&tick(&mut still, 300), highlight, x0),
        at_rest
    );
}

// ── native_theme ─────────────────────────────────────────────────────────

/// Fusion, Oxygen and Kvantum desktops get no Breeze theme, in any case.
#[test]
fn another_widget_style_gets_no_breeze_theme() {
    for style in [
        "Fusion",
        "fusion",
        "Oxygen",
        "kvantum",
        "Breeze-Dark",
        "Windows",
    ] {
        let mut d = light_desktop();
        d.widget_style = Some(style.to_owned());
        assert!(
            kde::native_theme(&d, TargetDensity::Compact).is_none(),
            "{style}"
        );
    }
}

/// An absent or empty widget style is Breeze, and so is `breeze` in any
/// case: the theme is `from_desktop`'s, at the density asked for.
#[test]
fn a_breeze_desktop_gets_its_theme() {
    for style in [
        None,
        Some(""),
        Some("Breeze"),
        Some("breeze"),
        Some("BREEZE"),
    ] {
        for (scheme, id) in [
            (KdeColorScheme::breeze_light(), "kde.light"),
            (KdeColorScheme::breeze_dark(), "kde.dark"),
        ] {
            let mut d = KdeDesktop::new(scheme);
            d.widget_style = style.map(str::to_owned);
            d.animation_duration_factor = Some(0.5);
            let theme = kde::native_theme(&d, TargetDensity::Comfortable)
                .unwrap_or_else(|| panic!("{style:?}: no theme"));
            assert_eq!(theme.id.as_str(), id, "{style:?}");
            assert_eq!(theme.input.density, TargetDensity::Comfortable);
            assert_eq!(ms(theme.motion.duration_normal), 100);
            let direct = kde::from_desktop(&d, TargetDensity::Comfortable);
            assert_eq!(theme.colors, direct.colors);
            assert_eq!(theme.typography, direct.typography);
            assert_eq!(theme.input, direct.input);
            assert_eq!(
                theme.extension::<KdePalette>(),
                direct.extension::<KdePalette>()
            );
        }
    }
}

/// The report the platform builds for a KDE session: the scheme's colours
/// and the desktop itself.
fn native_desktop(desktop: KdeDesktop) -> NativeDesktop {
    let mut native = NativeDesktop::new(desktop.scheme.os_theme_colors());
    native.kde = Some(desktop);
    native
}

/// The factory an app installs with `TeksiloAppBuilder::native_theme`.
fn factory() -> NativeThemeFactory {
    Box::new(|request: &NativeThemeRequest<'_>| {
        let desktop = request.desktop.kde.as_ref()?;
        kde::native_theme(desktop, request.density)
    })
}

/// Through the seam, a Breeze desktop's theme comes back as the factory
/// built it: `resolve_native_theme` sets no id and applies no density, so
/// it is `kde.dark` at the request's density until the window manager
/// names it `system`, which changes nothing else.
#[test]
fn the_theme_composes_through_the_native_seam() {
    let mut d = KdeDesktop::new(KdeColorScheme::breeze_dark());
    d.breeze.menu_opacity = 90;
    let native = native_desktop(d.clone());
    let factory = factory();
    let request = NativeThemeRequest::new(&native, true, TargetDensity::Touch);
    let theme = resolve_native_theme(Some(&factory), &request);
    assert_eq!(theme.id.as_str(), "kde.dark");
    assert_eq!(theme.input.density, TargetDensity::Touch);
    assert!(theme.extension::<KdePalette>().is_some());
    assert_eq!(
        theme
            .extension::<BreezeStyleConfig>()
            .map(|c| c.menu_opacity()),
        Some(0.9)
    );

    // What the window manager makes of it for a Touch window.
    let shown = theme.with_density(TargetDensity::Touch).with_id("system");
    assert_eq!(shown.id.as_str(), "system");
    assert_eq!(shown.colors, theme.colors);
    assert_eq!(shown.typography, theme.typography);
    assert_eq!(shown.shape, theme.shape);
    assert_eq!(shown.motion, theme.motion);
    assert_eq!(shown.input, theme.input);
    assert_eq!(shown.style_slots.installed(), theme.style_slots.installed());
    assert_eq!(
        shown.extension::<KdePalette>(),
        theme.extension::<KdePalette>()
    );
    assert_eq!(
        shown.extension::<BreezeStyleConfig>(),
        theme.extension::<BreezeStyleConfig>()
    );
    let menu = shown.style_slots.menu_item.as_ref().unwrap();
    assert_eq!(menu.metrics().item_height, 44.0);
}

/// A desktop styled otherwise falls back to the framework's IntUI coloured
/// by the desktop's report, which carries no Breeze palette or chrome; so
/// does a report without KDE settings.
#[test]
fn another_style_falls_back_through_the_seam() {
    let mut d = KdeDesktop::new(KdeColorScheme::breeze_dark());
    d.widget_style = Some("Fusion".to_owned());
    let native = native_desktop(d.clone());
    let factory = factory();
    let request = NativeThemeRequest::new(&native, true, TargetDensity::Compact);
    let theme = resolve_native_theme(Some(&factory), &request);
    assert!(theme.extension::<KdePalette>().is_none());
    assert!(theme.extension::<BreezeStyleConfig>().is_none());
    let intui = teksilo_core::presets::intui::from_os(&native.os, true);
    assert_eq!(theme.id, intui.id);
    assert_eq!(theme.colors, intui.colors);
    assert!(theme.is_dark());

    let bare = NativeDesktop::new(d.scheme.os_theme_colors());
    let request = NativeThemeRequest::new(&bare, true, TargetDensity::Compact);
    let theme = resolve_native_theme(Some(&factory), &request);
    assert!(theme.extension::<KdePalette>().is_none());
}
