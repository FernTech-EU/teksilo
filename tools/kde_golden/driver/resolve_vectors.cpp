// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech
//
// resolve.json: what KColorScheme and createApplicationPalette resolve for a
// set of input schemes. The installed schemes are read from the image at run
// time and never copied into the repository; the synthetic schemes are
// written below, in this file, and their full text is embedded in the output.

#include "golden_json.h"

#include <KColorScheme>
#include <KConfig>
#include <KConfigGroup>
#include <KSharedConfig>

#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QMetaEnum>
#include <QPalette>
#include <QTemporaryDir>

#include <cstdio>

namespace {

struct Synthetic {
    QString name;
    QString purpose;
    QString text;
};

// Hand-written inputs (MPL-2.0, original values). Each one isolates a rule.
const QList<Synthetic> kSynthetic = {
    {QStringLiteral("SyntheticNoHeader"),
     QStringLiteral("no Colors:Header group and no ColorEffects groups: Header falls back, effects take their defaults"),
     QStringLiteral("[General]\n"
                    "ColorScheme=SyntheticNoHeader\n"
                    "Name=Synthetic No Header\n"
                    "\n"
                    "[KDE]\n"
                    "contrast=7\n"
                    "\n"
                    "[Colors:View]\n"
                    "BackgroundNormal=250,248,240\n"
                    "BackgroundAlternate=240,236,226\n"
                    "ForegroundNormal=40,30,20\n"
                    "ForegroundInactive=120,110,100\n"
                    "ForegroundActive=200,90,10\n"
                    "ForegroundLink=10,90,200\n"
                    "ForegroundVisited=120,40,160\n"
                    "ForegroundNegative=190,20,40\n"
                    "ForegroundNeutral=200,140,0\n"
                    "ForegroundPositive=20,140,60\n"
                    "DecorationFocus=200,90,10\n"
                    "DecorationHover=230,130,50\n"
                    "\n"
                    "[Colors:Window]\n"
                    "BackgroundNormal=230,226,216\n"
                    "BackgroundAlternate=220,214,202\n"
                    "ForegroundNormal=30,25,20\n"
                    "ForegroundInactive=110,100,90\n"
                    "ForegroundActive=200,90,10\n"
                    "ForegroundLink=10,90,200\n"
                    "ForegroundVisited=120,40,160\n"
                    "ForegroundNegative=190,20,40\n"
                    "ForegroundNeutral=200,140,0\n"
                    "ForegroundPositive=20,140,60\n"
                    "DecorationFocus=200,90,10\n"
                    "DecorationHover=230,130,50\n"
                    "\n"
                    "[Colors:Button]\n"
                    "BackgroundNormal=236,232,224\n"
                    "BackgroundAlternate=210,204,190\n"
                    "ForegroundNormal=30,25,20\n"
                    "ForegroundInactive=110,100,90\n"
                    "ForegroundActive=200,90,10\n"
                    "ForegroundLink=10,90,200\n"
                    "ForegroundVisited=120,40,160\n"
                    "ForegroundNegative=190,20,40\n"
                    "ForegroundNeutral=200,140,0\n"
                    "ForegroundPositive=20,140,60\n"
                    "DecorationFocus=200,90,10\n"
                    "DecorationHover=230,130,50\n"
                    "\n"
                    "[Colors:Selection]\n"
                    "BackgroundNormal=200,90,10\n"
                    "BackgroundAlternate=170,80,20\n"
                    "ForegroundNormal=255,255,255\n"
                    "ForegroundInactive=240,220,200\n"
                    "ForegroundActive=255,240,200\n"
                    "ForegroundLink=255,230,160\n"
                    "ForegroundVisited=240,200,240\n"
                    "ForegroundNegative=255,180,180\n"
                    "ForegroundNeutral=255,230,120\n"
                    "ForegroundPositive=180,255,190\n"
                    "DecorationFocus=200,90,10\n"
                    "DecorationHover=230,130,50\n"
                    "\n"
                    "[Colors:Tooltip]\n"
                    "BackgroundNormal=255,250,220\n"
                    "BackgroundAlternate=245,238,200\n"
                    "ForegroundNormal=20,20,20\n"
                    "ForegroundInactive=100,100,90\n"
                    "ForegroundActive=200,90,10\n"
                    "ForegroundLink=10,90,200\n"
                    "ForegroundVisited=120,40,160\n"
                    "ForegroundNegative=190,20,40\n"
                    "ForegroundNeutral=200,140,0\n"
                    "ForegroundPositive=20,140,60\n"
                    "DecorationFocus=200,90,10\n"
                    "DecorationHover=230,130,50\n"
                    "\n"
                    "[Colors:Complementary]\n"
                    "BackgroundNormal=40,36,30\n"
                    "BackgroundAlternate=30,27,22\n"
                    "ForegroundNormal=240,235,225\n"
                    "ForegroundInactive=160,150,140\n"
                    "ForegroundActive=255,160,60\n"
                    "ForegroundLink=100,170,255\n"
                    "ForegroundVisited=200,140,230\n"
                    "ForegroundNegative=255,90,100\n"
                    "ForegroundNeutral=255,200,60\n"
                    "ForegroundPositive=90,220,130\n"
                    "DecorationFocus=255,160,60\n"
                    "DecorationHover=255,190,110\n")},

    {QStringLiteral("SyntheticInactiveEffects"),
     QStringLiteral("dark scheme, Header without an Inactive subgroup, Inactive effects enabled with the selection change; every effect kind with "
                    "non-zero amounts in both states"),
     QStringLiteral("[General]\n"
                    "ColorScheme=SyntheticInactiveEffects\n"
                    "Name=Synthetic Inactive Effects\n"
                    "\n"
                    "[KDE]\n"
                    "contrast=2\n"
                    "frameContrast=0.35\n"
                    "\n"
                    "[ColorEffects:Inactive]\n"
                    "Enable=true\n"
                    "ChangeSelectionColor=true\n"
                    "IntensityEffect=1\n"
                    "IntensityAmount=0.25\n"
                    "ColorEffect=1\n"
                    "ColorAmount=0.3\n"
                    "Color=112,111,110\n"
                    "ContrastEffect=1\n"
                    "ContrastAmount=0.4\n"
                    "\n"
                    "[ColorEffects:Disabled]\n"
                    "IntensityEffect=3\n"
                    "IntensityAmount=0.2\n"
                    "ColorEffect=3\n"
                    "ColorAmount=0.5\n"
                    "Color=56,56,56\n"
                    "ContrastEffect=2\n"
                    "ContrastAmount=0.6\n"
                    "\n"
                    "[Colors:View]\n"
                    "BackgroundNormal=24,26,30\n"
                    "BackgroundAlternate=32,35,40\n"
                    "ForegroundNormal=220,224,230\n"
                    "ForegroundInactive=140,146,154\n"
                    "ForegroundActive=90,200,250\n"
                    "ForegroundLink=80,160,255\n"
                    "ForegroundVisited=170,120,230\n"
                    "ForegroundNegative=240,80,90\n"
                    "ForegroundNeutral=240,170,40\n"
                    "ForegroundPositive=60,200,110\n"
                    "DecorationFocus=90,200,250\n"
                    "DecorationHover=90,200,250\n"
                    "\n"
                    "[Colors:Window]\n"
                    "BackgroundNormal=36,39,44\n"
                    "BackgroundAlternate=30,33,38\n"
                    "ForegroundNormal=220,224,230\n"
                    "ForegroundInactive=140,146,154\n"
                    "ForegroundActive=90,200,250\n"
                    "ForegroundLink=80,160,255\n"
                    "ForegroundVisited=170,120,230\n"
                    "ForegroundNegative=240,80,90\n"
                    "ForegroundNeutral=240,170,40\n"
                    "ForegroundPositive=60,200,110\n"
                    "DecorationFocus=90,200,250\n"
                    "DecorationHover=90,200,250\n"
                    "\n"
                    "[Colors:Button]\n"
                    "BackgroundNormal=44,48,54\n"
                    "BackgroundAlternate=38,42,48\n"
                    "ForegroundNormal=220,224,230\n"
                    "ForegroundInactive=140,146,154\n"
                    "ForegroundActive=90,200,250\n"
                    "ForegroundLink=80,160,255\n"
                    "ForegroundVisited=170,120,230\n"
                    "ForegroundNegative=240,80,90\n"
                    "ForegroundNeutral=240,170,40\n"
                    "ForegroundPositive=60,200,110\n"
                    "DecorationFocus=90,200,250\n"
                    "DecorationHover=90,200,250\n"
                    "\n"
                    "[Colors:Selection]\n"
                    "BackgroundNormal=90,200,250\n"
                    "BackgroundAlternate=60,150,200\n"
                    "ForegroundNormal=10,10,10\n"
                    "ForegroundInactive=40,60,70\n"
                    "ForegroundActive=0,0,0\n"
                    "ForegroundLink=0,40,120\n"
                    "ForegroundVisited=70,20,110\n"
                    "ForegroundNegative=120,0,10\n"
                    "ForegroundNeutral=110,70,0\n"
                    "ForegroundPositive=0,90,30\n"
                    "DecorationFocus=90,200,250\n"
                    "DecorationHover=90,200,250\n"
                    "\n"
                    "[Colors:Tooltip]\n"
                    "BackgroundNormal=48,52,58\n"
                    "BackgroundAlternate=40,44,50\n"
                    "ForegroundNormal=230,232,236\n"
                    "ForegroundInactive=150,156,164\n"
                    "ForegroundActive=90,200,250\n"
                    "ForegroundLink=80,160,255\n"
                    "ForegroundVisited=170,120,230\n"
                    "ForegroundNegative=240,80,90\n"
                    "ForegroundNeutral=240,170,40\n"
                    "ForegroundPositive=60,200,110\n"
                    "DecorationFocus=90,200,250\n"
                    "DecorationHover=90,200,250\n"
                    "\n"
                    "[Colors:Complementary]\n"
                    "BackgroundNormal=16,18,20\n"
                    "BackgroundAlternate=12,13,15\n"
                    "ForegroundNormal=235,238,242\n"
                    "ForegroundInactive=150,156,164\n"
                    "ForegroundActive=90,200,250\n"
                    "ForegroundLink=80,160,255\n"
                    "ForegroundVisited=170,120,230\n"
                    "ForegroundNegative=240,80,90\n"
                    "ForegroundNeutral=240,170,40\n"
                    "ForegroundPositive=60,200,110\n"
                    "DecorationFocus=90,200,250\n"
                    "DecorationHover=90,200,250\n"
                    "\n"
                    "[Colors:Header]\n"
                    "BackgroundNormal=52,56,62\n"
                    "BackgroundAlternate=36,39,44\n"
                    "ForegroundNormal=225,228,234\n"
                    "ForegroundInactive=140,146,154\n"
                    "ForegroundActive=90,200,250\n"
                    "ForegroundLink=80,160,255\n"
                    "ForegroundVisited=170,120,230\n"
                    "ForegroundNegative=240,80,90\n"
                    "ForegroundNeutral=240,170,40\n"
                    "ForegroundPositive=60,200,110\n"
                    "DecorationFocus=90,200,250\n"
                    "DecorationHover=90,200,250\n")},

    {QStringLiteral("SyntheticInactiveSubgroups"),
     QStringLiteral("Inactive effects enabled without the selection change, plus explicit Inactive subgroups for Header, View and (partial) "
                    "Window; Disabled uses shade, desaturate and fade"),
     QStringLiteral("[General]\n"
                    "ColorScheme=SyntheticInactiveSubgroups\n"
                    "Name=Synthetic Inactive Subgroups\n"
                    "\n"
                    "[KDE]\n"
                    "contrast=5\n"
                    "\n"
                    "[ColorEffects:Inactive]\n"
                    "Enable=true\n"
                    "ChangeSelectionColor=false\n"
                    "IntensityEffect=2\n"
                    "IntensityAmount=0.15\n"
                    "ColorEffect=2\n"
                    "ColorAmount=0.25\n"
                    "Color=200,0,0\n"
                    "ContrastEffect=2\n"
                    "ContrastAmount=0.3\n"
                    "\n"
                    "[ColorEffects:Disabled]\n"
                    "IntensityEffect=1\n"
                    "IntensityAmount=-0.5\n"
                    "ColorEffect=1\n"
                    "ColorAmount=0.8\n"
                    "ContrastEffect=1\n"
                    "ContrastAmount=0.5\n"
                    "\n"
                    "[Colors:View]\n"
                    "BackgroundNormal=255,255,255\n"
                    "BackgroundAlternate=245,247,250\n"
                    "ForegroundNormal=20,22,26\n"
                    "ForegroundInactive=110,116,124\n"
                    "ForegroundActive=0,120,215\n"
                    "ForegroundLink=0,100,200\n"
                    "ForegroundVisited=110,60,180\n"
                    "ForegroundNegative=200,30,40\n"
                    "ForegroundNeutral=190,120,0\n"
                    "ForegroundPositive=20,130,50\n"
                    "DecorationFocus=0,120,215\n"
                    "DecorationHover=60,150,230\n"
                    "\n"
                    "[Colors:View][Inactive]\n"
                    "BackgroundNormal=250,250,250\n"
                    "ForegroundNormal=60,62,66\n"
                    "\n"
                    "[Colors:Window]\n"
                    "BackgroundNormal=240,241,243\n"
                    "BackgroundAlternate=228,230,233\n"
                    "ForegroundNormal=20,22,26\n"
                    "ForegroundInactive=110,116,124\n"
                    "ForegroundActive=0,120,215\n"
                    "ForegroundLink=0,100,200\n"
                    "ForegroundVisited=110,60,180\n"
                    "ForegroundNegative=200,30,40\n"
                    "ForegroundNeutral=190,120,0\n"
                    "ForegroundPositive=20,130,50\n"
                    "DecorationFocus=0,120,215\n"
                    "DecorationHover=60,150,230\n"
                    "\n"
                    "[Colors:Window][Inactive]\n"
                    "BackgroundNormal=232,233,235\n"
                    "\n"
                    "[Colors:Button]\n"
                    "BackgroundNormal=248,249,250\n"
                    "BackgroundAlternate=220,222,226\n"
                    "ForegroundNormal=20,22,26\n"
                    "ForegroundInactive=110,116,124\n"
                    "ForegroundActive=0,120,215\n"
                    "ForegroundLink=0,100,200\n"
                    "ForegroundVisited=110,60,180\n"
                    "ForegroundNegative=200,30,40\n"
                    "ForegroundNeutral=190,120,0\n"
                    "ForegroundPositive=20,130,50\n"
                    "DecorationFocus=0,120,215\n"
                    "DecorationHover=60,150,230\n"
                    "\n"
                    "[Colors:Selection]\n"
                    "BackgroundNormal=0,120,215\n"
                    "BackgroundAlternate=0,100,180\n"
                    "ForegroundNormal=255,255,255\n"
                    "ForegroundInactive=200,220,240\n"
                    "ForegroundActive=255,255,255\n"
                    "ForegroundLink=220,240,255\n"
                    "ForegroundVisited=230,210,255\n"
                    "ForegroundNegative=255,200,200\n"
                    "ForegroundNeutral=255,230,170\n"
                    "ForegroundPositive=200,255,210\n"
                    "DecorationFocus=0,120,215\n"
                    "DecorationHover=60,150,230\n"
                    "\n"
                    "[Colors:Tooltip]\n"
                    "BackgroundNormal=250,250,250\n"
                    "BackgroundAlternate=240,240,240\n"
                    "ForegroundNormal=20,22,26\n"
                    "ForegroundInactive=110,116,124\n"
                    "ForegroundActive=0,120,215\n"
                    "ForegroundLink=0,100,200\n"
                    "ForegroundVisited=110,60,180\n"
                    "ForegroundNegative=200,30,40\n"
                    "ForegroundNeutral=190,120,0\n"
                    "ForegroundPositive=20,130,50\n"
                    "DecorationFocus=0,120,215\n"
                    "DecorationHover=60,150,230\n"
                    "\n"
                    "[Colors:Complementary]\n"
                    "BackgroundNormal=30,32,36\n"
                    "BackgroundAlternate=24,26,30\n"
                    "ForegroundNormal=240,242,245\n"
                    "ForegroundInactive=150,156,164\n"
                    "ForegroundActive=80,170,255\n"
                    "ForegroundLink=80,170,255\n"
                    "ForegroundVisited=180,140,240\n"
                    "ForegroundNegative=255,90,100\n"
                    "ForegroundNeutral=255,190,60\n"
                    "ForegroundPositive=80,210,120\n"
                    "DecorationFocus=80,170,255\n"
                    "DecorationHover=110,190,255\n"
                    "\n"
                    "[Colors:Header]\n"
                    "BackgroundNormal=226,228,232\n"
                    "BackgroundAlternate=240,241,243\n"
                    "ForegroundNormal=20,22,26\n"
                    "ForegroundInactive=110,116,124\n"
                    "ForegroundActive=0,120,215\n"
                    "ForegroundLink=0,100,200\n"
                    "ForegroundVisited=110,60,180\n"
                    "ForegroundNegative=200,30,40\n"
                    "ForegroundNeutral=190,120,0\n"
                    "ForegroundPositive=20,130,50\n"
                    "DecorationFocus=0,120,215\n"
                    "DecorationHover=60,150,230\n"
                    "\n"
                    "[Colors:Header][Inactive]\n"
                    "BackgroundNormal=240,241,243\n"
                    "BackgroundAlternate=226,228,232\n"
                    "ForegroundNormal=90,94,100\n"
                    "ForegroundInactive=130,136,144\n"
                    "ForegroundActive=0,120,215\n"
                    "ForegroundLink=0,100,200\n"
                    "ForegroundVisited=110,60,180\n"
                    "ForegroundNegative=200,30,40\n"
                    "ForegroundNeutral=190,120,0\n"
                    "ForegroundPositive=20,130,50\n"
                    "DecorationFocus=0,120,215\n"
                    "DecorationHover=60,150,230\n")},

    {QStringLiteral("SyntheticAccentKeys"),
     QStringLiteral("a light scheme carrying the [General] accent keys (AccentColor, TintFactor, TitlebarIsAccentColored) and a [WM] group; "
                    "shows which of them KColorScheme itself reads"),
     QStringLiteral("[General]\n"
                    "ColorScheme=SyntheticAccentKeys\n"
                    "Name=Synthetic Accent Keys\n"
                    "AccentColor=233,58,154\n"
                    "TintFactor=0.3\n"
                    "TitlebarIsAccentColored=true\n"
                    "\n"
                    "[KDE]\n"
                    "contrast=4\n"
                    "\n"
                    "[WM]\n"
                    "activeBackground=200,40,120\n"
                    "activeForeground=255,255,255\n"
                    "inactiveBackground=240,230,236\n"
                    "inactiveForeground=120,100,110\n"
                    "\n"
                    "[Colors:View]\n"
                    "BackgroundNormal=252,252,252\n"
                    "ForegroundNormal=35,38,41\n"
                    "DecorationFocus=61,174,233\n"
                    "\n"
                    "[Colors:Window]\n"
                    "BackgroundNormal=239,240,241\n"
                    "ForegroundNormal=35,38,41\n"
                    "\n"
                    "[Colors:Selection]\n"
                    "BackgroundNormal=61,174,233\n"
                    "ForegroundNormal=255,255,255\n")},

    {QStringLiteral("SyntheticEmpty"),
     QStringLiteral("an empty file: every value is KColorScheme's built-in default"),
     QString()},

    {QStringLiteral("SyntheticMalformed"),
     QStringLiteral("value syntax edge cases (hex, alpha, spaces, named colour, too few components, empty value), an out-of-range "
                    "contrast and effect id, a duplicated key and a duplicated group"),
     QStringLiteral("[General]\n"
                    "ColorScheme=SyntheticMalformed\n"
                    "Name=Synthetic Malformed\n"
                    "\n"
                    "[KDE]\n"
                    "contrast=15\n"
                    "\n"
                    "[ColorEffects:Disabled]\n"
                    "IntensityEffect=7\n"
                    "IntensityAmount=0.4\n"
                    "ColorEffect=0\n"
                    "ContrastEffect=1\n"
                    "ContrastAmount=2.5\n"
                    "\n"
                    "[Colors:View]\n"
                    "BackgroundNormal=#ff8800\n"
                    "BackgroundAlternate=10,20,30,128\n"
                    "ForegroundNormal=  12 , 34 , 56 \n"
                    "ForegroundInactive=300,20\n"
                    "ForegroundActive=red\n"
                    "ForegroundLink=\n"
                    "ForegroundVisited=256,-1,1000\n"
                    "ForegroundPositive=0,100,0\n"
                    "ForegroundPositive=0,200,0\n"
                    "\n"
                    "[Colors:Window]\n"
                    "BackgroundNormal=100,100,100\n"
                    "ForegroundNormal=1,2,3\n"
                    "\n"
                    "[Colors:Window]\n"
                    "BackgroundNormal=200,200,200\n"
                    "\n"
                    "[Colors:Selection]\n"
                    "BackgroundNormal=0,0,0\n")},

    {QStringLiteral("SyntheticValueForms"),
     QStringLiteral("more value forms: an empty boolean (Inactive effects Enable=), a signed component, the 3-, 9- and 12-digit hex "
                    "forms, colour names in other cases, transparent, a trailing comma, a hex component and an unknown CSS name"),
     QStringLiteral("[General]\n"
                    "ColorScheme=SyntheticValueForms\n"
                    "Name=Synthetic Value Forms\n"
                    "\n"
                    "[ColorEffects:Inactive]\n"
                    "Enable=\n"
                    "ChangeSelectionColor=off\n"
                    "\n"
                    "[Colors:View]\n"
                    "BackgroundNormal=+250,240,230\n"
                    "BackgroundAlternate=#abc\n"
                    "ForegroundNormal=#123456789\n"
                    "ForegroundInactive=#123456789abc\n"
                    "ForegroundActive=Red\n"
                    "ForegroundLink=grey\n"
                    "ForegroundVisited=rebeccapurple\n"
                    "ForegroundNegative=10,20,30,\n"
                    "ForegroundNeutral=0x10,20,30\n"
                    "ForegroundPositive=transparent\n"
                    "DecorationFocus=#803daee9\n"
                    "\n"
                    "[Colors:Window]\n"
                    "BackgroundNormal=200,190,180\n")},
};

const QList<QPair<KColorScheme::ColorSet, QString>> kSets = {
    {KColorScheme::View, QStringLiteral("View")},
    {KColorScheme::Window, QStringLiteral("Window")},
    {KColorScheme::Button, QStringLiteral("Button")},
    {KColorScheme::Selection, QStringLiteral("Selection")},
    {KColorScheme::Tooltip, QStringLiteral("Tooltip")},
    {KColorScheme::Complementary, QStringLiteral("Complementary")},
    {KColorScheme::Header, QStringLiteral("Header")},
};

const QList<QPair<QPalette::ColorGroup, QString>> kStates = {
    {QPalette::Active, QStringLiteral("Active")},
    {QPalette::Inactive, QStringLiteral("Inactive")},
    {QPalette::Disabled, QStringLiteral("Disabled")},
};

const QList<QPair<KColorScheme::BackgroundRole, QString>> kBg = {
    {KColorScheme::NormalBackground, QStringLiteral("Normal")},
    {KColorScheme::AlternateBackground, QStringLiteral("Alternate")},
    {KColorScheme::ActiveBackground, QStringLiteral("Active")},
    {KColorScheme::LinkBackground, QStringLiteral("Link")},
    {KColorScheme::VisitedBackground, QStringLiteral("Visited")},
    {KColorScheme::NegativeBackground, QStringLiteral("Negative")},
    {KColorScheme::NeutralBackground, QStringLiteral("Neutral")},
    {KColorScheme::PositiveBackground, QStringLiteral("Positive")},
};

const QList<QPair<KColorScheme::ForegroundRole, QString>> kFg = {
    {KColorScheme::NormalText, QStringLiteral("Normal")},
    {KColorScheme::InactiveText, QStringLiteral("Inactive")},
    {KColorScheme::ActiveText, QStringLiteral("Active")},
    {KColorScheme::LinkText, QStringLiteral("Link")},
    {KColorScheme::VisitedText, QStringLiteral("Visited")},
    {KColorScheme::NegativeText, QStringLiteral("Negative")},
    {KColorScheme::NeutralText, QStringLiteral("Neutral")},
    {KColorScheme::PositiveText, QStringLiteral("Positive")},
};

const QList<QPair<KColorScheme::DecorationRole, QString>> kDeco = {
    {KColorScheme::FocusColor, QStringLiteral("Focus")},
    {KColorScheme::HoverColor, QStringLiteral("Hover")},
};

const QList<QPair<KColorScheme::ShadeRole, QString>> kShade = {
    {KColorScheme::LightShade, QStringLiteral("Light")},
    {KColorScheme::MidlightShade, QStringLiteral("Midlight")},
    {KColorScheme::MidShade, QStringLiteral("Mid")},
    {KColorScheme::DarkShade, QStringLiteral("Dark")},
    {KColorScheme::ShadowShade, QStringLiteral("Shadow")},
};

void collectGroup(const KConfigGroup &g, const QString &path, QJsonObject &out)
{
    const QMap<QString, QString> map = g.entryMap();
    QJsonObject entries;
    for (auto it = map.cbegin(); it != map.cend(); ++it)
        entries.insert(it.key(), it.value());
    if (!entries.isEmpty())
        out.insert(path, entries);
    const QStringList subs = g.groupList();
    for (const QString &sub : subs)
        collectGroup(g.group(sub), path + QStringLiteral("][") + sub, out);
}

QJsonObject parsedInput(const KSharedConfigPtr &config)
{
    QJsonObject out;
    const QStringList tops = config->groupList();
    for (const QString &top : tops)
        collectGroup(config->group(top), top, out);
    return out;
}

QJsonObject resolveScheme(const KSharedConfigPtr &config)
{
    QJsonObject sets;
    for (const auto &set : kSets) {
        QJsonObject states;
        for (const auto &state : kStates) {
            const KColorScheme cs(state.first, set.first, config);
            QJsonObject bg, fg, deco, shade;
            for (const auto &r : kBg)
                bg.insert(r.second, golden::color(cs.background(r.first).color()));
            for (const auto &r : kFg)
                fg.insert(r.second, golden::color(cs.foreground(r.first).color()));
            for (const auto &r : kDeco)
                deco.insert(r.second, golden::color(cs.decoration(r.first).color()));
            for (const auto &r : kShade)
                shade.insert(r.second, golden::color(cs.shade(r.first)));
            QJsonObject st;
            st.insert(QStringLiteral("background"), bg);
            st.insert(QStringLiteral("foreground"), fg);
            st.insert(QStringLiteral("decoration"), deco);
            st.insert(QStringLiteral("shade"), shade);
            states.insert(state.second, st);
        }
        sets.insert(set.second, states);
    }
    return sets;
}

QJsonObject paletteOf(const KSharedConfigPtr &config)
{
    const QPalette pal = KColorScheme::createApplicationPalette(config);
    const QMetaEnum roles = QMetaEnum::fromType<QPalette::ColorRole>();
    QJsonObject out;
    for (const auto &state : kStates) {
        QJsonObject g;
        for (int i = 0; i < roles.keyCount(); ++i) {
            const int v = roles.value(i);
            if (v < 0 || v >= QPalette::NColorRoles || v == QPalette::NoRole)
                continue;
            g.insert(QString::fromLatin1(roles.key(i)), golden::color(pal.color(state.first, QPalette::ColorRole(v))));
        }
        out.insert(state.second, g);
    }
    return out;
}

QJsonObject schemeRecord(const QString &name, const QString &file, const KSharedConfigPtr &config)
{
    QJsonObject o;
    o.insert(QStringLiteral("name"), name);
    o.insert(QStringLiteral("input"), parsedInput(config));
    o.insert(QStringLiteral("contrastF"), KColorScheme::contrastF(config));
    o.insert(QStringLiteral("frameContrast"), KColorScheme::frameContrast(config));
    QJsonObject supported;
    for (const auto &set : kSets)
        supported.insert(set.second, KColorScheme::isColorSetSupported(config, set.first));
    o.insert(QStringLiteral("set_supported"), supported);
    o.insert(QStringLiteral("sets"), resolveScheme(config));
    o.insert(QStringLiteral("palette"), paletteOf(config));
    QFile f(file);
    if (f.open(QIODevice::ReadOnly))
        o.insert(QStringLiteral("sha256"), QString::fromLatin1(QCryptographicHash::hash(f.readAll(), QCryptographicHash::Sha256).toHex()));
    return o;
}

} // namespace

int writeResolve(const QString &path)
{
    QJsonArray schemes;

    // Installed schemes, in a fixed order; a missing one is reported, not fatal.
    const QStringList installed = {QStringLiteral("BreezeLight"),
                                   QStringLiteral("BreezeDark"),
                                   QStringLiteral("BreezeClassic"),
                                   QStringLiteral("Oxygen"),
                                   QStringLiteral("OxygenCold"),
                                   QStringLiteral("OxygenDark")};
    QJsonArray missing;
    for (const QString &name : installed) {
        const QString file = QStringLiteral("/usr/share/color-schemes/%1.colors").arg(name);
        if (!QFile::exists(file)) {
            missing.append(name);
            continue;
        }
        KSharedConfigPtr config = KSharedConfig::openConfig(file, KConfig::SimpleConfig);
        QJsonObject rec = schemeRecord(name, file, config);
        rec.insert(QStringLiteral("origin"), QStringLiteral("installed"));
        rec.insert(QStringLiteral("source"), file);
        schemes.append(rec);
    }

    QTemporaryDir dir;
    if (!dir.isValid()) {
        std::fprintf(stderr, "no temporary directory\n");
        return 1;
    }
    for (const Synthetic &s : kSynthetic) {
        const QString file = dir.filePath(s.name + QStringLiteral(".colors"));
        QFile f(file);
        if (!f.open(QIODevice::WriteOnly)) {
            std::fprintf(stderr, "cannot write %s\n", qPrintable(file));
            return 1;
        }
        f.write(s.text.toUtf8());
        f.close();
        KSharedConfigPtr config = KSharedConfig::openConfig(file, KConfig::SimpleConfig);
        QJsonObject rec = schemeRecord(s.name, file, config);
        rec.insert(QStringLiteral("origin"), QStringLiteral("synthetic"));
        rec.insert(QStringLiteral("purpose"), s.purpose);
        rec.insert(QStringLiteral("source_text"), s.text);
        schemes.append(rec);
    }

    return golden::writeFile(path,
                             {
                                 {QStringLiteral("meta"), golden::meta(QStringLiteral("resolve"))},
                                 {QStringLiteral("missing_installed"), missing},
                                 {QStringLiteral("schemes"), schemes, true},
                             })
        ? 0
        : 1;
}
