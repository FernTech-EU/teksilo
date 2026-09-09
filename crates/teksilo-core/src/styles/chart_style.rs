// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `BarChart` / `LineChart` / `PieChart` (teksilo-charts).
//! One of the few Tier-3 traits that returns pure-data recipes only — no
//! `make_*(cfg, ctx) -> WidgetId` methods, because charts are batched-paint.
//! [`GridViewStyle`](crate::styles::GridViewStyle) and
//! [`TextSelectionStyle`](crate::styles::TextSelectionStyle) have the same
//! shape, for the same reason. `RecipeChartStyle`,
//! the shipped default, lives in teksilo-charts itself (NOT teksilo-widgets), because
//! teksilo-charts does not depend on teksilo-widgets. teksilo-core only holds the trait
//! + the `Rc<dyn ChartStyle>` slot type.

use teksilo_tokens::Color;

use crate::styles::{BorderRecipe, FillRecipe, Theme};

#[derive(Clone, Copy, Debug)]
pub struct ChartFillContext<'a> {
    pub series_index: usize,
    pub resolved_color: Color,
    pub theme: &'a Theme,
}

pub trait ChartStyle: 'static {
    fn bar_fill(&self, cfg: &ChartFillContext) -> FillRecipe;
    fn area_fill(&self, cfg: &ChartFillContext, opacity: f32) -> FillRecipe;
    fn donut_fill(&self, cfg: &ChartFillContext) -> FillRecipe;
    fn gridline(&self, theme: &Theme) -> BorderRecipe;
}

pub type SharedChartStyle = std::rc::Rc<dyn ChartStyle>;
