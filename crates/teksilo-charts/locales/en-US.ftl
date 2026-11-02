# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

# The chart messages, for tr_widget! to check and to fall back on when no
# I18nManager is installed. The runtime copies, and every translation, are in
# crates/teksilo-widgets/locales/, the bundle every application registers;
# tests/locale_copy.rs keeps this file identical to the en-US one there.

chart-bar-summary =
    Bar chart, { $series ->
        [one] { $series } series
       *[other] { $series } series
    }, { $categories ->
        [one] { $categories } category
       *[other] { $categories } categories
    }
chart-line-summary =
    Line chart, { $series ->
        [one] { $series } series
       *[other] { $series } series
    }
chart-pie-summary =
    Pie chart, { $slices ->
        [one] { $slices } slice
       *[other] { $slices } slices
    }
chart-legend-name = Chart legend
