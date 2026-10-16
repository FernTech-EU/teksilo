// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech
//
// JSON helpers shared by every vector writer. A colour is always written the
// same way (see README.md, "Colour encoding"), and every file is written as
// one object whose arrays hold one record per line, so a regenerated file
// diffs line by line.

#pragma once

#include <QColor>
#include <QJsonArray>
#include <QJsonObject>
#include <QJsonValue>
#include <QList>
#include <QPair>
#include <QString>

namespace golden {

// "#rrggbbaa" from Qt's own 8-bit accessors.
QString hex8(const QColor &c);

// {"hex": "#rrggbbaa", "u16": [r, g, b, a]}, plus "spec" and "f" (exact
// doubles) when the colour is not stored in QColor::Rgb, and {"valid": false}
// for an invalid colour.
QJsonObject color(const QColor &c);

// Parses "#rrggbb" or "#rrggbbaa" into an 8-bit QColor (16-bit = 8-bit * 257).
QColor parseHex8(const QString &s);

// Metadata common to every file: generator, versions, environment.
QJsonObject meta(const QString &kind);

// One top-level member: either an array (written one element per line) or any
// other value (written compact on one line).
struct Member {
    QString key;
    QJsonValue value;
    bool oneElementPerLine = false;
};

// Writes {"k": v, ...} with arrays one element per line. Returns false and
// prints to stderr on failure.
bool writeFile(const QString &path, const QList<Member> &members);

} // namespace golden
