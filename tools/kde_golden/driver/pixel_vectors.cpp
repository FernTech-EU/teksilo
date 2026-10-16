// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech
//
// pixels-<scheme>.json: Breeze-painted controls, sampled. Each control is
// painted through QStyle with a hand-built option (no widget, so no animation
// engine and no window-activation guesswork) into an opaque QImage filled
// with the palette's background colour, then sampled along documented lines.
// No text is painted: glyph rasterisation is not what these vectors pin.
//
// The colour scheme is whatever kdeglobals the process sees: run.sh copies
// the scheme into a scratch XDG_CONFIG_HOME first, because the Breeze style
// reads its focus and hover colours from there, not from the palette.

#include "golden_json.h"

#include <KColorScheme>
#include <KConfigGroup>
#include <KSharedConfig>

#include <QApplication>
#include <QImage>
#include <QLineEdit>
#include <QPainter>
#include <QStyle>
#include <QStyleOption>
#include <QTabBar>

#include <functional>

namespace {

QJsonArray rc(const QRect &r)
{
    return QJsonArray{r.x(), r.y(), r.width(), r.height()};
}

// Run-length encoding of one line of pixels: [[start, length, "#rrggbbaa"], ...].
QJsonArray rle(const QImage &img, bool horizontal, int at)
{
    QJsonArray out;
    const int n = horizontal ? img.width() : img.height();
    int start = 0;
    QRgb prev = 0;
    for (int i = 0; i <= n; ++i) {
        const bool end = i == n;
        const QRgb px = end ? 0 : (horizontal ? img.pixel(i, at) : img.pixel(at, i));
        if (i == 0) {
            prev = px;
            continue;
        }
        if (end || px != prev) {
            out.append(QJsonArray{start, i - start, golden::hex8(QColor::fromRgba(prev))});
            start = i;
            prev = px;
        }
    }
    return out;
}

struct Shot {
    QString id;
    QString control;
    QJsonObject state;
    QSize size;
    QPalette::ColorRole background = QPalette::Window;
    QPalette::ColorGroup group = QPalette::Active;
    std::function<void(QPainter &, const QRect &)> draw;
    QList<int> rows; // empty: the centre row
    QList<int> cols; // empty: the centre column
};

QJsonObject runShot(const Shot &s, const QPalette &pal)
{
    QImage img(s.size, QImage::Format_ARGB32);
    img.fill(pal.color(s.group, s.background));
    {
        QPainter p(&img);
        s.draw(p, QRect(QPoint(0, 0), s.size));
    }
    QJsonObject o;
    o.insert(QStringLiteral("id"), s.id);
    o.insert(QStringLiteral("control"), s.control);
    o.insert(QStringLiteral("state"), s.state);
    o.insert(QStringLiteral("size"), QJsonArray{s.size.width(), s.size.height()});
    o.insert(QStringLiteral("background"), golden::hex8(pal.color(s.group, s.background)));
    QJsonObject rows, cols;
    const QList<int> ry = s.rows.isEmpty() ? QList<int>{s.size.height() / 2} : s.rows;
    const QList<int> cx = s.cols.isEmpty() ? QList<int>{s.size.width() / 2} : s.cols;
    for (int y : ry)
        rows.insert(QString::number(y), rle(img, true, y));
    for (int x : cx)
        cols.insert(QString::number(x), rle(img, false, x));
    o.insert(QStringLiteral("rows"), rows);
    o.insert(QStringLiteral("cols"), cols);
    return o;
}

struct Ctx {
    QStyle *style;
    QPalette pal;
    QFont font;

    // Fills the fields QStyleOption::initFrom would, deterministically.
    void base(QStyleOption &opt, const QRect &r, QStyle::State extra, QPalette::ColorGroup g) const
    {
        opt.rect = r;
        opt.direction = Qt::LeftToRight;
        opt.fontMetrics = QFontMetrics(font);
        opt.palette = pal;
        opt.palette.setCurrentColorGroup(g);
        opt.state = extra;
        if (g != QPalette::Disabled)
            opt.state |= QStyle::State_Enabled;
        if (g != QPalette::Inactive)
            opt.state |= QStyle::State_Active;
        opt.styleObject = nullptr;
    }
};

struct Variant {
    QString name;
    QStyle::State state;
    QPalette::ColorGroup group = QPalette::Active;
};

QJsonObject stateDesc(const Variant &v)
{
    QJsonObject o;
    o.insert(QStringLiteral("name"), v.name);
    o.insert(QStringLiteral("flags"), int(v.state));
    o.insert(QStringLiteral("group"), v.group == QPalette::Active ? QStringLiteral("Active")
                                        : v.group == QPalette::Inactive ? QStringLiteral("Inactive")
                                                                        : QStringLiteral("Disabled"));
    return o;
}

QList<Shot> shots(const Ctx &c)
{
    QList<Shot> out;
    QStyle *st = c.style;
    const QStyle::State hover = QStyle::State_MouseOver;
    const QStyle::State focus = QStyle::State_HasFocus | QStyle::State_KeyboardFocusChange;
    const QStyle::State sunken = QStyle::State_Sunken;
    const QStyle::State on = QStyle::State_On;

    // Push button, at the size Breeze gives an "OK" button.
    {
        const QSize size(84, 34);
        const QList<QPair<Variant, QStyleOptionButton::ButtonFeatures>> vs = {
            {{QStringLiteral("normal"), QStyle::State_Raised}, QStyleOptionButton::None},
            {{QStringLiteral("hover"), QStyle::State_Raised | hover}, QStyleOptionButton::None},
            {{QStringLiteral("focus"), QStyle::State_Raised | focus}, QStyleOptionButton::None},
            {{QStringLiteral("pressed"), sunken}, QStyleOptionButton::None},
            {{QStringLiteral("default"), QStyle::State_Raised}, QStyleOptionButton::DefaultButton},
            {{QStringLiteral("disabled"), QStyle::State_Raised, QPalette::Disabled}, QStyleOptionButton::None},
            {{QStringLiteral("inactive_window"), QStyle::State_Raised, QPalette::Inactive}, QStyleOptionButton::None},
            {{QStringLiteral("flat"), QStyle::State_None}, QStyleOptionButton::Flat},
            {{QStringLiteral("flat_hover"), hover}, QStyleOptionButton::Flat},
        };
        for (const auto &v : vs) {
            Shot s;
            s.id = QStringLiteral("push_button.") + v.first.name;
            s.control = QStringLiteral("CE_PushButtonBevel");
            s.state = stateDesc(v.first);
            s.state.insert(QStringLiteral("features"), int(v.second));
            s.size = size;
            s.group = v.first.group;
            s.rows = {0, 1, 2, size.height() / 2, size.height() - 2, size.height() - 1};
            s.cols = {0, 1, 2, size.width() / 2};
            const Variant var = v.first;
            const auto feat = v.second;
            s.draw = [&c, st, var, feat](QPainter &p, const QRect &r) {
                QStyleOptionButton opt;
                c.base(opt, r, var.state, var.group);
                opt.features = feat;
                st->drawControl(QStyle::CE_PushButtonBevel, &opt, &p, nullptr);
            };
            out.append(s);
        }
    }

    // Tool button (auto-raise), 28 x 28.
    for (const Variant &v : {Variant{QStringLiteral("normal"), QStyle::State_AutoRaise | QStyle::State_Raised},
                             Variant{QStringLiteral("hover"), QStyle::State_AutoRaise | QStyle::State_Raised | hover},
                             Variant{QStringLiteral("pressed"), QStyle::State_AutoRaise | sunken | hover},
                             Variant{QStringLiteral("checked"), QStyle::State_AutoRaise | on},
                             Variant{QStringLiteral("framed"), QStyle::State_Raised}}) {
        Shot s;
        s.id = QStringLiteral("tool_button.") + v.name;
        s.control = QStringLiteral("CC_ToolButton");
        s.state = stateDesc(v);
        s.size = QSize(28, 28);
        s.rows = {0, 1, 14};
        s.cols = {0, 1, 14};
        s.draw = [&c, st, v](QPainter &p, const QRect &r) {
            QStyleOptionToolButton opt;
            c.base(opt, r, v.state, v.group);
            opt.subControls = QStyle::SC_ToolButton;
            opt.activeSubControls = (v.state & (QStyle::State_MouseOver | QStyle::State_Sunken)) ? QStyle::SC_ToolButton : QStyle::SC_None;
            opt.toolButtonStyle = Qt::ToolButtonIconOnly;
            opt.iconSize = QSize(16, 16);
            st->drawComplexControl(QStyle::CC_ToolButton, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Line edit panel.
    for (const Variant &v : {Variant{QStringLiteral("normal"), sunken}, Variant{QStringLiteral("hover"), sunken | hover},
                             Variant{QStringLiteral("focus"), sunken | focus}, Variant{QStringLiteral("disabled"), sunken, QPalette::Disabled}}) {
        Shot s;
        s.id = QStringLiteral("line_edit.") + v.name;
        s.control = QStringLiteral("PE_PanelLineEdit");
        s.state = stateDesc(v);
        s.size = QSize(150, 32);
        s.group = v.group;
        s.rows = {0, 1, 2, 16, 30, 31};
        s.cols = {0, 1, 2, 75};
        // QLineEdit::initStyleOption asks the style for the frame width with
        // the line edit itself as the widget; with no widget Breeze answers 0
        // and QCommonStyle then paints no frame at all.
        QLineEdit probe;
        probe.ensurePolished();
        QStyleOptionFrame popt;
        c.base(popt, QRect(QPoint(0, 0), s.size), v.state, v.group);
        const int lineWidth = st->pixelMetric(QStyle::PM_DefaultFrameWidth, &popt, &probe);
        s.state.insert(QStringLiteral("line_width"), lineWidth);
        s.draw = [&c, st, v, lineWidth](QPainter &p, const QRect &r) {
            QStyleOptionFrame opt;
            c.base(opt, r, v.state, v.group);
            opt.lineWidth = lineWidth;
            opt.midLineWidth = 0;
            st->drawPrimitive(QStyle::PE_PanelLineEdit, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Check box and radio indicators, at the indicator size the style reports.
    const int cbw = st->pixelMetric(QStyle::PM_IndicatorWidth);
    const int cbh = st->pixelMetric(QStyle::PM_IndicatorHeight);
    for (const Variant &v : {Variant{QStringLiteral("off"), QStyle::State_Off}, Variant{QStringLiteral("on"), on},
                             Variant{QStringLiteral("partial"), QStyle::State_NoChange}, Variant{QStringLiteral("hover_off"), QStyle::State_Off | hover},
                             Variant{QStringLiteral("focus_on"), on | focus}, Variant{QStringLiteral("pressed_off"), QStyle::State_Off | sunken},
                             Variant{QStringLiteral("disabled_on"), on, QPalette::Disabled}}) {
        Shot s;
        s.id = QStringLiteral("check_box.") + v.name;
        s.control = QStringLiteral("PE_IndicatorCheckBox");
        s.state = stateDesc(v);
        s.size = QSize(cbw, cbh);
        s.group = v.group;
        s.rows = {cbh / 2, cbh / 2 - 2, cbh / 2 + 2, 2, 3};
        s.cols = {cbw / 2, cbw / 2 - 2, 2, 3};
        s.draw = [&c, st, v](QPainter &p, const QRect &r) {
            QStyleOptionButton opt;
            c.base(opt, r, v.state, v.group);
            st->drawPrimitive(QStyle::PE_IndicatorCheckBox, &opt, &p, nullptr);
        };
        out.append(s);
    }
    const int rbw = st->pixelMetric(QStyle::PM_ExclusiveIndicatorWidth);
    const int rbh = st->pixelMetric(QStyle::PM_ExclusiveIndicatorHeight);
    for (const Variant &v : {Variant{QStringLiteral("off"), QStyle::State_Off}, Variant{QStringLiteral("on"), on},
                             Variant{QStringLiteral("hover_off"), QStyle::State_Off | hover}, Variant{QStringLiteral("focus_on"), on | focus},
                             Variant{QStringLiteral("disabled_on"), on, QPalette::Disabled}}) {
        Shot s;
        s.id = QStringLiteral("radio_button.") + v.name;
        s.control = QStringLiteral("PE_IndicatorRadioButton");
        s.state = stateDesc(v);
        s.size = QSize(rbw, rbh);
        s.group = v.group;
        s.rows = {rbh / 2, 3};
        s.cols = {rbw / 2};
        s.draw = [&c, st, v](QPainter &p, const QRect &r) {
            QStyleOptionButton opt;
            c.base(opt, r, v.state, v.group);
            st->drawPrimitive(QStyle::PE_IndicatorRadioButton, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Combo box frame and arrow (no label).
    for (const auto &[v, editable] : QList<QPair<Variant, bool>>{{{QStringLiteral("normal"), QStyle::State_None}, false},
                                                                   {{QStringLiteral("hover"), hover}, false},
                                                                   {{QStringLiteral("focus"), focus}, false},
                                                                   {{QStringLiteral("editable"), QStyle::State_None}, true}}) {
        Shot s;
        s.id = QStringLiteral("combo_box.") + v.name;
        s.control = QStringLiteral("CC_ComboBox");
        s.state = stateDesc(v);
        s.state.insert(QStringLiteral("editable"), editable);
        s.size = QSize(150, 32);
        s.rows = {0, 1, 16, 31};
        s.cols = {0, 1, 40, 140};
        const Variant var = v;
        const bool ed = editable;
        s.draw = [&c, st, var, ed](QPainter &p, const QRect &r) {
            QStyleOptionComboBox opt;
            c.base(opt, r, var.state, var.group);
            opt.editable = ed;
            opt.frame = true;
            opt.subControls = QStyle::SC_All;
            st->drawComplexControl(QStyle::CC_ComboBox, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Spin box.
    for (const Variant &v : {Variant{QStringLiteral("normal"), QStyle::State_None}, Variant{QStringLiteral("hover"), hover}}) {
        Shot s;
        s.id = QStringLiteral("spin_box.") + v.name;
        s.control = QStringLiteral("CC_SpinBox");
        s.state = stateDesc(v);
        s.size = QSize(100, 32);
        s.rows = {0, 8, 16, 24, 31};
        s.cols = {0, 40, 90};
        s.draw = [&c, st, v](QPainter &p, const QRect &r) {
            QStyleOptionSpinBox opt;
            c.base(opt, r, v.state, v.group);
            opt.frame = true;
            opt.buttonSymbols = QAbstractSpinBox::UpDownArrows;
            opt.stepEnabled = QAbstractSpinBox::StepUpEnabled | QAbstractSpinBox::StepDownEnabled;
            opt.subControls = QStyle::SC_All;
            st->drawComplexControl(QStyle::CC_SpinBox, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Slider, value 30 of 0..100.
    for (const Variant &v : {Variant{QStringLiteral("normal"), QStyle::State_Horizontal}, Variant{QStringLiteral("handle_hover"), QStyle::State_Horizontal | hover},
                             Variant{QStringLiteral("focus"), QStyle::State_Horizontal | focus}, Variant{QStringLiteral("disabled"), QStyle::State_Horizontal, QPalette::Disabled}}) {
        QStyleOptionSlider probe;
        c.base(probe, QRect(), v.state, v.group);
        probe.orientation = Qt::Horizontal;
        probe.minimum = 0;
        probe.maximum = 100;
        probe.sliderPosition = 30;
        probe.sliderValue = 30;
        const int thick = st->sizeFromContents(QStyle::CT_Slider, &probe, QSize(200, st->pixelMetric(QStyle::PM_SliderThickness, &probe, nullptr)), nullptr).height();
        probe.rect = QRect(0, 0, 200, thick);
        probe.subControls = QStyle::SC_All;
        const QRect handle = st->subControlRect(QStyle::CC_Slider, &probe, QStyle::SC_SliderHandle, nullptr);
        Shot s;
        s.id = QStringLiteral("slider.") + v.name;
        s.control = QStringLiteral("CC_Slider");
        s.state = stateDesc(v);
        s.state.insert(QStringLiteral("value"), 30);
        s.state.insert(QStringLiteral("handle_rect"), rc(handle));
        s.size = QSize(200, thick);
        s.group = v.group;
        s.rows = {thick / 2};
        s.cols = {handle.center().x(), 10, 190};
        s.draw = [&c, st, v](QPainter &p, const QRect &r) {
            QStyleOptionSlider opt;
            c.base(opt, r, v.state, v.group);
            opt.orientation = Qt::Horizontal;
            opt.minimum = 0;
            opt.maximum = 100;
            opt.sliderPosition = 30;
            opt.sliderValue = 30;
            opt.subControls = QStyle::SC_SliderGroove | QStyle::SC_SliderHandle;
            opt.activeSubControls = (v.state & QStyle::State_MouseOver) ? QStyle::SC_SliderHandle : QStyle::SC_None;
            st->drawComplexControl(QStyle::CC_Slider, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Vertical scroll bar, value 0 of 0..100, page 20.
    for (const Variant &v : {Variant{QStringLiteral("normal"), QStyle::State_None}, Variant{QStringLiteral("hover"), hover},
                             Variant{QStringLiteral("slider_hover"), hover}, Variant{QStringLiteral("slider_pressed"), hover | sunken}}) {
        QStyleOptionSlider probe;
        c.base(probe, QRect(), v.state, v.group);
        const int ext = st->pixelMetric(QStyle::PM_ScrollBarExtent, &probe, nullptr);
        probe.rect = QRect(0, 0, ext, 200);
        probe.orientation = Qt::Vertical;
        probe.minimum = 0;
        probe.maximum = 100;
        probe.pageStep = 20;
        probe.singleStep = 1;
        probe.subControls = QStyle::SC_All;
        const QRect slider = st->subControlRect(QStyle::CC_ScrollBar, &probe, QStyle::SC_ScrollBarSlider, nullptr);
        const bool sliderActive = v.name.startsWith(QStringLiteral("slider"));
        Shot s;
        s.id = QStringLiteral("scroll_bar.") + v.name;
        s.control = QStringLiteral("CC_ScrollBar");
        s.state = stateDesc(v);
        s.state.insert(QStringLiteral("slider_rect"), rc(slider));
        s.state.insert(QStringLiteral("active_sub_control"), sliderActive ? QStringLiteral("SC_ScrollBarSlider") : QStringLiteral("none"));
        s.size = QSize(ext, 200);
        s.rows = {slider.center().y(), 150};
        s.cols = {0, ext / 2};
        s.draw = [&c, st, v, sliderActive](QPainter &p, const QRect &r) {
            QStyleOptionSlider opt;
            c.base(opt, r, v.state, v.group);
            opt.orientation = Qt::Vertical;
            opt.minimum = 0;
            opt.maximum = 100;
            opt.pageStep = 20;
            opt.singleStep = 1;
            opt.subControls = QStyle::SC_All;
            opt.activeSubControls = sliderActive ? QStyle::SC_ScrollBarSlider : QStyle::SC_None;
            st->drawComplexControl(QStyle::CC_ScrollBar, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Progress bar at 50 %, no text.
    {
        QStyleOptionProgressBar probe;
        c.base(probe, QRect(), QStyle::State_Horizontal, QPalette::Active);
        probe.minimum = 0;
        probe.maximum = 100;
        probe.progress = 50;
        probe.textVisible = false;
        const QSize hint = st->sizeFromContents(QStyle::CT_ProgressBar, &probe, QSize(200, probe.fontMetrics.height()), nullptr);
        Shot s;
        s.id = QStringLiteral("progress_bar.half");
        s.control = QStringLiteral("CE_ProgressBar");
        s.state = stateDesc({QStringLiteral("half"), QStyle::State_Horizontal});
        s.size = QSize(200, hint.height());
        s.rows = {hint.height() / 2};
        s.cols = {50, 150};
        s.draw = [&c, st](QPainter &p, const QRect &r) {
            QStyleOptionProgressBar opt;
            c.base(opt, r, QStyle::State_Horizontal, QPalette::Active);
            opt.minimum = 0;
            opt.maximum = 100;
            opt.progress = 50;
            opt.textVisible = false;
            st->drawControl(QStyle::CE_ProgressBar, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Menu panel and menu item rows (empty text: the highlight is the subject).
    {
        Shot s;
        s.id = QStringLiteral("menu.panel");
        s.control = QStringLiteral("PE_PanelMenu+PE_FrameMenu");
        s.state = stateDesc({QStringLiteral("normal"), QStyle::State_None});
        s.size = QSize(200, 100);
        s.rows = {0, 1, 50};
        s.cols = {0, 1, 100};
        s.draw = [&c, st](QPainter &p, const QRect &r) {
            QStyleOption opt;
            c.base(opt, r, QStyle::State_None, QPalette::Active);
            st->drawPrimitive(QStyle::PE_PanelMenu, &opt, &p, nullptr);
            QStyleOptionFrame fr;
            c.base(fr, r, QStyle::State_None, QPalette::Active);
            fr.lineWidth = st->pixelMetric(QStyle::PM_MenuPanelWidth, &fr, nullptr);
            st->drawPrimitive(QStyle::PE_FrameMenu, &fr, &p, nullptr);
        };
        out.append(s);
    }
    for (const auto &[v, type] : QList<QPair<Variant, QStyleOptionMenuItem::MenuItemType>>{
             {{QStringLiteral("normal"), QStyle::State_None}, QStyleOptionMenuItem::Normal},
             {{QStringLiteral("selected"), QStyle::State_Selected}, QStyleOptionMenuItem::Normal},
             {{QStringLiteral("separator"), QStyle::State_None}, QStyleOptionMenuItem::Separator}}) {
        QStyleOptionMenuItem probe;
        c.base(probe, QRect(), v.state, v.group);
        probe.menuItemType = type;
        probe.maxIconWidth = 16;
        const QSize hint = st->sizeFromContents(QStyle::CT_MenuItem, &probe, QSize(0, probe.fontMetrics.height()), nullptr);
        Shot s;
        s.id = QStringLiteral("menu_item.") + v.name;
        s.control = QStringLiteral("CE_MenuItem");
        s.state = stateDesc(v);
        s.state.insert(QStringLiteral("type"), int(type));
        s.size = QSize(200, hint.height());
        s.background = QPalette::Window;
        s.rows = {hint.height() / 2, 0, 1, 2, 3, 4, 5};
        s.cols = {100, 4, 5, 6};
        const Variant var = v;
        const auto ty = type;
        s.draw = [&c, st, var, ty](QPainter &p, const QRect &r) {
            QStyleOptionMenuItem opt;
            c.base(opt, r, var.state, var.group);
            opt.menuItemType = ty;
            opt.maxIconWidth = 16;
            opt.menuRect = r;
            st->drawControl(QStyle::CE_MenuItem, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Tab shapes.
    for (const auto &[v, doc] : QList<QPair<Variant, bool>>{{{QStringLiteral("selected"), QStyle::State_Selected}, false},
                                                            {{QStringLiteral("unselected"), QStyle::State_None}, false},
                                                            {{QStringLiteral("hover"), hover}, false},
                                                            {{QStringLiteral("selected.document_mode"), QStyle::State_Selected}, true},
                                                            {{QStringLiteral("unselected.document_mode"), QStyle::State_None}, true}}) {
        Shot s;
        s.id = QStringLiteral("tab.") + v.name;
        s.control = QStringLiteral("CE_TabBarTabShape");
        s.state = stateDesc(v);
        s.state.insert(QStringLiteral("document_mode"), doc);
        s.size = QSize(80, 30);
        s.rows = {0, 1, 2, 3, 15, 27, 28, 29};
        s.cols = {0, 1, 40, 79};
        const Variant var = v;
        const bool d = doc;
        s.draw = [&c, st, var, d](QPainter &p, const QRect &r) {
            QStyleOptionTab opt;
            c.base(opt, r, var.state, var.group);
            opt.shape = QTabBar::RoundedNorth;
            opt.position = QStyleOptionTab::Middle;
            opt.selectedPosition = QStyleOptionTab::NotAdjacent;
            opt.documentMode = d;
            st->drawControl(QStyle::CE_TabBarTabShape, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Header section.
    for (const Variant &v : {Variant{QStringLiteral("normal"), QStyle::State_Raised}, Variant{QStringLiteral("hover"), QStyle::State_Raised | hover},
                             Variant{QStringLiteral("sunken"), sunken}}) {
        Shot s;
        s.id = QStringLiteral("header.") + v.name;
        s.control = QStringLiteral("CE_HeaderSection");
        s.state = stateDesc(v);
        s.size = QSize(100, 30);
        s.background = QPalette::Base;
        s.rows = {0, 15, 28, 29};
        s.cols = {0, 50, 98, 99};
        s.draw = [&c, st, v](QPainter &p, const QRect &r) {
            QStyleOptionHeader opt;
            c.base(opt, r, v.state | QStyle::State_Horizontal, v.group);
            opt.orientation = Qt::Horizontal;
            opt.section = 0;
            opt.position = QStyleOptionHeader::Middle;
            st->drawControl(QStyle::CE_HeaderSection, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Item-view row backgrounds.
    for (const Variant &v : {Variant{QStringLiteral("normal"), QStyle::State_None}, Variant{QStringLiteral("hover"), hover},
                             Variant{QStringLiteral("selected"), QStyle::State_Selected}, Variant{QStringLiteral("selected_hover"), QStyle::State_Selected | hover},
                             Variant{QStringLiteral("selected_inactive_window"), QStyle::State_Selected, QPalette::Inactive},
                             Variant{QStringLiteral("focus"), focus}}) {
        Shot s;
        s.id = QStringLiteral("item_view_row.") + v.name;
        s.control = QStringLiteral("PE_PanelItemViewItem");
        s.state = stateDesc(v);
        s.size = QSize(200, 24);
        s.background = QPalette::Base;
        s.group = v.group;
        s.rows = {0, 1, 2, 12, 22, 23};
        s.cols = {0, 1, 2, 3, 100};
        s.draw = [&c, st, v](QPainter &p, const QRect &r) {
            QStyleOptionViewItem opt;
            c.base(opt, r, v.state, v.group);
            opt.features = QStyleOptionViewItem::HasDisplay;
            opt.viewItemPosition = QStyleOptionViewItem::OnlyOne;
            opt.showDecorationSelected = true;
            st->drawPrimitive(QStyle::PE_PanelItemViewItem, &opt, &p, nullptr);
        };
        out.append(s);
    }

    // Tooltip panel, generic frame, group box frame.
    {
        Shot s;
        s.id = QStringLiteral("tooltip.panel");
        s.control = QStringLiteral("PE_PanelTipLabel");
        s.state = stateDesc({QStringLiteral("normal"), QStyle::State_None});
        s.size = QSize(100, 30);
        s.background = QPalette::ToolTipBase;
        s.rows = {0, 1, 15};
        s.cols = {0, 1, 50};
        s.draw = [&c, st](QPainter &p, const QRect &r) {
            QStyleOptionFrame opt;
            c.base(opt, r, QStyle::State_None, QPalette::Active);
            st->drawPrimitive(QStyle::PE_PanelTipLabel, &opt, &p, nullptr);
        };
        out.append(s);
    }
    {
        Shot s;
        s.id = QStringLiteral("frame.styled_panel");
        s.control = QStringLiteral("PE_Frame");
        s.state = stateDesc({QStringLiteral("sunken"), sunken});
        s.size = QSize(100, 50);
        s.rows = {0, 1, 25};
        s.cols = {0, 1, 50};
        s.draw = [&c, st](QPainter &p, const QRect &r) {
            QStyleOptionFrame opt;
            c.base(opt, r, QStyle::State_Sunken, QPalette::Active);
            opt.lineWidth = st->pixelMetric(QStyle::PM_DefaultFrameWidth, &opt, nullptr);
            opt.frameShape = QFrame::StyledPanel;
            st->drawPrimitive(QStyle::PE_Frame, &opt, &p, nullptr);
        };
        out.append(s);
    }
    {
        Shot s;
        s.id = QStringLiteral("group_box.frame");
        s.control = QStringLiteral("PE_FrameGroupBox");
        s.state = stateDesc({QStringLiteral("normal"), QStyle::State_None});
        s.size = QSize(100, 60);
        s.rows = {0, 1, 2, 30};
        s.cols = {0, 1, 2, 50};
        s.draw = [&c, st](QPainter &p, const QRect &r) {
            QStyleOptionFrame opt;
            c.base(opt, r, QStyle::State_None, QPalette::Active);
            opt.lineWidth = 1;
            st->drawPrimitive(QStyle::PE_FrameGroupBox, &opt, &p, nullptr);
        };
        out.append(s);
    }
    return out;
}

} // namespace

int writePixels(const QString &path, const QString &schemeLabel)
{
    KSharedConfigPtr config = KSharedConfig::openConfig();
    const QPalette pal = KColorScheme::createApplicationPalette(config);
    QApplication::setPalette(pal);
    QFont font(QStringLiteral("Noto Sans"));
    font.setPointSizeF(10);
    QApplication::setFont(font);

    Ctx c{QApplication::style(), pal, font};
    QJsonArray records;
    const QList<Shot> all = shots(c);
    for (const Shot &s : all)
        records.append(runShot(s, pal));

    QJsonObject env;
    env.insert(QStringLiteral("scheme"), schemeLabel);
    env.insert(QStringLiteral("kdeglobals_color_scheme"), config->group(QStringLiteral("General")).readEntry("ColorScheme", QString()));
    env.insert(QStringLiteral("style"), c.style ? c.style->name() : QStringLiteral("none"));
    env.insert(QStringLiteral("platform"), QGuiApplication::platformName());
    env.insert(QStringLiteral("image_format"), QStringLiteral("ARGB32, filled opaque with the palette background, then painted"));

    return golden::writeFile(path,
                             {
                                 {QStringLiteral("meta"), golden::meta(QStringLiteral("pixels"))},
                                 {QStringLiteral("environment"), env},
                                 {QStringLiteral("records"), records, true},
                             })
        ? 0
        : 1;
}
