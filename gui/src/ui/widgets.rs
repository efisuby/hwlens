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
    /// Behind text drawn over a graph.
    pub fn backdrop() -> Rgba { Rgba { r: 0.09, g: 0.10, b: 0.13, a: 0.72 } }
    pub fn tooltip() -> Rgba { Rgba { r: 0.07, g: 0.08, b: 0.10, a: 0.92 } }

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

/// Formats a value for the hover tooltip.
pub type Fmt = fn(f32) -> String;

fn fmt_plain(v: f32) -> String {
    if v.abs() >= 100.0 { format!("{v:.0}") } else { format!("{v:.1}") }
}
pub fn fmt_pct(v: f32) -> String {
    format!("{v:.1} %")
}
pub fn fmt_rate(v: f32) -> String {
    crate::model::rate(v)
}
pub fn fmt_bytes(v: f32) -> String {
    crate::model::bytes(v.max(0.0) as u64)
}
pub fn fmt_celsius(v: f32) -> String {
    format!("{v:.0} °C")
}

#[derive(Clone)]
pub struct Graph {
    /// Values oldest first, color, name for the tooltip.
    pub series: Vec<(Vec<f32>, Rgba, &'static str)>,
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
    /// Unix time of the last sample and seconds per sample; None: the live data of hwlensd.
    pub time: Option<(f64, f64)>,
    pub fmt: Fmt,
    /// Set by graph_area: the slot that remembers this graph's geometry, and the hovered time.
    slot: Option<usize>,
    hover: Option<f64>,
}

impl Graph {
    pub fn new() -> Self {
        Graph {
            series: Vec::new(),
            max: None,
            min_max: 1.0,
            stacked: false,
            grid: false,
            line: true,
            step: 2.0,
            fit: false,
            marker: None,
            time: None,
            fmt: fmt_plain,
            slot: None,
            hover: None,
        }
    }
    pub fn percent() -> Self {
        Graph { max: Some(100.0), fmt: fmt_pct, ..Graph::new() }
    }
    pub fn series(mut self, values: &[f32], col: Rgba) -> Self {
        self.series.push((values.to_vec(), col, ""));
        self
    }
    /// Names the last added series in the tooltip.
    pub fn name(mut self, name: &'static str) -> Self {
        if let Some(s) = self.series.last_mut() {
            s.2 = name;
        }
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
    pub fn fmt(mut self, f: Fmt) -> Self {
        self.fmt = f;
        self
    }
    /// The samples are `dt` seconds apart and the last one is at unix time `last`.
    pub fn time(mut self, last: f64, dt: f64) -> Self {
        self.time = Some((last, dt));
        self
    }

    fn longest(&self) -> usize {
        self.series.iter().map(|(v, _, _)| v.len()).max().unwrap_or(0)
    }

    /// Pixels per sample at a width.
    fn step_for(&self, w: f32) -> f32 {
        if self.fit { w / self.longest().saturating_sub(1).max(1) as f32 } else { self.step }
    }

    /// The scale the graph would use for a given width (for an axis label).
    pub fn scale(&self, width_px: f32) -> f32 {
        if let Some(m) = self.max {
            return m;
        }
        let n = ((width_px / self.step_for(width_px)) as usize).saturating_add(1);
        let tail = |v: &Vec<f32>| v.iter().rev().take(n).copied().collect::<Vec<_>>();
        let peak = if self.stacked {
            let tails: Vec<Vec<f32>> = self.series.iter().map(|(v, _, _)| tail(v)).collect();
            (0..n.min(self.longest())).map(|i| tails.iter().filter_map(|t| t.get(i)).sum::<f32>()).fold(0.0, f32::max)
        } else {
            self.series.iter().flat_map(|(v, _, _)| tail(v)).fold(0.0, f32::max)
        };
        // Round up to a "nice" number so the scale does not twitch every tick.
        nice_ceil(peak.max(self.min_max))
    }

    pub fn render(self) -> impl IntoElement {
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _: &mut App| {
                let mut this = self;
                let w = f32::from(bounds.size.width);
                this.step = this.step_for(w);
                if let Some(slot) = this.slot {
                    remember(slot, bounds, this.step);
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
                for (values, col, _) in &this.series {
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
                let vline = |x: gpui::Pixels, col: Rgba, window: &mut gpui::Window| {
                    window.paint_quad(fill(gpui::Bounds::new(point(x.round(), bounds.top()), size(px(1.), bounds.size.height)), col));
                };
                if let Some(f) = this.marker {
                    vline(bounds.left() + px(f.clamp(0.0, 1.0) * w), color::text(), window);
                }
                // The hovered moment, on this graph and on every other one showing that time.
                if let (Some(t), Some((last, dt))) = (this.hover, this.time) {
                    let age = (last - t) / dt;
                    if age >= -0.5 && age <= (this.longest().saturating_sub(1)) as f64 + 0.5 {
                        let x = bounds.right() - px(this.step * age as f32);
                        if x >= bounds.left() {
                            let mut c = color::text();
                            c.a = 0.55;
                            vline(x, c, window);
                        }
                    }
                }
            },
        )
        .size_full()
    }
}

/// Hover state shared by every graph of a frame. Set by the view before it renders
/// (`begin_frame`); only the UI thread draws, so a thread-local is enough.
struct HoverState {
    /// The hovered unix time, if the mouse is over a graph.
    t: Option<f64>,
    /// When the latest live snapshot arrived, and seconds per live sample.
    live_last: f64,
    live_dt: f64,
    view: Option<gpui::WeakEntity<super::HwlensView>>,
    /// Graphs of the frame are numbered in build order, which is stable from frame to frame;
    /// their painted bounds and pixels per sample are kept for the mouse and the tooltip.
    next: usize,
    geometry: std::collections::HashMap<usize, (gpui::Bounds<gpui::Pixels>, f32)>,
}

thread_local! {
    static HOVER: std::cell::RefCell<HoverState> = std::cell::RefCell::new(HoverState {
        t: None,
        live_last: 0.0,
        live_dt: 1.0,
        view: None,
        next: 0,
        geometry: Default::default(),
    });
}

pub fn begin_frame(t: Option<f64>, live_last: f64, live_dt: f64, view: gpui::WeakEntity<super::HwlensView>, page_changed: bool) {
    HOVER.with_borrow_mut(|h| {
        h.t = t;
        h.live_last = live_last;
        h.live_dt = live_dt;
        h.view = Some(view);
        h.next = 0;
        if page_changed {
            h.geometry.clear();
        }
    });
}

fn remember(slot: usize, bounds: gpui::Bounds<gpui::Pixels>, step: f32) {
    HOVER.with_borrow_mut(|h| {
        h.geometry.insert(slot, (bounds, step));
    });
}

fn set_hover(t: Option<f64>, cx: &mut App) {
    let view = HOVER.with_borrow(|h| h.view.clone());
    if let Some(v) = view {
        let _ = v.update(cx, |view, cx| view.set_hover(t, cx));
    }
}

/// Unix time now, with fractions.
pub fn unix_now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// A graph filling its box, with the hover line and, for boxes tall enough, a tooltip with the
/// values at the hovered moment. `overlay` is drawn over the graph (text on a dim backdrop).
fn graph_area(mut g: Graph, overlay: Option<Div>, label: Option<String>, tooltip: bool) -> gpui::Stateful<Div> {
    let (slot, hover, live_last, live_dt, geo) = HOVER.with_borrow_mut(|h| {
        let slot = h.next;
        h.next += 1;
        (slot, h.t, h.live_last, h.live_dt, h.geometry.get(&slot).copied())
    });
    let live = g.time.is_none();
    let (last, dt) = *g.time.get_or_insert((live_last, live_dt));
    g.slot = Some(slot);
    g.hover = hover;

    // Tooltip from the previous frame's geometry: the layout does not move between frames.
    let mut tip = None;
    if let (true, Some(t), Some((b, step))) = (tooltip, hover, geo) {
        let age = (last - t) / dt;
        let longest = g.longest();
        let w = f32::from(b.size.width);
        let x = w - step * age as f32;
        if longest > 0 && age > -0.5 && age < longest as f64 - 0.5 && x >= 0.0 {
            let mut rows = div().flex().flex_col().gap(px(1.)).text_xs().font_family(MONO).child(
                div().text_color(color::muted()).child(if live {
                    if age < 0.5 { "now".to_string() } else { format!("−{:.0} s", age * dt) }
                } else {
                    super::history::local_time(t.round() as i64, true)
                }),
            );
            let i = age.round() as usize;
            for (values, col, name) in &g.series {
                let Some(v) = values.len().checked_sub(1 + i).and_then(|k| values.get(k)) else { continue };
                rows = rows.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(div().size(px(6.)).rounded_sm().bg(*col))
                        .when(!name.is_empty(), |d| d.child(div().text_color(color::muted()).child(*name)))
                        .child((g.fmt)(*v)),
                );
            }
            let boxed = rows.px_1().py(px(2.)).rounded_sm().bg(color::tooltip()).border_1().border_color(color::border());
            // Right of the line, or left of it near the right edge.
            // At the bottom: text drawn over graphs sits at the top.
            tip = Some(if x < w - 150.0 {
                div().absolute().bottom(px(4.)).left(px(x + 6.0)).child(boxed)
            } else {
                div().absolute().bottom(px(4.)).right(px(w - x + 6.0)).child(boxed)
            });
        }
    }

    // No w_full: a percentage width inside a stretched flex column resolves wrong in taffy;
    // the parent's stretch gives the width. The root is a flex column so the overlay stretches too.
    div()
        .id(("graph", slot))
        .relative()
        .flex()
        .flex_col()
        .overflow_hidden()
        .on_mouse_move(move |ev: &gpui::MouseMoveEvent, _, cx| {
            let Some((b, step)) = HOVER.with_borrow(|h| h.geometry.get(&slot).copied()) else { return };
            let age = f32::from(b.right() - ev.position.x) / step.max(0.001);
            set_hover(Some(last - age as f64 * dt), cx);
        })
        .on_hover(|hovered: &bool, _, cx| {
            if !*hovered {
                set_hover(None, cx);
            }
        })
        .child(div().absolute().top_0().left_0().right_0().bottom_0().child(g.render()))
        .children(label.map(|l| div().absolute().top(px(2.)).right(px(4.)).text_xs().text_color(color::faint()).child(l)))
        .children(overlay)
        .children(tip)
}

/// A graph in a box of fixed height, with an optional top-right scale label.
pub fn graph_box(g: Graph, height: f32, label: Option<String>) -> gpui::Stateful<Div> {
    graph_area(g, None, label, height >= 60.0).h(px(height)).rounded_md().bg(color::bg())
}

/// A graph box with `content` drawn over it (see graph_card), for use inside a card.
pub fn graph_panel(g: Graph, height: f32, content: Div) -> gpui::Stateful<Div> {
    graph_area(g, Some(div().relative().p_2().flex().flex_col().child(content)), None, height >= 100.0)
        .h(px(height))
        .rounded_md()
        .bg(color::bg())
}

/// A card that is all graph: `content` (title, values, legend) sits on top of it, each piece
/// on a dim backdrop (use `chip`). Saves the space a graph under the text would take.
pub fn graph_card(g: Graph, height: f32, content: Div) -> gpui::Stateful<Div> {
    graph_area(g, Some(div().relative().p_3().flex().flex_col().child(content)), None, height >= 100.0)
        .h(px(height))
        .rounded_lg()
        .bg(color::card())
        .border_1()
        .border_color(color::border())
}

/// Content over a graph: a column whose pieces keep their own width (and backdrop).
pub fn overlay() -> Div {
    div().flex().flex_col().items_start().gap_1()
}

/// A row across the whole graph (title left, value right) among content laid out at the start.
pub fn overlay_row() -> Div {
    let mut d = div().flex().items_center().gap_2();
    d.style().align_self = Some(gpui::AlignItems::Stretch);
    d
}

/// Text over a graph: a dim, rounded backdrop that keeps it readable.
pub fn chip(d: Div) -> Div {
    d.px_1().rounded_sm().bg(color::backdrop())
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
