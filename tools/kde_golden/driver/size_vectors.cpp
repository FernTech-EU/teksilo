// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech
//
// sizes.json: what the Breeze QStyle reports, through QStyle and through the
// widgets' own sizeHint, under QT_QPA_PLATFORM=offscreen. Every widget is
// created hidden, polished with the Breeze style, measured, then destroyed.

#include "golden_json.h"

#include <QAction>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QFontDatabase>
#include <QFrame>
#include <QGroupBox>
#include <QMenuBar>
#include <QStatusBar>
#include <QTabWidget>
#include <QToolBar>
#include <QFontInfo>
#include <QFontMetrics>
#include <QFontMetricsF>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QListView>
#include <QMenu>
#include <QMetaEnum>
#include <QPixmap>
#include <QProgressBar>
#include <QPushButton>
#include <QRadioButton>
#include <QScreen>
#include <QScrollBar>
#include <QSlider>
#include <QSpinBox>
#include <QStandardItemModel>
#include <QStyle>
#include <QStyleOption>
#include <QTabBar>
#include <QTableView>
#include <QToolButton>
#include <QToolTip>
#include <QTreeView>

#include <functional>
#include <memory>

namespace {

// Every measured widget except a QMenu is given this parent, so the style
// sees an ordinary child widget rather than a top-level window (Breeze, for
// one, answers the layout margins differently for a window).
QWidget *gHost = nullptr;

void adopt(QWidget *w)
{
    if (gHost && !w->parentWidget() && !w->inherits("QMenu"))
        w->setParent(gHost);
}

QJsonArray sz(const QSize &s)
{
    return QJsonArray{s.width(), s.height()};
}

QJsonArray rc(const QRect &r)
{
    return QJsonArray{r.x(), r.y(), r.width(), r.height()};
}

QIcon testIcon()
{
    QPixmap pm(64, 64);
    pm.fill(QColor(61, 174, 233));
    return QIcon(pm);
}

QJsonObject fontRecord(const QFont &font)
{
    const QFontInfo info(font);
    const QFontMetrics fm(font);
    const QFontMetricsF fmf(font);
    QJsonObject o;
    o.insert(QStringLiteral("requested_family"), font.family());
    o.insert(QStringLiteral("requested_point_size"), font.pointSizeF());
    o.insert(QStringLiteral("resolved_family"), info.family());
    o.insert(QStringLiteral("resolved_style"), info.styleName());
    o.insert(QStringLiteral("resolved_point_size"), info.pointSizeF());
    o.insert(QStringLiteral("resolved_pixel_size"), info.pixelSize());
    o.insert(QStringLiteral("exact_match"), info.exactMatch());
    o.insert(QStringLiteral("to_string"), font.toString());
    QJsonObject i;
    i.insert(QStringLiteral("height"), fm.height());
    i.insert(QStringLiteral("ascent"), fm.ascent());
    i.insert(QStringLiteral("descent"), fm.descent());
    i.insert(QStringLiteral("leading"), fm.leading());
    i.insert(QStringLiteral("line_spacing"), fm.lineSpacing());
    i.insert(QStringLiteral("x_height"), fm.xHeight());
    i.insert(QStringLiteral("cap_height"), fm.capHeight());
    i.insert(QStringLiteral("average_char_width"), fm.averageCharWidth());
    i.insert(QStringLiteral("max_width"), fm.maxWidth());
    o.insert(QStringLiteral("metrics"), i);
    QJsonObject f;
    f.insert(QStringLiteral("height"), fmf.height());
    f.insert(QStringLiteral("ascent"), fmf.ascent());
    f.insert(QStringLiteral("descent"), fmf.descent());
    f.insert(QStringLiteral("leading"), fmf.leading());
    f.insert(QStringLiteral("line_spacing"), fmf.lineSpacing());
    f.insert(QStringLiteral("x_height"), fmf.xHeight());
    f.insert(QStringLiteral("cap_height"), fmf.capHeight());
    f.insert(QStringLiteral("average_char_width"), fmf.averageCharWidth());
    f.insert(QStringLiteral("max_width"), fmf.maxWidth());
    o.insert(QStringLiteral("metrics_f"), f);
    QJsonObject adv;
    for (const QString &s : {QStringLiteral("M"), QStringLiteral("x"), QStringLiteral("0"), QStringLiteral("OK"),
                             QStringLiteral("Cancel"), QStringLiteral("The quick brown fox")}) {
        adv.insert(s, QJsonArray{fm.horizontalAdvance(s), fmf.horizontalAdvance(s)});
    }
    o.insert(QStringLiteral("advance"), adv);
    return o;
}

struct Recorder {
    QJsonArray out;

    void add(const QString &id, QWidget *w, const QJsonObject &config = {}, const QJsonObject &extra = {})
    {
        adopt(w);
        w->ensurePolished();
        QJsonObject o;
        o.insert(QStringLiteral("id"), id);
        o.insert(QStringLiteral("class"), QString::fromLatin1(w->metaObject()->className()));
        if (!config.isEmpty())
            o.insert(QStringLiteral("config"), config);
        o.insert(QStringLiteral("size_hint"), sz(w->sizeHint()));
        o.insert(QStringLiteral("minimum_size_hint"), sz(w->minimumSizeHint()));
        for (auto it = extra.begin(); it != extra.end(); ++it)
            o.insert(it.key(), it.value());
        out.append(o);
    }
};

QJsonObject cfg(std::initializer_list<QPair<QString, QJsonValue>> kv)
{
    QJsonObject o;
    for (const auto &p : kv)
        o.insert(p.first, p.second);
    return o;
}

QStandardItemModel *rowsModel(QObject *parent, bool icon, bool check)
{
    auto *m = new QStandardItemModel(parent);
    m->setHorizontalHeaderLabels({QStringLiteral("Name"), QStringLiteral("Size")});
    for (const QString &t : {QStringLiteral("Row one"), QStringLiteral("Row two")}) {
        auto *a = new QStandardItem(t);
        if (icon)
            a->setIcon(testIcon());
        if (check) {
            a->setCheckable(true);
            a->setCheckState(Qt::Checked);
        }
        m->appendRow({a, new QStandardItem(QStringLiteral("12 KiB"))});
    }
    return m;
}

QJsonArray widgetsFor()
{
    Recorder r;
    const QIcon icon = testIcon();

    {
        QLabel w(QStringLiteral("Label"));
        r.add(QStringLiteral("label"), &w, cfg({{QStringLiteral("text"), QStringLiteral("Label")}}));
    }
    for (const QString &t : {QStringLiteral("OK"), QStringLiteral("Cancel"), QStringLiteral("Apply changes")}) {
        QPushButton w(t);
        r.add(QStringLiteral("push_button"), &w, cfg({{QStringLiteral("text"), t}}));
    }
    {
        QPushButton w(icon, QStringLiteral("Save"));
        r.add(QStringLiteral("push_button.icon"), &w, cfg({{QStringLiteral("text"), QStringLiteral("Save")}, {QStringLiteral("icon"), true}}));
    }
    {
        QPushButton w(QStringLiteral("OK"));
        w.setDefault(true);
        r.add(QStringLiteral("push_button.default"), &w, cfg({{QStringLiteral("text"), QStringLiteral("OK")}, {QStringLiteral("default"), true}}));
    }
    {
        QPushButton w(QStringLiteral("OK"));
        w.setFlat(true);
        r.add(QStringLiteral("push_button.flat"), &w, cfg({{QStringLiteral("text"), QStringLiteral("OK")}, {QStringLiteral("flat"), true}}));
    }
    {
        QPushButton w;
        w.setIcon(icon);
        r.add(QStringLiteral("push_button.icon_only"), &w, cfg({{QStringLiteral("icon"), true}}));
    }

    struct Tb {
        QString id;
        bool autoRaise;
        int iconSize;
        Qt::ToolButtonStyle style;
        QToolButton::ToolButtonPopupMode popup;
        bool menu;
    };
    const QList<Tb> tbs = {
        {QStringLiteral("tool_button.auto_raise.16"), true, 16, Qt::ToolButtonIconOnly, QToolButton::DelayedPopup, false},
        {QStringLiteral("tool_button.auto_raise.22"), true, 22, Qt::ToolButtonIconOnly, QToolButton::DelayedPopup, false},
        {QStringLiteral("tool_button.framed.16"), false, 16, Qt::ToolButtonIconOnly, QToolButton::DelayedPopup, false},
        {QStringLiteral("tool_button.framed.22"), false, 22, Qt::ToolButtonIconOnly, QToolButton::DelayedPopup, false},
        {QStringLiteral("tool_button.text_beside_icon.16"), true, 16, Qt::ToolButtonTextBesideIcon, QToolButton::DelayedPopup, false},
        {QStringLiteral("tool_button.text_under_icon.22"), true, 22, Qt::ToolButtonTextUnderIcon, QToolButton::DelayedPopup, false},
        {QStringLiteral("tool_button.menu_button_popup.16"), true, 16, Qt::ToolButtonIconOnly, QToolButton::MenuButtonPopup, true},
        {QStringLiteral("tool_button.instant_popup.16"), true, 16, Qt::ToolButtonIconOnly, QToolButton::InstantPopup, true},
    };
    for (const Tb &t : tbs) {
        QToolButton w;
        QMenu menu;
        w.setIcon(icon);
        w.setText(QStringLiteral("Tool"));
        w.setAutoRaise(t.autoRaise);
        w.setIconSize(QSize(t.iconSize, t.iconSize));
        w.setToolButtonStyle(t.style);
        if (t.menu) {
            menu.addAction(QStringLiteral("Entry"));
            w.setMenu(&menu);
            w.setPopupMode(t.popup);
        }
        r.add(t.id, &w,
              cfg({{QStringLiteral("auto_raise"), t.autoRaise},
                   {QStringLiteral("icon_size"), t.iconSize},
                   {QStringLiteral("text"), QStringLiteral("Tool")},
                   {QStringLiteral("style"), int(t.style)},
                   {QStringLiteral("popup_mode"), t.menu ? int(t.popup) : -1}}));
    }

    {
        QLineEdit w;
        r.add(QStringLiteral("line_edit"), &w);
    }
    {
        QLineEdit w(QStringLiteral("Text"));
        w.setClearButtonEnabled(true);
        r.add(QStringLiteral("line_edit.clear_button"), &w, cfg({{QStringLiteral("clear_button"), true}}));
    }
    {
        QLineEdit w;
        w.setFrame(false);
        r.add(QStringLiteral("line_edit.frameless"), &w, cfg({{QStringLiteral("frame"), false}}));
    }
    {
        QComboBox w;
        w.addItems({QStringLiteral("Item"), QStringLiteral("Longer item text")});
        r.add(QStringLiteral("combo_box"), &w, cfg({{QStringLiteral("items"), QJsonArray{QStringLiteral("Item"), QStringLiteral("Longer item text")}}}));
    }
    {
        QComboBox w;
        w.setEditable(true);
        w.addItems({QStringLiteral("Item"), QStringLiteral("Longer item text")});
        r.add(QStringLiteral("combo_box.editable"), &w, cfg({{QStringLiteral("editable"), true}}));
    }
    {
        QComboBox w;
        w.addItem(icon, QStringLiteral("Item"));
        r.add(QStringLiteral("combo_box.icon"), &w, cfg({{QStringLiteral("icon"), true}}));
    }
    {
        QSpinBox w;
        w.setRange(0, 99);
        r.add(QStringLiteral("spin_box.0_99"), &w, cfg({{QStringLiteral("range"), QJsonArray{0, 99}}}));
    }
    {
        QSpinBox w;
        w.setRange(0, 9999);
        r.add(QStringLiteral("spin_box.0_9999"), &w, cfg({{QStringLiteral("range"), QJsonArray{0, 9999}}}));
    }
    {
        QDoubleSpinBox w;
        w.setRange(0, 100);
        w.setDecimals(2);
        r.add(QStringLiteral("double_spin_box"), &w, cfg({{QStringLiteral("range"), QJsonArray{0, 100}}, {QStringLiteral("decimals"), 2}}));
    }
    {
        QSpinBox w;
        w.setRange(0, 99);
        w.setButtonSymbols(QAbstractSpinBox::NoButtons);
        r.add(QStringLiteral("spin_box.no_buttons"), &w, cfg({{QStringLiteral("range"), QJsonArray{0, 99}}, {QStringLiteral("buttons"), false}}));
    }
    {
        QCheckBox w(QStringLiteral("Check"));
        r.add(QStringLiteral("check_box"), &w, cfg({{QStringLiteral("text"), QStringLiteral("Check")}}));
    }
    {
        QCheckBox w;
        r.add(QStringLiteral("check_box.no_text"), &w);
    }
    {
        QRadioButton w(QStringLiteral("Radio"));
        r.add(QStringLiteral("radio_button"), &w, cfg({{QStringLiteral("text"), QStringLiteral("Radio")}}));
    }
    {
        QRadioButton w;
        r.add(QStringLiteral("radio_button.no_text"), &w);
    }

    struct Sl {
        QString id;
        Qt::Orientation o;
        QSlider::TickPosition t;
    };
    for (const Sl &s : {Sl{QStringLiteral("slider.horizontal"), Qt::Horizontal, QSlider::NoTicks},
                        Sl{QStringLiteral("slider.horizontal.ticks_below"), Qt::Horizontal, QSlider::TicksBelow},
                        Sl{QStringLiteral("slider.horizontal.ticks_both"), Qt::Horizontal, QSlider::TicksBothSides},
                        Sl{QStringLiteral("slider.vertical"), Qt::Vertical, QSlider::NoTicks}}) {
        QSlider w(s.o);
        w.setTickPosition(s.t);
        r.add(s.id, &w, cfg({{QStringLiteral("orientation"), int(s.o)}, {QStringLiteral("ticks"), int(s.t)}}));
    }
    for (Qt::Orientation o : {Qt::Horizontal, Qt::Vertical}) {
        QScrollBar w(o);
        r.add(o == Qt::Horizontal ? QStringLiteral("scroll_bar.horizontal") : QStringLiteral("scroll_bar.vertical"), &w);
    }
    {
        QProgressBar w;
        w.setValue(50);
        r.add(QStringLiteral("progress_bar"), &w, cfg({{QStringLiteral("text_visible"), true}}));
    }
    {
        QProgressBar w;
        w.setValue(50);
        w.setTextVisible(false);
        r.add(QStringLiteral("progress_bar.no_text"), &w, cfg({{QStringLiteral("text_visible"), false}}));
    }
    {
        QProgressBar w;
        w.setOrientation(Qt::Vertical);
        w.setValue(50);
        r.add(QStringLiteral("progress_bar.vertical"), &w, cfg({{QStringLiteral("orientation"), int(Qt::Vertical)}}));
    }

    // Menus: action rows, read back through actionGeometry.
    for (bool icons : {true, false}) {
        QMenu menu;
        QMenu sub(QStringLiteral("Submenu"));
        sub.addAction(QStringLiteral("Inner"));
        QAction *a = menu.addAction(QStringLiteral("Open"));
        if (icons)
            a->setIcon(icon);
        QAction *b = menu.addAction(QStringLiteral("Save"));
        b->setShortcut(QKeySequence(QStringLiteral("Ctrl+S")));
        menu.addSeparator();
        menu.addSection(QStringLiteral("Section"));
        QAction *c = menu.addAction(QStringLiteral("Checkable"));
        c->setCheckable(true);
        c->setChecked(true);
        menu.addMenu(&sub);
        adopt(&menu);
        menu.ensurePolished();
        QJsonArray rows;
        const QList<QAction *> actions = menu.actions();
        for (QAction *act : actions) {
            QJsonObject row;
            QString kind = QStringLiteral("item");
            if (act->isSeparator())
                kind = act->text().isEmpty() ? QStringLiteral("separator") : QStringLiteral("section");
            else if (act->menu())
                kind = QStringLiteral("submenu");
            else if (act->isCheckable())
                kind = QStringLiteral("checkable");
            else if (!act->shortcut().isEmpty())
                kind = QStringLiteral("shortcut");
            row.insert(QStringLiteral("kind"), kind);
            row.insert(QStringLiteral("text"), act->text());
            row.insert(QStringLiteral("icon"), !act->icon().isNull());
            row.insert(QStringLiteral("geometry"), rc(menu.actionGeometry(act)));
            rows.append(row);
        }
        r.add(icons ? QStringLiteral("menu.with_icon") : QStringLiteral("menu.no_icons"), &menu, cfg({{QStringLiteral("icons"), icons}}),
              cfg({{QStringLiteral("rows"), rows}}));
    }

    // Tab bars.
    struct Tab {
        QString id;
        bool document;
        bool closable;
        QTabBar::Shape shape;
    };
    for (const Tab &t : {Tab{QStringLiteral("tab_bar"), false, false, QTabBar::RoundedNorth},
                         Tab{QStringLiteral("tab_bar.document_mode"), true, false, QTabBar::RoundedNorth},
                         Tab{QStringLiteral("tab_bar.closable"), false, true, QTabBar::RoundedNorth},
                         Tab{QStringLiteral("tab_bar.west"), false, false, QTabBar::RoundedWest},
                         Tab{QStringLiteral("tab_bar.south"), false, false, QTabBar::RoundedSouth}}) {
        QTabBar w;
        w.setDocumentMode(t.document);
        w.setTabsClosable(t.closable);
        w.setShape(t.shape);
        w.setExpanding(false);
        for (const QString &s : {QStringLiteral("Tab"), QStringLiteral("Second tab"), QStringLiteral("Third")})
            w.addTab(s);
        adopt(&w);
        w.ensurePolished();
        w.resize(w.sizeHint());
        QJsonArray tabs;
        for (int i = 0; i < w.count(); ++i)
            tabs.append(rc(w.tabRect(i)));
        r.add(t.id, &w,
              cfg({{QStringLiteral("document_mode"), t.document}, {QStringLiteral("closable"), t.closable}, {QStringLiteral("shape"), int(t.shape)},
                   {QStringLiteral("tabs"), QJsonArray{QStringLiteral("Tab"), QStringLiteral("Second tab"), QStringLiteral("Third")}}}),
              cfg({{QStringLiteral("tab_rects"), tabs}}));
    }

    // Header sections and item-view rows.
    {
        QTableView view;
        view.setModel(rowsModel(&view, false, false));
        view.horizontalHeader()->setSortIndicatorShown(false);
        adopt(&view);
        view.ensurePolished();
        QHeaderView *h = view.horizontalHeader();
        QHeaderView *v = view.verticalHeader();
        r.add(QStringLiteral("header.horizontal"), h, {},
              cfg({{QStringLiteral("section_size_hint"), QJsonArray{h->sectionSizeHint(0), h->sectionSizeHint(1)}},
                   {QStringLiteral("default_section_size"), h->defaultSectionSize()},
                   {QStringLiteral("minimum_section_size"), h->minimumSectionSize()}}));
        r.add(QStringLiteral("header.vertical"), v, {},
              cfg({{QStringLiteral("default_section_size"), v->defaultSectionSize()},
                   {QStringLiteral("minimum_section_size"), v->minimumSectionSize()},
                   {QStringLiteral("section_size_hint"), QJsonArray{v->sectionSizeHint(0), v->sectionSizeHint(1)}}}));
        r.add(QStringLiteral("table_view"), &view, {}, cfg({{QStringLiteral("size_hint_for_row"), static_cast<QAbstractItemView &>(view).sizeHintForRow(0)}}));
    }
    {
        QTableView view;
        view.setModel(rowsModel(&view, false, false));
        view.setSortingEnabled(true);
        adopt(&view);
        view.ensurePolished();
        QHeaderView *h = view.horizontalHeader();
        r.add(QStringLiteral("header.horizontal.sort_indicator"), h, cfg({{QStringLiteral("sort_indicator"), true}}),
              cfg({{QStringLiteral("section_size_hint"), QJsonArray{h->sectionSizeHint(0), h->sectionSizeHint(1)}}}));
    }
    for (int variant = 0; variant < 3; ++variant) {
        const bool icon = variant == 1;
        const bool check = variant == 2;
        QListView view;
        view.setModel(rowsModel(&view, icon, check));
        adopt(&view);
        view.ensurePolished();
        const QString id = variant == 0 ? QStringLiteral("list_view") : (icon ? QStringLiteral("list_view.icon") : QStringLiteral("list_view.check"));
        r.add(id, &view, cfg({{QStringLiteral("icon"), icon}, {QStringLiteral("check"), check}}),
              cfg({{QStringLiteral("size_hint_for_row"), static_cast<QAbstractItemView &>(view).sizeHintForRow(0)}}));
    }
    for (int variant = 0; variant < 2; ++variant) {
        QTreeView view;
        view.setModel(rowsModel(&view, variant == 1, false));
        adopt(&view);
        view.ensurePolished();
        r.add(variant == 0 ? QStringLiteral("tree_view") : QStringLiteral("tree_view.icon"), &view, cfg({{QStringLiteral("icon"), variant == 1}}),
              cfg({{QStringLiteral("size_hint_for_row"), static_cast<QAbstractItemView &>(view).sizeHintForRow(0)},
                   {QStringLiteral("indentation"), view.indentation()},
                   {QStringLiteral("header_height"), view.header()->sizeHint().height()}}));
    }
    return r.out;
}

QJsonObject enumValues(const QMetaEnum &e, const std::function<int(int)> &query)
{
    QJsonObject o;
    for (int i = 0; i < e.keyCount(); ++i) {
        const QByteArray key = e.key(i);
        if (key.endsWith("CustomBase") || key.contains("_Last"))
            continue;
        o.insert(QString::fromLatin1(key), query(e.value(i)));
    }
    return o;
}

// sizeFromContents over a small lattice of content sizes. Each option is
// initialised from a real widget (initFrom: font, palette, direction, state)
// and then given the fields listed in "option".
QJsonArray contentsLattice(QStyle *style)
{
    const QList<QSize> sizes = {QSize(0, 0), QSize(20, 16), QSize(60, 18), QSize(100, 24), QSize(100, 32)};
    QJsonArray out;
    auto add = [&](const QString &id, QStyle::ContentsType ct, const QStyleOption &opt, QWidget *w, const QJsonObject &desc) {
        for (const QSize &s : sizes) {
            QJsonObject o;
            o.insert(QStringLiteral("id"), id);
            o.insert(QStringLiteral("option"), desc);
            o.insert(QStringLiteral("contents"), sz(s));
            o.insert(QStringLiteral("out"), sz(style->sizeFromContents(ct, &opt, s, w)));
            out.append(o);
        }
    };

    {
        QPushButton w(QStringLiteral("OK"));
        adopt(&w);
        w.ensurePolished();
        QStyleOptionButton opt;
        opt.initFrom(&w);
        opt.text = QStringLiteral("OK");
        add(QStringLiteral("CT_PushButton"), QStyle::CT_PushButton, opt, &w, cfg({{QStringLiteral("text"), QStringLiteral("OK")}}));
        opt.features |= QStyleOptionButton::Flat;
        add(QStringLiteral("CT_PushButton.flat"), QStyle::CT_PushButton, opt, &w, cfg({{QStringLiteral("text"), QStringLiteral("OK")}, {QStringLiteral("flat"), true}}));
    }
    for (bool autoRaise : {true, false}) {
        QToolButton w;
        w.setAutoRaise(autoRaise);
        adopt(&w);
        w.ensurePolished();
        QStyleOptionToolButton opt;
        opt.initFrom(&w);
        opt.subControls = QStyle::SC_ToolButton;
        opt.toolButtonStyle = Qt::ToolButtonIconOnly;
        opt.iconSize = QSize(16, 16);
        opt.icon = testIcon();
        if (autoRaise)
            opt.state |= QStyle::State_AutoRaise;
        add(autoRaise ? QStringLiteral("CT_ToolButton.auto_raise") : QStringLiteral("CT_ToolButton.framed"), QStyle::CT_ToolButton, opt, &w,
            cfg({{QStringLiteral("auto_raise"), autoRaise}, {QStringLiteral("icon_size"), 16}}));
    }
    {
        QLineEdit w;
        adopt(&w);
        w.ensurePolished();
        QStyleOptionFrame opt;
        opt.initFrom(&w);
        opt.lineWidth = style->pixelMetric(QStyle::PM_DefaultFrameWidth, &opt, &w);
        opt.midLineWidth = 0;
        opt.state |= QStyle::State_Sunken;
        add(QStringLiteral("CT_LineEdit"), QStyle::CT_LineEdit, opt, &w, cfg({{QStringLiteral("line_width"), opt.lineWidth}}));
    }
    for (bool editable : {false, true}) {
        QComboBox w;
        w.setEditable(editable);
        adopt(&w);
        w.ensurePolished();
        QStyleOptionComboBox opt;
        opt.initFrom(&w);
        opt.editable = editable;
        opt.frame = true;
        add(editable ? QStringLiteral("CT_ComboBox.editable") : QStringLiteral("CT_ComboBox"), QStyle::CT_ComboBox, opt, &w,
            cfg({{QStringLiteral("editable"), editable}, {QStringLiteral("frame"), true}}));
    }
    {
        QSpinBox w;
        adopt(&w);
        w.ensurePolished();
        QStyleOptionSpinBox opt;
        opt.initFrom(&w);
        opt.frame = true;
        opt.buttonSymbols = QAbstractSpinBox::UpDownArrows;
        opt.subControls = QStyle::SC_SpinBoxFrame | QStyle::SC_SpinBoxEditField | QStyle::SC_SpinBoxUp | QStyle::SC_SpinBoxDown;
        add(QStringLiteral("CT_SpinBox"), QStyle::CT_SpinBox, opt, &w, cfg({{QStringLiteral("frame"), true}}));
    }
    {
        QCheckBox w(QStringLiteral("X"));
        adopt(&w);
        w.ensurePolished();
        QStyleOptionButton opt;
        opt.initFrom(&w);
        opt.text = QStringLiteral("X");
        add(QStringLiteral("CT_CheckBox"), QStyle::CT_CheckBox, opt, &w, cfg({{QStringLiteral("text"), QStringLiteral("X")}}));
        QRadioButton rw(QStringLiteral("X"));
        adopt(&rw);
        rw.ensurePolished();
        QStyleOptionButton ropt;
        ropt.initFrom(&rw);
        ropt.text = QStringLiteral("X");
        add(QStringLiteral("CT_RadioButton"), QStyle::CT_RadioButton, ropt, &rw, cfg({{QStringLiteral("text"), QStringLiteral("X")}}));
    }
    {
        QSlider w(Qt::Horizontal);
        adopt(&w);
        w.ensurePolished();
        QStyleOptionSlider opt;
        opt.initFrom(&w);
        opt.orientation = Qt::Horizontal;
        opt.tickPosition = QSlider::NoTicks;
        opt.minimum = 0;
        opt.maximum = 100;
        add(QStringLiteral("CT_Slider"), QStyle::CT_Slider, opt, &w, cfg({{QStringLiteral("orientation"), int(Qt::Horizontal)}}));
        opt.tickPosition = QSlider::TicksBothSides;
        add(QStringLiteral("CT_Slider.ticks_both"), QStyle::CT_Slider, opt, &w, cfg({{QStringLiteral("orientation"), int(Qt::Horizontal)}, {QStringLiteral("ticks"), int(QSlider::TicksBothSides)}}));
    }
    {
        QProgressBar w;
        adopt(&w);
        w.ensurePolished();
        QStyleOptionProgressBar opt;
        opt.initFrom(&w);
        opt.minimum = 0;
        opt.maximum = 100;
        opt.progress = 50;
        opt.textVisible = true;
        opt.text = QStringLiteral("50%");
        opt.state |= QStyle::State_Horizontal;
        add(QStringLiteral("CT_ProgressBar"), QStyle::CT_ProgressBar, opt, &w, cfg({{QStringLiteral("text_visible"), true}}));
    }
    {
        QMenu w;
        adopt(&w);
        w.ensurePolished();
        struct Mi {
            QString id;
            QStyleOptionMenuItem::MenuItemType type;
            QString text;
            int maxIconWidth;
            QStyleOptionMenuItem::CheckType check;
        };
        for (const Mi &m : {Mi{QStringLiteral("CT_MenuItem.normal"), QStyleOptionMenuItem::Normal, QStringLiteral("Item"), 0, QStyleOptionMenuItem::NotCheckable},
                            Mi{QStringLiteral("CT_MenuItem.normal.icon_column"), QStyleOptionMenuItem::Normal, QStringLiteral("Item"), 16, QStyleOptionMenuItem::NotCheckable},
                            Mi{QStringLiteral("CT_MenuItem.shortcut"), QStyleOptionMenuItem::Normal, QStringLiteral("Save\tCtrl+S"), 16, QStyleOptionMenuItem::NotCheckable},
                            Mi{QStringLiteral("CT_MenuItem.checkable"), QStyleOptionMenuItem::Normal, QStringLiteral("Item"), 16, QStyleOptionMenuItem::NonExclusive},
                            Mi{QStringLiteral("CT_MenuItem.submenu"), QStyleOptionMenuItem::SubMenu, QStringLiteral("Item"), 16, QStyleOptionMenuItem::NotCheckable},
                            Mi{QStringLiteral("CT_MenuItem.separator"), QStyleOptionMenuItem::Separator, QString(), 16, QStyleOptionMenuItem::NotCheckable},
                            Mi{QStringLiteral("CT_MenuItem.section"), QStyleOptionMenuItem::Separator, QStringLiteral("Section"), 16, QStyleOptionMenuItem::NotCheckable}}) {
            QStyleOptionMenuItem opt;
            opt.initFrom(&w);
            opt.menuItemType = m.type;
            opt.text = m.text;
            opt.maxIconWidth = m.maxIconWidth;
            opt.checkType = m.check;
            opt.menuHasCheckableItems = m.check != QStyleOptionMenuItem::NotCheckable;
            opt.reservedShortcutWidth = 0;
            add(m.id, QStyle::CT_MenuItem, opt, &w,
                cfg({{QStringLiteral("type"), int(m.type)}, {QStringLiteral("text"), m.text}, {QStringLiteral("max_icon_width"), m.maxIconWidth},
                     {QStringLiteral("check_type"), int(m.check)}}));
        }
    }
    {
        QTabBar w;
        adopt(&w);
        w.ensurePolished();
        QStyleOptionTab opt;
        opt.initFrom(&w);
        opt.text = QStringLiteral("Tab");
        opt.shape = QTabBar::RoundedNorth;
        opt.position = QStyleOptionTab::Middle;
        add(QStringLiteral("CT_TabBarTab"), QStyle::CT_TabBarTab, opt, &w, cfg({{QStringLiteral("text"), QStringLiteral("Tab")}, {QStringLiteral("shape"), 0}}));
        opt.documentMode = true;
        add(QStringLiteral("CT_TabBarTab.document_mode"), QStyle::CT_TabBarTab, opt, &w,
            cfg({{QStringLiteral("text"), QStringLiteral("Tab")}, {QStringLiteral("shape"), 0}, {QStringLiteral("document_mode"), true}}));
    }
    {
        QTableView view;
        adopt(&view);
        view.ensurePolished();
        QStyleOptionHeader opt;
        opt.initFrom(view.horizontalHeader());
        opt.text = QStringLiteral("Name");
        opt.orientation = Qt::Horizontal;
        opt.section = 0;
        add(QStringLiteral("CT_HeaderSection"), QStyle::CT_HeaderSection, opt, view.horizontalHeader(), cfg({{QStringLiteral("text"), QStringLiteral("Name")}}));
    }
    {
        QListView view;
        adopt(&view);
        view.ensurePolished();
        QStyleOptionViewItem opt;
        opt.initFrom(&view);
        opt.features = QStyleOptionViewItem::HasDisplay;
        opt.text = QStringLiteral("Row");
        opt.decorationSize = QSize(16, 16);
        opt.displayAlignment = Qt::AlignLeft | Qt::AlignVCenter;
        add(QStringLiteral("CT_ItemViewItem"), QStyle::CT_ItemViewItem, opt, &view, cfg({{QStringLiteral("text"), QStringLiteral("Row")}}));
    }
    return out;
}

QJsonObject subRects(QStyle *style)
{
    QJsonObject out;

    auto ccRects = [&](QStyle::ComplexControl cc, const QStyleOptionComplex &opt, QWidget *w,
                       const QList<QPair<QStyle::SubControl, QString>> &scs) {
        QJsonObject o;
        for (const auto &sc : scs)
            o.insert(sc.second, rc(style->subControlRect(cc, &opt, sc.first, w)));
        return o;
    };

    {
        QComboBox w;
        adopt(&w);
        w.ensurePolished();
        QStyleOptionComboBox opt;
        opt.initFrom(&w);
        opt.rect = QRect(0, 0, 150, 32);
        opt.frame = true;
        opt.subControls = QStyle::SC_All;
        QJsonObject o = ccRects(QStyle::CC_ComboBox, opt, &w,
                                {{QStyle::SC_ComboBoxFrame, QStringLiteral("frame")},
                                 {QStyle::SC_ComboBoxEditField, QStringLiteral("edit_field")},
                                 {QStyle::SC_ComboBoxArrow, QStringLiteral("arrow")},
                                 {QStyle::SC_ComboBoxListBoxPopup, QStringLiteral("popup")}});
        o.insert(QStringLiteral("rect"), rc(opt.rect));
        out.insert(QStringLiteral("combo_box"), o);
        opt.editable = true;
        QJsonObject e = ccRects(QStyle::CC_ComboBox, opt, &w,
                                {{QStyle::SC_ComboBoxEditField, QStringLiteral("edit_field")}, {QStyle::SC_ComboBoxArrow, QStringLiteral("arrow")}});
        e.insert(QStringLiteral("rect"), rc(opt.rect));
        out.insert(QStringLiteral("combo_box.editable"), e);
    }
    {
        QSpinBox w;
        adopt(&w);
        w.ensurePolished();
        QStyleOptionSpinBox opt;
        opt.initFrom(&w);
        opt.rect = QRect(0, 0, 100, 32);
        opt.frame = true;
        opt.buttonSymbols = QAbstractSpinBox::UpDownArrows;
        opt.subControls = QStyle::SC_All;
        QJsonObject o = ccRects(QStyle::CC_SpinBox, opt, &w,
                                {{QStyle::SC_SpinBoxFrame, QStringLiteral("frame")},
                                 {QStyle::SC_SpinBoxEditField, QStringLiteral("edit_field")},
                                 {QStyle::SC_SpinBoxUp, QStringLiteral("up")},
                                 {QStyle::SC_SpinBoxDown, QStringLiteral("down")}});
        o.insert(QStringLiteral("rect"), rc(opt.rect));
        out.insert(QStringLiteral("spin_box"), o);
    }
    for (int value : {0, 50, 100}) {
        QScrollBar w(Qt::Vertical);
        adopt(&w);
        w.ensurePolished();
        QStyleOptionSlider opt;
        opt.initFrom(&w);
        opt.orientation = Qt::Vertical;
        opt.rect = QRect(0, 0, style->pixelMetric(QStyle::PM_ScrollBarExtent, &opt, &w), 200);
        opt.minimum = 0;
        opt.maximum = 100;
        opt.pageStep = 20;
        opt.singleStep = 1;
        opt.sliderPosition = value;
        opt.sliderValue = value;
        opt.subControls = QStyle::SC_All;
        QJsonObject o = ccRects(QStyle::CC_ScrollBar, opt, &w,
                                {{QStyle::SC_ScrollBarGroove, QStringLiteral("groove")},
                                 {QStyle::SC_ScrollBarSlider, QStringLiteral("slider")},
                                 {QStyle::SC_ScrollBarAddLine, QStringLiteral("add_line")},
                                 {QStyle::SC_ScrollBarSubLine, QStringLiteral("sub_line")},
                                 {QStyle::SC_ScrollBarAddPage, QStringLiteral("add_page")},
                                 {QStyle::SC_ScrollBarSubPage, QStringLiteral("sub_page")}});
        o.insert(QStringLiteral("rect"), rc(opt.rect));
        o.insert(QStringLiteral("range"), QJsonArray{0, 100});
        o.insert(QStringLiteral("page_step"), 20);
        out.insert(QStringLiteral("scroll_bar.vertical.value_%1").arg(value), o);
    }
    for (QSlider::TickPosition tp : {QSlider::NoTicks, QSlider::TicksBelow, QSlider::TicksBothSides}) {
        QSlider w(Qt::Horizontal);
        adopt(&w);
        w.ensurePolished();
        QStyleOptionSlider opt;
        opt.initFrom(&w);
        opt.orientation = Qt::Horizontal;
        opt.tickPosition = tp;
        opt.minimum = 0;
        opt.maximum = 100;
        opt.sliderPosition = 30;
        opt.sliderValue = 30;
        opt.tickInterval = 10;
        opt.subControls = QStyle::SC_All;
        const QSize hint = style->sizeFromContents(QStyle::CT_Slider, &opt, QSize(200, style->pixelMetric(QStyle::PM_SliderThickness, &opt, &w)), &w);
        opt.rect = QRect(QPoint(0, 0), QSize(200, hint.height()));
        QJsonObject o = ccRects(QStyle::CC_Slider, opt, &w,
                                {{QStyle::SC_SliderGroove, QStringLiteral("groove")}, {QStyle::SC_SliderHandle, QStringLiteral("handle")}});
        o.insert(QStringLiteral("rect"), rc(opt.rect));
        o.insert(QStringLiteral("value"), 30);
        out.insert(QStringLiteral("slider.horizontal.ticks_%1").arg(int(tp)), o);
    }

    auto seRects = [&](const QStyleOption &opt, QWidget *w, const QList<QPair<QStyle::SubElement, QString>> &ses) {
        QJsonObject o;
        for (const auto &se : ses)
            o.insert(se.second, rc(style->subElementRect(se.first, &opt, w)));
        o.insert(QStringLiteral("rect"), rc(opt.rect));
        return o;
    };
    {
        QCheckBox w(QStringLiteral("Check"));
        adopt(&w);
        w.ensurePolished();
        QStyleOptionButton opt;
        opt.initFrom(&w);
        opt.text = QStringLiteral("Check");
        opt.rect = QRect(QPoint(0, 0), w.sizeHint());
        out.insert(QStringLiteral("check_box"),
                   seRects(opt, &w,
                           {{QStyle::SE_CheckBoxIndicator, QStringLiteral("indicator")},
                            {QStyle::SE_CheckBoxContents, QStringLiteral("contents")},
                            {QStyle::SE_CheckBoxFocusRect, QStringLiteral("focus")}}));
        QRadioButton rw(QStringLiteral("Radio"));
        adopt(&rw);
        rw.ensurePolished();
        QStyleOptionButton ropt;
        ropt.initFrom(&rw);
        ropt.text = QStringLiteral("Radio");
        ropt.rect = QRect(QPoint(0, 0), rw.sizeHint());
        out.insert(QStringLiteral("radio_button"),
                   seRects(ropt, &rw,
                           {{QStyle::SE_RadioButtonIndicator, QStringLiteral("indicator")},
                            {QStyle::SE_RadioButtonContents, QStringLiteral("contents")},
                            {QStyle::SE_RadioButtonFocusRect, QStringLiteral("focus")}}));
    }
    {
        QPushButton w(QStringLiteral("OK"));
        adopt(&w);
        w.ensurePolished();
        QStyleOptionButton opt;
        opt.initFrom(&w);
        opt.text = QStringLiteral("OK");
        opt.rect = QRect(QPoint(0, 0), w.sizeHint());
        out.insert(QStringLiteral("push_button"),
                   seRects(opt, &w, {{QStyle::SE_PushButtonContents, QStringLiteral("contents")}, {QStyle::SE_PushButtonFocusRect, QStringLiteral("focus")}}));
    }
    {
        QLineEdit w;
        adopt(&w);
        w.ensurePolished();
        QStyleOptionFrame opt;
        opt.initFrom(&w);
        opt.rect = QRect(QPoint(0, 0), w.sizeHint());
        opt.lineWidth = style->pixelMetric(QStyle::PM_DefaultFrameWidth, &opt, &w);
        out.insert(QStringLiteral("line_edit"), seRects(opt, &w, {{QStyle::SE_LineEditContents, QStringLiteral("contents")}}));
    }
    {
        QProgressBar w;
        adopt(&w);
        w.ensurePolished();
        QStyleOptionProgressBar opt;
        opt.initFrom(&w);
        opt.minimum = 0;
        opt.maximum = 100;
        opt.progress = 50;
        opt.textVisible = true;
        opt.text = QStringLiteral("50%");
        opt.state |= QStyle::State_Horizontal;
        opt.rect = QRect(QPoint(0, 0), QSize(200, w.sizeHint().height()));
        out.insert(QStringLiteral("progress_bar"),
                   seRects(opt, &w,
                           {{QStyle::SE_ProgressBarGroove, QStringLiteral("groove")},
                            {QStyle::SE_ProgressBarContents, QStringLiteral("contents")},
                            {QStyle::SE_ProgressBarLabel, QStringLiteral("label")}}));
    }
    {
        QListView view;
        adopt(&view);
        view.ensurePolished();
        QStyleOptionViewItem opt;
        opt.initFrom(&view);
        opt.features = QStyleOptionViewItem::HasDisplay | QStyleOptionViewItem::HasDecoration | QStyleOptionViewItem::HasCheckIndicator;
        opt.text = QStringLiteral("Row");
        opt.icon = testIcon();
        opt.decorationSize = QSize(16, 16);
        opt.displayAlignment = Qt::AlignLeft | Qt::AlignVCenter;
        opt.decorationAlignment = Qt::AlignCenter;
        opt.decorationPosition = QStyleOptionViewItem::Left;
        opt.rect = QRect(0, 0, 200, 24);
        out.insert(QStringLiteral("item_view_item"),
                   seRects(opt, &view,
                           {{QStyle::SE_ItemViewItemCheckIndicator, QStringLiteral("check")},
                            {QStyle::SE_ItemViewItemDecoration, QStringLiteral("decoration")},
                            {QStyle::SE_ItemViewItemText, QStringLiteral("text")},
                            {QStyle::SE_ItemViewItemFocusRect, QStringLiteral("focus")}}));
    }
    {
        QTabBar w;
        adopt(&w);
        w.ensurePolished();
        QStyleOptionTab opt;
        opt.initFrom(&w);
        opt.text = QStringLiteral("Tab");
        opt.shape = QTabBar::RoundedNorth;
        opt.rect = QRect(0, 0, 80, 30);
        out.insert(QStringLiteral("tab"), seRects(opt, &w, {{QStyle::SE_TabBarTabText, QStringLiteral("text")}}));
    }
    return out;
}


// Pixel metrics that change when the style is told which widget asks. Only
// the values that differ from the widget-less answer are recorded.
QJsonObject widgetPixelMetrics(QStyle *style, const QMetaEnum &pmEnum)
{
    QJsonObject out;
    auto probe = [&](const QString &id, QWidget *w) {
        adopt(w);
        w->ensurePolished();
        QStyleOption opt;
        opt.initFrom(w);
        QJsonObject o;
        for (int i = 0; i < pmEnum.keyCount(); ++i) {
            const QByteArray key = pmEnum.key(i);
            if (key.endsWith("CustomBase"))
                continue;
            const auto pm = QStyle::PixelMetric(pmEnum.value(i));
            const int bare = style->pixelMetric(pm, nullptr, nullptr);
            const int with = style->pixelMetric(pm, &opt, w);
            if (with != bare)
                o.insert(QString::fromLatin1(key), with);
        }
        out.insert(id, o);
    };
    QLineEdit le;
    probe(QStringLiteral("QLineEdit"), &le);
    QComboBox cb;
    probe(QStringLiteral("QComboBox"), &cb);
    QComboBox cbe;
    cbe.setEditable(true);
    probe(QStringLiteral("QComboBox.editable"), &cbe);
    QSpinBox sb;
    probe(QStringLiteral("QSpinBox"), &sb);
    QPushButton pb(QStringLiteral("OK"));
    probe(QStringLiteral("QPushButton"), &pb);
    QToolButton tb;
    probe(QStringLiteral("QToolButton"), &tb);
    QCheckBox chk;
    probe(QStringLiteral("QCheckBox"), &chk);
    QRadioButton rb;
    probe(QStringLiteral("QRadioButton"), &rb);
    QSlider sl(Qt::Horizontal);
    probe(QStringLiteral("QSlider"), &sl);
    QScrollBar sc(Qt::Vertical);
    probe(QStringLiteral("QScrollBar"), &sc);
    QProgressBar pr;
    probe(QStringLiteral("QProgressBar"), &pr);
    QMenu menu;
    probe(QStringLiteral("QMenu"), &menu);
    QMenuBar mb;
    probe(QStringLiteral("QMenuBar"), &mb);
    QTabBar tab;
    probe(QStringLiteral("QTabBar"), &tab);
    QTabWidget tw;
    probe(QStringLiteral("QTabWidget"), &tw);
    QTableView table;
    probe(QStringLiteral("QTableView"), &table);
    probe(QStringLiteral("QHeaderView.horizontal"), table.horizontalHeader());
    QListView lv;
    probe(QStringLiteral("QListView"), &lv);
    QTreeView tv;
    probe(QStringLiteral("QTreeView"), &tv);
    QFrame fr;
    fr.setFrameShape(QFrame::StyledPanel);
    probe(QStringLiteral("QFrame.styled_panel"), &fr);
    QGroupBox gb(QStringLiteral("Group"));
    probe(QStringLiteral("QGroupBox"), &gb);
    QToolBar tbar;
    probe(QStringLiteral("QToolBar"), &tbar);
    QStatusBar stb;
    probe(QStringLiteral("QStatusBar"), &stb);
    return out;
}

} // namespace

int writeSizes(const QString &path)
{
    QStyle *style = QApplication::style();
    QWidget host;
    gHost = &host;
    QJsonObject env;
    env.insert(QStringLiteral("widgets_parented"), QStringLiteral("every measured widget except QMenu has a plain QWidget parent"));
    env.insert(QStringLiteral("style"), style ? style->name() : QStringLiteral("none"));
    env.insert(QStringLiteral("platform"), QGuiApplication::platformName());
    if (QScreen *s = QGuiApplication::primaryScreen()) {
        env.insert(QStringLiteral("logical_dpi"), s->logicalDotsPerInch());
        env.insert(QStringLiteral("physical_dpi"), s->physicalDotsPerInch());
        env.insert(QStringLiteral("device_pixel_ratio"), s->devicePixelRatio());
    }
    env.insert(QStringLiteral("default_font_before_override"), QApplication::font().toString());

    const QMetaEnum pmEnum = QMetaEnum::fromType<QStyle::PixelMetric>();
    const QMetaEnum shEnum = QMetaEnum::fromType<QStyle::StyleHint>();

    QJsonArray fonts, widgets;
    struct Req {
        QString family;
        double pt;
    };
    const QList<Req> reqs = {{QStringLiteral("Noto Sans"), 10}, {QStringLiteral("Noto Sans"), 8}, {QStringLiteral("Noto Sans"), 9},
                             {QStringLiteral("Noto Sans"), 11}, {QStringLiteral("Noto Sans"), 12}, {QStringLiteral("Noto Sans"), 14}};
    QJsonObject pixelMetrics, widgetMetrics, styleHints, lattice, sub;
    for (const Req &rq : reqs) {
        QFont f(rq.family);
        f.setPointSizeF(rq.pt);
        QApplication::setFont(f);
        // No event loop runs here, so an existing widget does not pick up the
        // new application font by itself: give the host the font explicitly.
        host.setFont(f);
        fonts.append(fontRecord(f));
        const QJsonArray ws = widgetsFor();
        for (const QJsonValue &v : ws) {
            QJsonObject o = v.toObject();
            o.insert(QStringLiteral("font"), QStringLiteral("%1 %2").arg(rq.family).arg(rq.pt));
            widgets.append(o);
        }
        if (rq.pt == 10) {
            pixelMetrics = enumValues(pmEnum, [&](int v) { return style->pixelMetric(QStyle::PixelMetric(v), nullptr, nullptr); });
            widgetMetrics = widgetPixelMetrics(style, pmEnum);
            styleHints = enumValues(shEnum, [&](int v) { return style->styleHint(QStyle::StyleHint(v), nullptr, nullptr, nullptr); });
            sub = subRects(style);
            lattice.insert(QStringLiteral("records"), contentsLattice(style));
        }
    }
    // Restore the default font for anything after us.
    QFont def(QStringLiteral("Noto Sans"));
    def.setPointSizeF(10);
    QApplication::setFont(def);

    QJsonArray monoFonts;
    for (double pt : {9.0, 10.0, 11.0}) {
        QFont f(QStringLiteral("Hack"));
        f.setPointSizeF(pt);
        monoFonts.append(fontRecord(f));
    }

    // QFont::fromString on a Qt 5 style string (10 fields): the legacy weight
    // 0..99 in field 4 and the OpenType weight Qt maps it to.
    QJsonArray legacyWeights;
    for (int w = 0; w <= 99; ++w) {
        QFont f;
        f.fromString(QStringLiteral("Noto Sans,10,-1,5,%1,0,0,0,0,0").arg(w));
        QJsonObject o;
        o.insert(QStringLiteral("legacy"), w);
        o.insert(QStringLiteral("weight"), int(f.weight()));
        legacyWeights.append(o);
    }

    // The tooltip label Qt shows (QToolTip::showText), found among the
    // top-level widgets by its class name: its margin and size hint.
    QJsonObject tooltip;
    {
        QFont tipFont(QStringLiteral("Noto Sans"));
        tipFont.setPointSizeF(10.0);
        QApplication::setFont(tipFont);
        QToolTip::showText(QPoint(100, 100), QStringLiteral("Tooltip text"));
        QApplication::processEvents();
        for (QWidget *w : QApplication::topLevelWidgets()) {
            if (QString::fromLatin1(w->metaObject()->className()) != QLatin1String("QTipLabel"))
                continue;
            const auto *label = qobject_cast<QLabel *>(w);
            const QMargins m = w->contentsMargins();
            tooltip.insert(QStringLiteral("class"), QStringLiteral("QTipLabel"));
            tooltip.insert(QStringLiteral("text"), QStringLiteral("Tooltip text"));
            tooltip.insert(QStringLiteral("font"), QStringLiteral("Noto Sans 10"));
            tooltip.insert(QStringLiteral("margin"), label ? label->margin() : -1);
            tooltip.insert(QStringLiteral("contents_margins"), QJsonArray{m.left(), m.top(), m.right(), m.bottom()});
            tooltip.insert(QStringLiteral("size_hint"), QJsonArray{w->sizeHint().width(), w->sizeHint().height()});
            const QFontMetrics fm(w->font());
            const QSize text = fm.size(0, QStringLiteral("Tooltip text"));
            tooltip.insert(QStringLiteral("text_size"), QJsonArray{text.width(), text.height()});
        }
        QToolTip::hideText();
        QApplication::processEvents();
    }

    return golden::writeFile(path,
                             {
                                 {QStringLiteral("meta"), golden::meta(QStringLiteral("sizes"))},
                                 {QStringLiteral("environment"), env},
                                 {QStringLiteral("pixel_metrics"), pixelMetrics, true},
                                 {QStringLiteral("widget_pixel_metrics"), widgetMetrics, true},
                                 {QStringLiteral("style_hints"), styleHints, true},
                                 {QStringLiteral("sub_rects"), sub, true},
                                 {QStringLiteral("size_from_contents"), lattice.value(QStringLiteral("records")), true},
                                 {QStringLiteral("monospace_fonts"), monoFonts, true},
                                 {QStringLiteral("fonts"), fonts, true},
                                 {QStringLiteral("widgets"), widgets, true},
                                 {QStringLiteral("legacy_font_weights"), legacyWeights, true},
                                 {QStringLiteral("tooltip"), tooltip},
                             })
        ? 0
        : 1;
}
