// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech
//
// math.json: KColorUtils and the static KColorScheme::shade over a fixed
// lattice of colours and amounts, plus the Qt colour behaviour the KDE
// colour model rests on (QColor::lighter / darker, the sRGB colour-space
// transforms, out-of-range float colours, colour text parsing). Every input
// colour is an 8-bit value, so its 16-bit form is exactly value * 257.

#include "golden_json.h"

#include <KColorScheme>
#include <KColorUtils>

#include <QColorSpace>
#include <QColorTransform>
#include <QPainter>

namespace {

// The lattice. Order is part of the format: tests may index by position.
const QStringList kColors = {
    // black, white, greys (including the values around 0x7f/0x80 and the ends)
    QStringLiteral("#000000ff"), QStringLiteral("#ffffffff"), QStringLiteral("#010101ff"), QStringLiteral("#020202ff"),
    QStringLiteral("#040404ff"), QStringLiteral("#080808ff"), QStringLiteral("#101010ff"), QStringLiteral("#202020ff"),
    QStringLiteral("#404040ff"), QStringLiteral("#7f7f7fff"), QStringLiteral("#808080ff"), QStringLiteral("#818181ff"),
    QStringLiteral("#bfbfbfff"), QStringLiteral("#c0c0c0ff"), QStringLiteral("#fdfdfdff"), QStringLiteral("#fefefeff"),
    // saturated primaries and secondaries, half-intensity and pastel variants
    QStringLiteral("#ff0000ff"), QStringLiteral("#00ff00ff"), QStringLiteral("#0000ffff"), QStringLiteral("#ffff00ff"),
    QStringLiteral("#00ffffff"), QStringLiteral("#ff00ffff"), QStringLiteral("#800000ff"), QStringLiteral("#008000ff"),
    QStringLiteral("#000080ff"), QStringLiteral("#ff8080ff"), QStringLiteral("#80ff80ff"), QStringLiteral("#8080ffff"),
    // near-threshold: almost grey, almost black, almost white
    QStringLiteral("#030201ff"), QStringLiteral("#fffefdff"), QStringLiteral("#7f8081ff"), QStringLiteral("#010000ff"),
    QStringLiteral("#000001ff"), QStringLiteral("#fefffeff"),
    // colours found in the Breeze schemes
    QStringLiteral("#3daee9ff"), QStringLiteral("#232629ff"), QStringLiteral("#eff0f1ff"), QStringLiteral("#fcfcfcff"),
    QStringLiteral("#31363bff"), QStringLiteral("#1b1e20ff"), QStringLiteral("#da4453ff"), QStringLiteral("#27ae60ff"),
    QStringLiteral("#f67400ff"), QStringLiteral("#2980b9ff"), QStringLiteral("#7f8c8dff"), QStringLiteral("#9b59b6ff"),
    QStringLiteral("#1d99f3ff"), QStringLiteral("#dee0e2ff"), QStringLiteral("#292c30ff"),
    // the Plasma accent swatches not already listed
    QStringLiteral("#e93a9aff"), QStringLiteral("#e93d58ff"), QStringLiteral("#e9643aff"), QStringLiteral("#e8cb2dff"),
    QStringLiteral("#3dd425ff"), QStringLiteral("#00d3b8ff"), QStringLiteral("#b875dcff"), QStringLiteral("#926ee4ff"),
    QStringLiteral("#686b6fff"),
    // translucent
    QStringLiteral("#3daee980"), QStringLiteral("#ffffff00"), QStringLiteral("#00000080"), QStringLiteral("#ff000033"),
};

// The smaller set used for two-colour operations (tint, mix, overlay).
const QStringList kPairColors = {
    QStringLiteral("#000000ff"), QStringLiteral("#ffffffff"), QStringLiteral("#7f7f7fff"), QStringLiteral("#808080ff"),
    QStringLiteral("#ff0000ff"), QStringLiteral("#00ff00ff"), QStringLiteral("#0000ffff"), QStringLiteral("#ffff00ff"),
    QStringLiteral("#030201ff"), QStringLiteral("#3daee9ff"), QStringLiteral("#232629ff"), QStringLiteral("#eff0f1ff"),
    QStringLiteral("#fcfcfcff"), QStringLiteral("#da4453ff"), QStringLiteral("#e8cb2dff"), QStringLiteral("#3daee980"),
    QStringLiteral("#ffffff00"),
};

const QList<double> kLightenAmounts = {-0.5, 0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0, 1.5};
const QList<double> kGains = {0.0, 0.5, 1.0, 2.0};
const QList<double> kShadeLuma = {-1.5, -1.0, -0.5, -0.1, 0.0, 0.1, 0.5, 1.0, 1.5};
const QList<double> kShadeChroma = {-1.0, 0.0, 0.5, 1.0};
const QList<double> kTintAmounts = {-0.5, 0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.7, 1.0, 1.5};
const QList<double> kMixBias = {-0.5, 0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0, 1.5};
const QList<double> kHcyHue = {-0.25, 0.0, 0.1, 1.0 / 6.0, 0.5, 0.9, 1.0, 1.25};
const QList<double> kHcyChroma = {-0.5, 0.0, 0.25, 0.5, 1.0, 1.5};
const QList<double> kHcyLuma = {-0.1, 0.0, 0.1, 0.5, 0.9, 1.0, 1.1};
const QList<double> kHcyAlpha = {1.0, 0.5};
const QList<double> kSchemeContrast = {-1.0, 0.0, 0.4, 0.7, 1.0, 1.5};
const QList<double> kSchemeChroma = {0.0, 0.5};
// QColor::lighter / darker factors: the non-positive and below-100 branches,
// the identity, and the factors the Breeze style uses (102, 110, 120).
const QList<int> kQtFactors = {-10, 0, 1, 50, 99, 100, 101, 102, 110, 120, 150, 200, 300, 1000};

const QList<QPair<KColorScheme::ShadeRole, QString>> kShadeRoles = {
    {KColorScheme::LightShade, QStringLiteral("Light")},
    {KColorScheme::MidlightShade, QStringLiteral("Midlight")},
    {KColorScheme::MidShade, QStringLiteral("Mid")},
    {KColorScheme::DarkShade, QStringLiteral("Dark")},
    {KColorScheme::ShadowShade, QStringLiteral("Shadow")},
};

const QList<QPair<QPainter::CompositionMode, QString>> kOverlayModes = {
    {QPainter::CompositionMode_SourceOver, QStringLiteral("SourceOver")},
    {QPainter::CompositionMode_Multiply, QStringLiteral("Multiply")},
    {QPainter::CompositionMode_Screen, QStringLiteral("Screen")},
};

} // namespace

int writeMath(const QString &path)
{
    using namespace golden;
    QJsonArray single;
    for (const QString &h : kColors) {
        const QColor c = parseHex8(h);
        qreal hh = 0, cc = 0, yy = 0, aa = 0;
        KColorUtils::getHcy(c, &hh, &cc, &yy, &aa);
        QJsonObject o;
        o.insert(QStringLiteral("in"), h);
        o.insert(QStringLiteral("luma"), KColorUtils::luma(c));
        o.insert(QStringLiteral("hue"), KColorUtils::hue(c));
        o.insert(QStringLiteral("chroma"), KColorUtils::chroma(c));
        o.insert(QStringLiteral("hcy"), QJsonArray{hh, cc, yy, aa});
        o.insert(QStringLiteral("roundtrip"), color(KColorUtils::hcyColor(hh, cc, yy, aa)));
        single.append(o);
    }

    QJsonArray contrast;
    for (const QString &a : kPairColors)
        for (const QString &b : kColors) {
            QJsonObject o;
            o.insert(QStringLiteral("a"), a);
            o.insert(QStringLiteral("b"), b);
            o.insert(QStringLiteral("out"), KColorUtils::contrastRatio(parseHex8(a), parseHex8(b)));
            contrast.append(o);
        }

    QJsonArray lighten, darken;
    for (const QString &h : kColors)
        for (double amt : kLightenAmounts)
            for (double g : kGains) {
                const QColor c = parseHex8(h);
                QJsonObject o;
                o.insert(QStringLiteral("in"), h);
                o.insert(QStringLiteral("amount"), amt);
                o.insert(QStringLiteral("gain"), g);
                o.insert(QStringLiteral("out"), color(KColorUtils::lighten(c, amt, g)));
                lighten.append(o);
                o.insert(QStringLiteral("out"), color(KColorUtils::darken(c, amt, g)));
                darken.append(o);
            }

    QJsonArray shade;
    for (const QString &h : kColors)
        for (double l : kShadeLuma)
            for (double ch : kShadeChroma) {
                QJsonObject o;
                o.insert(QStringLiteral("in"), h);
                o.insert(QStringLiteral("luma"), l);
                o.insert(QStringLiteral("chroma"), ch);
                o.insert(QStringLiteral("out"), color(KColorUtils::shade(parseHex8(h), l, ch)));
                shade.append(o);
            }

    QJsonArray tint, mix;
    for (const QString &a : kPairColors)
        for (const QString &b : kPairColors) {
            for (double amt : kTintAmounts) {
                QJsonObject o;
                o.insert(QStringLiteral("base"), a);
                o.insert(QStringLiteral("color"), b);
                o.insert(QStringLiteral("amount"), amt);
                o.insert(QStringLiteral("out"), color(KColorUtils::tint(parseHex8(a), parseHex8(b), amt)));
                tint.append(o);
            }
            for (double bias : kMixBias) {
                QJsonObject o;
                o.insert(QStringLiteral("c1"), a);
                o.insert(QStringLiteral("c2"), b);
                o.insert(QStringLiteral("bias"), bias);
                o.insert(QStringLiteral("out"), color(KColorUtils::mix(parseHex8(a), parseHex8(b), bias)));
                mix.append(o);
            }
        }

    QJsonArray overlay;
    for (const QString &a : kPairColors)
        for (const QString &b : kPairColors)
            for (const auto &mode : kOverlayModes) {
                QJsonObject o;
                o.insert(QStringLiteral("base"), a);
                o.insert(QStringLiteral("paint"), b);
                o.insert(QStringLiteral("mode"), mode.second);
                o.insert(QStringLiteral("out"), color(KColorUtils::overlayColors(parseHex8(a), parseHex8(b), mode.first)));
                overlay.append(o);
            }

    QJsonArray hcy;
    for (double h : kHcyHue)
        for (double c : kHcyChroma)
            for (double y : kHcyLuma)
                for (double a : kHcyAlpha) {
                    QJsonObject o;
                    o.insert(QStringLiteral("h"), h);
                    o.insert(QStringLiteral("c"), c);
                    o.insert(QStringLiteral("y"), y);
                    o.insert(QStringLiteral("a"), a);
                    o.insert(QStringLiteral("out"), color(KColorUtils::hcyColor(h, c, y, a)));
                    hcy.append(o);
                }

    QJsonArray schemeShade;
    for (const QString &h : kColors)
        for (const auto &role : kShadeRoles)
            for (double k : kSchemeContrast)
                for (double ch : kSchemeChroma) {
                    QJsonObject o;
                    o.insert(QStringLiteral("in"), h);
                    o.insert(QStringLiteral("role"), role.second);
                    o.insert(QStringLiteral("contrast"), k);
                    o.insert(QStringLiteral("chroma"), ch);
                    o.insert(QStringLiteral("out"), color(KColorScheme::shade(parseHex8(h), role.first, k, ch)));
                    schemeShade.append(o);
                }

    QJsonArray qtLighterDarker;
    for (const QString &h : kColors)
        for (int f : kQtFactors) {
            const QColor c = parseHex8(h);
            QJsonObject o;
            o.insert(QStringLiteral("in"), h);
            o.insert(QStringLiteral("factor"), f);
            o.insert(QStringLiteral("op"), QStringLiteral("lighter"));
            o.insert(QStringLiteral("out"), color(c.lighter(f)));
            qtLighterDarker.append(o);
            o.insert(QStringLiteral("op"), QStringLiteral("darker"));
            o.insert(QStringLiteral("out"), color(c.darker(f)));
            qtLighterDarker.append(o);
        }

    // Qt's own sRGB <-> linear sRGB colour-space transforms (QColorSpace /
    // QColorTransform, public API) on QColor values, and the storage of
    // out-of-range float colours (QColor::fromRgbF then toRgb). These pin the
    // conversions the OKLab accent tint goes through.
    const QColorTransform toLinear = QColorSpace(QColorSpace::SRgb).transformationToColorSpace(QColorSpace::SRgbLinear);
    const QColorTransform fromLinear = QColorSpace(QColorSpace::SRgbLinear).transformationToColorSpace(QColorSpace::SRgb);
    QStringList csInputs = kColors;
    for (int v = 0; v < 256; ++v)
        csInputs.append(QString::asprintf("#%02x%02x%02xff", v, v, v));
    for (int v = 1; v < 256; ++v)
        csInputs.append(QString::asprintf("#%02x0000ff", v));
    QJsonArray qtColorSpace;
    for (const QString &h : csInputs) {
        const QColor c = parseHex8(h);
        QJsonObject o;
        o.insert(QStringLiteral("in"), h);
        o.insert(QStringLiteral("to_linear"), color(toLinear.map(c)));
        o.insert(QStringLiteral("from_linear"), color(fromLinear.map(c)));
        qtColorSpace.append(o);
    }
    const QList<QList<float>> kExtendedInputs = {
        {1.05f, 0.123456789f, -0.01f}, {-0.0000001f, 0.5f, 0.75f}, {1.0000001f, 0.333333343f, 0.666666687f},
        {2.0f, -1.0f, 0.0001f}, {0.999f, 1.001f, 0.2f}, {0.1f, 0.2f, 1.5f},
    };
    QJsonArray qtExtended;
    for (const auto &v : kExtendedInputs) {
        QJsonObject o;
        o.insert(QStringLiteral("in"), QJsonArray{double(v[0]), double(v[1]), double(v[2])});
        const QColor e = QColor::fromRgbF(v[0], v[1], v[2]);
        o.insert(QStringLiteral("stored"), color(e));
        o.insert(QStringLiteral("to_rgb"), color(e.toRgb()));
        qtExtended.append(o);
    }

    // QColor's string parsing, which KConfig uses for every colour value that
    // starts with '#' or contains no comma.
    const QStringList kColorTexts = {
        QStringLiteral("#abc"), QStringLiteral("#3daee9"), QStringLiteral("#803daee9"),
        QStringLiteral("#123456789"), QStringLiteral("#123456789abc"), QStringLiteral("#12"),
        QStringLiteral("#12345"), QStringLiteral("#1234567"), QStringLiteral("#xyzxyz"),
        QStringLiteral("red"), QStringLiteral("Red"), QStringLiteral("RED"),
        QStringLiteral("transparent"), QStringLiteral("rebeccapurple"), QStringLiteral("grey"),
        QStringLiteral("notacolour"), QStringLiteral(" #3daee9"), QStringLiteral(""),
    };
    QJsonArray qtColorText;
    for (const QString &t : kColorTexts) {
        QJsonObject o;
        o.insert(QStringLiteral("in"), t);
        o.insert(QStringLiteral("out"), color(QColor::fromString(t)));
        qtColorText.append(o);
    }

    QJsonArray colorsList, pairList;
    for (const QString &h : kColors)
        colorsList.append(h);
    for (const QString &h : kPairColors)
        pairList.append(h);

    return writeFile(path,
                     {
                         {QStringLiteral("meta"), meta(QStringLiteral("math"))},
                         {QStringLiteral("colors"), colorsList},
                         {QStringLiteral("pair_colors"), pairList},
                         {QStringLiteral("single"), single, true},
                         {QStringLiteral("contrast_ratio"), contrast, true},
                         {QStringLiteral("lighten"), lighten, true},
                         {QStringLiteral("darken"), darken, true},
                         {QStringLiteral("shade"), shade, true},
                         {QStringLiteral("tint"), tint, true},
                         {QStringLiteral("mix"), mix, true},
                         {QStringLiteral("overlay"), overlay, true},
                         {QStringLiteral("hcy_color"), hcy, true},
                         {QStringLiteral("scheme_shade"), schemeShade, true},
                         {QStringLiteral("qt_lighter_darker"), qtLighterDarker, true},
                         {QStringLiteral("qt_colorspace"), qtColorSpace, true},
                         {QStringLiteral("qt_extended"), qtExtended, true},
                         {QStringLiteral("qt_color_text"), qtColorText, true},
                     })
        ? 0
        : 1;
}
