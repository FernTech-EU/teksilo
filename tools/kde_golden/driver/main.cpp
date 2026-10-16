// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech
//
// kde_golden: writes the reference vectors for the KDE Breeze preset from a
// real KDE installation. It calls only public Qt 6 and KDE Frameworks 6 API.
//
//   kde_golden math    OUT.json
//   kde_golden resolve OUT.json
//   kde_golden sizes   OUT.json
//   kde_golden pixels  OUT.json SCHEME_LABEL
//
// run.sh drives it inside the container; README.md documents the output.

#include <QApplication>
#include <QStyle>
#include <QStyleFactory>

#include <cstdio>

int writeMath(const QString &path);
int writeResolve(const QString &path);
int writeSizes(const QString &path);
int writePixels(const QString &path, const QString &schemeLabel);

int main(int argc, char **argv)
{
    if (qEnvironmentVariable("QT_QPA_PLATFORM") != QLatin1String("offscreen")) {
        std::fprintf(stderr, "kde_golden: run with QT_QPA_PLATFORM=offscreen\n");
        return 2;
    }
    QApplication app(argc, argv);
    const QStringList args = app.arguments();
    if (args.size() < 3) {
        std::fprintf(stderr, "usage: kde_golden math|resolve|sizes|pixels OUT.json [SCHEME_LABEL]\n");
        return 2;
    }
    const QString mode = args[1];
    const QString out = args[2];

    if (mode == QLatin1String("sizes") || mode == QLatin1String("pixels")) {
        QStyle *breeze = QStyleFactory::create(QStringLiteral("Breeze"));
        if (!breeze) {
            std::fprintf(stderr, "kde_golden: the Breeze style plugin is not available (have: %s)\n",
                         qPrintable(QStyleFactory::keys().join(QLatin1String(", "))));
            return 1;
        }
        QApplication::setStyle(breeze);
    }

    if (mode == QLatin1String("math"))
        return writeMath(out);
    if (mode == QLatin1String("resolve"))
        return writeResolve(out);
    if (mode == QLatin1String("sizes"))
        return writeSizes(out);
    if (mode == QLatin1String("pixels"))
        return writePixels(out, args.size() > 3 ? args[3] : QString());
    std::fprintf(stderr, "kde_golden: unknown mode %s\n", qPrintable(mode));
    return 2;
}
