// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

#include "golden_json.h"

#include <QFile>
#include <QJsonDocument>
#include <QProcessEnvironment>
#include <QTextStream>
#include <QtGlobal>

#include <kcolorscheme_version.h>
#include <kguiaddons_version.h>

#include <cstdio>

namespace golden {

QString hex8(const QColor &c)
{
    return QString::asprintf("#%02x%02x%02x%02x", c.red(), c.green(), c.blue(), c.alpha());
}

static QString specName(QColor::Spec s)
{
    switch (s) {
    case QColor::Invalid:
        return QStringLiteral("Invalid");
    case QColor::Rgb:
        return QStringLiteral("Rgb");
    case QColor::Hsv:
        return QStringLiteral("Hsv");
    case QColor::Cmyk:
        return QStringLiteral("Cmyk");
    case QColor::Hsl:
        return QStringLiteral("Hsl");
    case QColor::ExtendedRgb:
        return QStringLiteral("ExtendedRgb");
    }
    return QStringLiteral("Unknown");
}

QJsonObject color(const QColor &c)
{
    QJsonObject o;
    if (!c.isValid()) {
        o.insert(QStringLiteral("valid"), false);
        return o;
    }
    const QRgba64 w = c.rgba64();
    o.insert(QStringLiteral("hex"), hex8(c));
    o.insert(QStringLiteral("u16"), QJsonArray{int(w.red()), int(w.green()), int(w.blue()), int(w.alpha())});
    if (c.spec() != QColor::Rgb) {
        o.insert(QStringLiteral("spec"), specName(c.spec()));
        o.insert(QStringLiteral("f"), QJsonArray{c.redF(), c.greenF(), c.blueF(), c.alphaF()});
    }
    return o;
}

QColor parseHex8(const QString &s)
{
    bool ok = true;
    const int r = s.mid(1, 2).toInt(&ok, 16);
    const int g = s.mid(3, 2).toInt(&ok, 16);
    const int b = s.mid(5, 2).toInt(&ok, 16);
    const int a = s.size() >= 9 ? s.mid(7, 2).toInt(&ok, 16) : 255;
    return QColor::fromRgb(r, g, b, a);
}

static QString readText(const QString &path)
{
    QFile f(path);
    if (!f.open(QIODevice::ReadOnly))
        return QString();
    return QString::fromUtf8(f.readAll());
}

QJsonObject meta(const QString &kind)
{
    const auto env = QProcessEnvironment::systemEnvironment();
    QJsonObject m;
    m.insert(QStringLiteral("kind"), kind);
    m.insert(QStringLiteral("generator"), QStringLiteral("tools/kde_golden/driver (kde_golden)"));
    m.insert(QStringLiteral("image"), env.value(QStringLiteral("KDE_GOLDEN_IMAGE"), QStringLiteral("unknown")));
    m.insert(QStringLiteral("qt_runtime"), QString::fromLatin1(qVersion()));
    m.insert(QStringLiteral("qt_build"), QStringLiteral(QT_VERSION_STR));
    m.insert(QStringLiteral("kcolorscheme_build"), QStringLiteral(KCOLORSCHEME_VERSION_STRING));
    m.insert(QStringLiteral("kguiaddons_build"), QStringLiteral(KGUIADDONS_VERSION_STRING));
    QJsonObject pkgs;
    const QStringList lines = readText(QStringLiteral("/opt/kde_golden/versions.txt")).split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    for (const QString &line : lines) {
        const QStringList kv = line.split(QLatin1Char('\t'));
        if (kv.size() == 2)
            pkgs.insert(kv[0], kv[1]);
    }
    m.insert(QStringLiteral("packages"), pkgs);
    m.insert(QStringLiteral("readme"), QStringLiteral("tools/kde_golden/README.md documents every field"));
    return m;
}

static QByteArray compact(const QJsonValue &v)
{
    if (v.isObject())
        return QJsonDocument(v.toObject()).toJson(QJsonDocument::Compact);
    if (v.isArray())
        return QJsonDocument(v.toArray()).toJson(QJsonDocument::Compact);
    // Scalars: wrap in an array and strip the brackets.
    QByteArray a = QJsonDocument(QJsonArray{v}).toJson(QJsonDocument::Compact);
    return a.mid(1, a.size() - 2);
}

static QByteArray quoted(const QString &s)
{
    return compact(QJsonValue(s));
}

bool writeFile(const QString &path, const QList<Member> &members)
{
    QFile f(path);
    if (!f.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        std::fprintf(stderr, "cannot write %s\n", qPrintable(path));
        return false;
    }
    f.write("{\n");
    for (int i = 0; i < members.size(); ++i) {
        const Member &m = members[i];
        f.write("\"");
        f.write(m.key.toUtf8());
        f.write("\": ");
        if (m.oneElementPerLine && m.value.isArray()) {
            const QJsonArray arr = m.value.toArray();
            f.write("[\n");
            for (int j = 0; j < arr.size(); ++j) {
                f.write(compact(arr[j]));
                f.write(j + 1 < arr.size() ? ",\n" : "\n");
            }
            f.write("]");
        } else if (m.oneElementPerLine && m.value.isObject()) {
            const QJsonObject obj = m.value.toObject();
            f.write("{\n");
            const QStringList keys = obj.keys();
            for (int j = 0; j < keys.size(); ++j) {
                f.write(quoted(keys[j]));
                f.write(": ");
                f.write(compact(obj.value(keys[j])));
                f.write(j + 1 < keys.size() ? ",\n" : "\n");
            }
            f.write("}");
        } else {
            f.write(compact(m.value));
        }
        f.write(i + 1 < members.size() ? ",\n" : "\n");
    }
    f.write("}\n");
    return f.error() == QFile::NoError;
}

} // namespace golden
