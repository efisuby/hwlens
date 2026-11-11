//! Building blocks shared by the pages: palette, cards, graphs, bars, stat rows.

use gpui::{App, Div, PathBuilder, Rgba, SharedString, canvas, div, fill, point, prelude::*, px, size};

pub mod color {
    use gpui::{Rgba, rgb};
    pub fn bg() -> Rgba { rgb(0x0f1115) }
    pub fn sidebar() -> Rgba { rgb(0x12151b) }
    pub fn card() -> Rgba { rgb(0x171a21) }
    pub fn border() -> Rgba { rgb(0x262b36) }
    pub fn grid() -> Rgba { rgb(0x1f2430) }
    pub fn row_alt() -> Rgba { rgb(0x1a1e26) }
    pub fn hover() -> Rgba { rgb(0x1e2430) }
    pub fn selected() -> Rgba { rgb(0x23324d) }
    pub fn text() -> Rgba { rgb(0xe6e8ee) }
    pub fn muted() -> Rgba { rgb(0x8a92a6) }
    pub fn faint() -> Rgba { rgb(0x5a6274) }
    pub fn cpu() -> Rgba { rgb(0x4f9dff) }
    pub fn mem() -> Rgba { rgb(0x3ccf91) }
    pub fn down() -> Rgba { rgb(0x5ec4ff) }
    pub fn up() -> Rgba { rgb(0xff8a5c) }
    pub fn gpu() -> Rgba { rgb(0xb48cff) }
    pub fn disk() -> Rgba { rgb(0xf2c94c) }
    pub fn temp() -> Rgba { rgb(0xff7a7a) }
    pub fn warn() -> Rgba { rgb(0xffb547) }
    pub fn bad() -> Rgba { rgb(0xff5c6c) }

    /// Distinct colors for stacked or multi-series graphs.
    pub fn series(i: usize) -> Rgba {
        const C: [u32; 8] = [0x4f9dff, 0x9ad0ff, 0xff8a5c, 0xff5c6c, 0xf2c94c, 0xb48cff, 0x8a92a6, 0x3ccf91];
        rgb(C[i % C.len()])
    }

    /// Green → amber → red as a value approaches its limit.
    pub fn level(value: f32, warn_at: f32, bad_at: f32) -> Rgba {
        if value >= bad_at { bad() } else if value >= warn_at { warn() } else { mem() }
    }
}

pub const MONO: &str = "DejaVu Sans Mono";

pub fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_lg()
        .bg(color::card())
        .border_1()
        .border_color(color::border())
        .overflow_hidden()
}

pub fn title(text: impl Into<SharedString>, col: Rgba) -> Div {
    let text: SharedString = text.into();
    div()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(col)
        .truncate()
        .child(SharedString::from(text.to_uppercase()))
}

pub fn big(text: impl Into<SharedString>) -> Div {
    div().text_2xl().font_weight(gpui::FontWeight::SEMIBOLD).child(text.into())
}

pub fn sub(text: impl Into<SharedString>) -> Div {
    div().text_xs().text_color(color::muted()).truncate().child(text.into())
}

/// A "label ……… value" row.
pub fn stat(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .gap_2()
        .text_xs()
        .child(div().text_color(color::muted()).flex_shrink_0().child(label.into()))
        .child(div().flex_1())
        .child(div().font_family(MONO).truncate().child(value.into()))
}

/// Colored dot + label + value, used as a graph legend.
pub fn legend(col: Rgba, label: impl Into<SharedString>, value: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .text_xs()
        .child(div().size(px(8.)).rounded_sm().bg(col))
        .child(div().text_color(color::muted()).child(label.into()))
        .child(div().font_family(MONO).child(value.into()))
}

/// Horizontal fill bar, `fraction` in 0..=1.
pub fn bar(fraction: f32, col: Rgba) -> Div {
    segments(vec![(fraction, col)], 6.)
}

/// A bar split into consecutive colored parts (fractions of the whole), painted directly:
/// nested percentage widths inside stretched flex columns resolve to zero in taffy.
pub fn segments(parts: Vec<(f32, Rgba)>, height: f32) -> Div {
    div().h(px(height)).w_full().relative().child(
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _: &mut App| {
                let r = px(height / 2.0);
                window.paint_quad(fill(bounds, color::row_alt()).corner_radii(r));
                let w = f32::from(bounds.size.width);
                let mut x = 0.0;
                for (f, col) in &parts {
                    let seg = (f.clamp(0.0, 1.0) * w).min(w - x);
                    if seg <= 0.0 {
                        continue;
                    }
                    let b = gpui::Bounds::new(point(bounds.left() + px(x), bounds.top()), size(px(seg), bounds.size.height));
                    window.paint_quad(fill(b, *col).corner_radii(r));
                    x += seg;
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

#[derive(Clone)]
pub struct Graph {
    pub series: Vec<(Vec<f32>, Rgba)>,
    /// Top of the scale; None: the largest visible value (at least `min_max`).
    pub max: Option<f32>,
    pub min_max: f32,
    pub stacked: bool,
    pub grid: bool,
    pub line: bool,
    /// Pixels per sample.
    pub step: f32,
    /// Stretch the longest series over the whole width (short histories).
    pub fit: bool,
    /// A vertical line at this fraction of the width (a selected moment).
    pub marker: Option<f32>,
    /// Receives the painted bounds, so clicks can be mapped to a position.
    pub track: Option<Tracked>,
}

/// Bounds of a painted element, written during paint and read by mouse handlers.
pub type Tracked = std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>;

impl Graph {
    pub fn new() -> Self {
        Graph { series: Vec::new(), max: None, min_max: 1.0, stacked: false, grid: false, line: true, step: 2.0, fit: false, marker: None, track: None }
    }
    pub fn percent() -> Self {
        Graph { max: Some(100.0), ..Graph::new() }
    }
    pub fn series(mut self, values: &[f32], col: Rgba) -> Self {
        self.series.push((values.to_vec(), col));
        self
    }
    pub fn max(mut self, max: f32) -> Self {
        self.max = Some(max);
        self
    }
    pub fn min_max(mut self, m: f32) -> Self {
        self.min_max = m;
        self
    }
    pub fn stacked(mut self) -> Self {
        self.stacked = true;
        self.line = false;
        self
    }
    pub fn grid(mut self) -> Self {
        self.grid = true;
        self
    }
    pub fn fit(mut self) -> Self {
        self.fit = true;
        self
    }
    pub fn marker(mut self, at: Option<f32>) -> Self {
        self.marker = at;
        self
    }
    pub fn track(mut self, t: Tracked) -> Self {
        self.track = Some(t);
        self
    }

    /// The scale the graph would use for a given width (for an axis label).
    pub fn scale(&self, width_px: f32) -> f32 {
        if let Some(m) = self.max {
            return m;
        }
        let n = (width_px / self.step) as usize + 1;
        let tail = |v: &Vec<f32>| v.iter().rev().take(n).copied().collect::<Vec<_>>();
        let peak = if self.stacked {
            let tails: Vec<Vec<f32>> = self.series.iter().map(|(v, _)| tail(v)).collect();
            (0..n).map(|i| tails.iter().filter_map(|t| t.get(i)).sum::<f32>()).fold(0.0, f32::max)
        } else {
            self.series.iter().flat_map(|(v, _)| tail(v)).fold(0.0, f32::max)
        };
        // Round up to a "nice" number so the scale does not twitch every tick.
        nice_ceil(peak.max(self.min_max))
    }

    pub fn render(self) -> impl IntoElement {
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _: &mut App| {
                let mut this = self;
                if let Some(t) = &this.track {
                    t.set(Some(bounds));
                }
                let w = f32::from(bounds.size.width);
                if this.fit {
                    let longest = this.series.iter().map(|(v, _)| v.len()).max().unwrap_or(0);
                    this.step = w / longest.saturating_sub(1).max(1) as f32;
                }
                let this = this;
                let h = f32::from(bounds.size.height);
                if this.grid {
                    for i in 1..4 {
                        let y = bounds.top() + px(h * i as f32 / 4.0);
                        window.paint_quad(fill(gpui::Bounds::new(point(bounds.left(), y), size(bounds.size.width, px(1.))), color::grid()));
                    }
                }
                let max = this.scale(w).max(f32::EPSILON);
                let n_fit = (w / this.step) as usize + 1;
                let y_of = |v: f32| bounds.top() + px(h - (v / max).clamp(0.0, 1.0) * h);
                let mut base: Vec<f32> = Vec::new();
                for (values, col) in &this.series {
                    let n = n_fit.min(values.len());
                    if n < 2 {
                        continue;
                    }
                    let vals = &values[values.len() - n..];
                    let x0 = bounds.right() - px(this.step * (n - 1) as f32);
                    let x_of = |i: usize| x0 + px(this.step * i as f32);
                    if base.len() < n {
                        base.splice(0..0, std::iter::repeat_n(0.0, n - base.len()));
                    }
                    let b = &base[base.len() - n..];
                    let top: Vec<f32> =
                        if this.stacked { vals.iter().zip(b).map(|(v, b)| v + b).collect() } else { vals.to_vec() };

                    let mut area = PathBuilder::fill();
                    if this.stacked {
                        area.move_to(point(x_of(0), y_of(b[0])));
                        for (i, &v) in top.iter().enumerate() {
                            area.line_to(point(x_of(i), y_of(v)));
                        }
                        for i in (0..n).rev() {
                            area.line_to(point(x_of(i), y_of(b[i])));
                        }
                    } else {
                        area.move_to(point(x0, bounds.bottom()));
                        for (i, &v) in top.iter().enumerate() {
                            area.line_to(point(x_of(i), y_of(v)));
                        }
                        area.line_to(bounds.bottom_right());
                    }
                    area.close();
                    if let Ok(path) = area.build() {
                        let mut f = *col;
                        f.a = if this.stacked { 0.75 } else { 0.14 };
                        window.paint_path(path, f);
                    }
                    if this.line {
                        let mut line = PathBuilder::stroke(px(1.5));
                        line.move_to(point(x_of(0), y_of(top[0])));
                        for (i, &v) in top.iter().enumerate().skip(1) {
                            line.line_to(point(x_of(i), y_of(v)));
                        }
                        if let Ok(path) = line.build() {
                            window.paint_path(path, *col);
                        }
                    }
                    if this.stacked {
                        let off = base.len() - n;
                        for (i, v) in top.into_iter().enumerate() {
                            base[off + i] = v;
                        }
                    }
                }
                if let Some(f) = this.marker {
                    let x = bounds.left() + px((f.clamp(0.0, 1.0) * w).round());
                    window.paint_quad(fill(gpui::Bounds::new(point(x, bounds.top()), size(px(1.), bounds.size.height)), color::text()));
                }
            },
        )
        .size_full()
    }
}

/// A graph in a box of fixed height, with an optional top-right scale label.
pub fn graph_box(g: Graph, height: f32, label: Option<String>) -> Div {
    div()
        .relative()
        .h(px(height))
        .w_full()
        .rounded_md()
        .bg(color::bg())
        .overflow_hidden()
        .child(div().absolute().top_0().left_0().right_0().bottom_0().child(g.render()))
        .children(label.map(|l| {
            div().absolute().top(px(2.)).right(px(4.)).text_xs().text_color(color::faint()).child(l)
        }))
}

pub fn nice_ceil(v: f32) -> f32 {
    if v <= 0.0 {
        return 1.0;
    }
    let mag = 10f32.powf(v.log10().floor());
    for m in [1.0, 2.0, 2.5, 5.0, 10.0] {
        if v <= m * mag {
            return m * mag;
        }
    }
    10.0 * mag
}

pub fn avg(v: &[f32], n: usize) -> f32 {
    let t: Vec<f32> = v.iter().rev().take(n).copied().collect();
    if t.is_empty() { 0.0 } else { t.iter().sum::<f32>() / t.len() as f32 }
}

pub fn peak(v: &[f32], n: usize) -> f32 {
    v.iter().rev().take(n).copied().fold(0.0, f32::max)
}

pub fn last(v: &[f32]) -> f32 {
    v.last().copied().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_ceil_rounds_up() {
        assert_eq!(nice_ceil(0.0), 1.0);
        assert_eq!(nice_ceil(3.0), 5.0);
        assert_eq!(nice_ceil(1100.0), 2000.0);
        assert_eq!(nice_ceil(2400.0), 2500.0);
        assert_eq!(nice_ceil(100.0), 100.0);
    }

    #[test]
    fn stacked_scale_sums_series() {
        let g = Graph::new().series(&[10.0, 20.0], color::cpu()).series(&[30.0, 70.0], color::up()).stacked();
        assert_eq!(g.scale(100.0), 100.0);
    }
}
