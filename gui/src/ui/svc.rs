//! Services page: systemd system and user services with their cgroup resources and actions.

use gpui::{Context, Div, Rgba, Stateful, div, prelude::*, px, uniform_list};

use super::{
    HwlensView,
    procs::button,
    widgets::{MONO, card, color, legend, stat, sub, title},
};
use crate::{
    model::{bytes, rate},
    services::{Scope, Unit},
};

#[derive(Clone, Copy, PartialEq)]
pub enum Show {
    All,
    Running,
    Failed,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SvcSort {
    Name,
    Cpu,
    Mem,
    Io,
}

pub struct SvcState {
    pub units: Vec<Unit>,
    /// Indices into `units` after filtering and sorting.
    pub rows: Vec<usize>,
    pub filter: String,
    pub show: Show,
    pub scope: Option<Scope>,
    pub sort: SvcSort,
    pub selected: Option<(Scope, String)>,
    pub journal: Option<(Scope, String, String)>,
    pub error: Option<String>,
    pub loaded: bool,
}

impl Default for SvcState {
    fn default() -> Self {
        SvcState {
            units: Vec::new(),
            rows: Vec::new(),
            filter: String::new(),
            show: Show::All,
            scope: None,
            sort: SvcSort::Cpu,
            selected: None,
            journal: None,
            error: None,
            loaded: false,
        }
    }
}

impl SvcState {
    pub fn rebuild(&mut self) {
        let needle = self.filter.to_lowercase();
        let mut rows: Vec<usize> = (0..self.units.len())
            .filter(|&i| {
                let u = &self.units[i];
                let shown = match self.show {
                    Show::All => true,
                    Show::Running => u.active == "active" || u.active == "reloading",
                    Show::Failed => u.active == "failed",
                };
                shown
                    && self.scope.is_none_or(|s| s == u.scope)
                    && (needle.is_empty() || u.name.to_lowercase().contains(&needle) || u.description.to_lowercase().contains(&needle))
            })
            .collect();
        let u = &self.units;
        let key = |i: &usize| -> f32 {
            let x = &u[*i].usage;
            match self.sort {
                SvcSort::Cpu => x.cpu.unwrap_or(-1.0),
                SvcSort::Mem => x.mem.map(|m| m as f32).unwrap_or(-1.0),
                SvcSort::Io => x.io.unwrap_or(-1.0),
                SvcSort::Name => 0.0,
            }
        };
        rows.sort_by(|a, b| key(b).total_cmp(&key(a)).then_with(|| u[*a].name.cmp(&u[*b].name)).then((u[*a].scope as u8).cmp(&(u[*b].scope as u8))));
        self.rows = rows;
    }

    pub fn selected_unit(&self) -> Option<&Unit> {
        let (scope, name) = self.selected.as_ref()?;
        self.units.iter().find(|u| u.scope == *scope && &u.name == name)
    }

    pub fn selected_index(&self) -> Option<usize> {
        let (scope, name) = self.selected.as_ref()?;
        self.rows.iter().position(|&i| self.units[i].scope == *scope && &self.units[i].name == name)
    }
}

fn active_color(active: &str) -> Rgba {
    match active {
        "active" | "reloading" => color::mem(),
        "failed" => color::bad(),
        "activating" | "deactivating" => color::warn(),
        _ => color::faint(),
    }
}

/// Column title, width (None: the rest), sort key, right aligned.
const COLUMNS: [(&str, Option<f32>, Option<SvcSort>, bool); 8] = [
    ("Unit", Some(300.), Some(SvcSort::Name), false),
    ("", Some(52.), None, false),
    ("State", Some(130.), None, false),
    ("Enabled", Some(90.), None, false),
    ("CPU", Some(70.), Some(SvcSort::Cpu), true),
    ("Memory", Some(84.), Some(SvcSort::Mem), true),
    ("Tasks", Some(54.), None, true),
    ("I/O", Some(84.), Some(SvcSort::Io), true),
];

fn cell(width: Option<f32>, right: bool) -> Div {
    let c = div().px_1().whitespace_nowrap().overflow_hidden().truncate().when(right, |d| d.text_right());
    match width {
        Some(w) => c.w(px(w)).flex_shrink_0(),
        None => c.flex_1().min_w_0(),
    }
}

impl HwlensView {
    pub(super) fn svc_page(&mut self, cx: &mut Context<Self>) -> Div {
        let st = &self.svc;
        if !st.loaded {
            let text = st.error.clone().unwrap_or_else(|| "reading systemd units…".into());
            return div().child(sub(text));
        }
        let count = |f: &dyn Fn(&Unit) -> bool| st.units.iter().filter(|u| f(u)).count();
        let running = count(&|u| u.active == "active");
        let failed: Vec<&Unit> = st.units.iter().filter(|u| u.active == "failed").collect();
        let user = count(&|u| u.scope == Scope::User);

        let toggle = |id: &'static str, label: String, on: bool| {
            button(id, &label, if on { color::text() } else { color::muted() }).when(on, |b| b.bg(color::selected()))
        };
        let show_btn = |id, label: String, show: Show, cx: &mut Context<Self>| {
            toggle(id, label, st.show == show).on_click(cx.listener(move |this, _, _, cx| {
                this.svc.show = show;
                this.svc.rebuild();
                cx.notify();
            }))
        };
        let scope_btn = |id, label: &str, scope: Option<Scope>, cx: &mut Context<Self>| {
            toggle(id, label.to_owned(), st.scope == scope).on_click(cx.listener(move |this, _, _, cx| {
                this.svc.scope = scope;
                this.svc.rebuild();
                cx.notify();
            }))
        };

        let filter = if st.filter.is_empty() {
            div().text_color(color::faint()).child("type to filter")
        } else {
            div().px_2().rounded_md().bg(color::selected()).child(format!("filter: {}▏", st.filter))
        };
        let head = div()
            .flex()
            .items_center()
            .gap_2()
            .child(title("Services", color::text()))
            .child(div().text_xs().text_color(color::muted()).child(format!("{} shown", st.rows.len())))
            .child(div().flex_1())
            .child(filter)
            .child(show_btn("sv-all", format!("All {}", st.units.len()), Show::All, cx))
            .child(show_btn("sv-run", format!("Running {running}"), Show::Running, cx))
            .child(show_btn("sv-fail", format!("Failed {}", failed.len()), Show::Failed, cx).when(!failed.is_empty(), |b| b.text_color(color::bad())))
            .child(div().w(px(8.)))
            .child(scope_btn("sc-any", "System + user", None, cx))
            .child(scope_btn("sc-sys", "System", Some(Scope::System), cx))
            .child(scope_btn("sc-usr", "User", Some(Scope::User), cx));

        let columns = div()
            .flex()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(color::border())
            .text_xs()
            .text_color(color::muted())
            .children(COLUMNS.iter().enumerate().map(|(i, &(name, w, key, right))| {
                let arrow = if key == Some(st.sort) { " ▼" } else { "" };
                let el = cell(w, right).id(("svc-col", i)).child(format!("{name}{arrow}"));
                match key {
                    Some(k) => el.cursor_pointer().hover(|s| s.text_color(color::text())).on_click(cx.listener(move |this, _, _, cx| {
                        this.svc.sort = k;
                        this.svc.rebuild();
                        cx.notify();
                    })),
                    None => el,
                }
            }))
            .child(cell(None, false).child("Description"));
        let list = uniform_list(
            "svc",
            st.rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _window, cx| range.map(|ix| this.svc_row(ix, cx)).collect::<Vec<_>>()),
        )
        .track_scroll(self.svc_scroll.clone())
        .flex_1();

        let summary = card()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_4()
            .child(legend(color::mem(), "running", running.to_string()))
            .child(legend(color::bad(), "failed", failed.len().to_string()))
            .child(legend(color::cpu(), "system", (st.units.len() - user).to_string()))
            .child(legend(color::gpu(), "user", user.to_string()))
            .children(failed.iter().take(4).map(|u| div().max_w(px(260.)).truncate().text_xs().text_color(color::bad()).child(format!("✕ {}", u.name))))
            .when(failed.len() > 4, |d| d.child(sub(format!("+{} more failed", failed.len() - 4))))
            .when_some(st.error.clone(), |d, e| d.child(div().text_xs().text_color(color::bad()).child(e)));

        let table = card().flex_1().min_w_0().min_h(px(200.)).child(head).child(columns).child(list);
        let mut page = div().flex().flex_col().gap_3().size_full().child(summary).child(table);
        if let Some(u) = st.selected_unit().cloned() {
            page = page.child(self.svc_details(u, cx));
        }
        page
    }

    fn svc_row(&self, ix: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let u = &self.svc.units[self.svc.rows[ix]];
        let key = (u.scope, u.name.clone());
        let selected = self.svc.selected.as_ref() == Some(&key);
        let x = &u.usage;
        let dash = || "—".to_owned();
        let values: [(String, Rgba); 8] = [
            (u.name.clone(), if u.load == "not-found" { color::faint() } else { color::text() }),
            (u.scope.name().to_owned(), if u.scope == Scope::User { color::gpu() } else { color::faint() }),
            (format!("{} · {}", u.active, u.sub), active_color(&u.active)),
            (u.file_state.clone(), if u.file_state == "enabled" { color::text() } else { color::muted() }),
            (x.cpu.map(|c| format!("{c:.1} %")).unwrap_or_else(dash), if x.cpu.unwrap_or(0.0) >= 50.0 { color::warn() } else { color::text() }),
            (x.mem.map(bytes).unwrap_or_else(dash), color::text()),
            (x.tasks.map(|t| t.to_string()).unwrap_or_else(dash), color::muted()),
            (x.io.filter(|v| *v > 0.0).map(rate).unwrap_or_else(dash), color::text()),
        ];
        div()
            .id(("svc", ix))
            .w_full()
            .flex()
            .px_2()
            .py(px(2.))
            .cursor_pointer()
            .bg(if selected { color::selected() } else if ix % 2 == 1 { color::row_alt() } else { color::card() })
            .hover(|s| s.bg(color::hover()))
            .text_xs()
            .font_family(MONO)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.svc_select(key.clone());
                cx.notify();
            }))
            .children(COLUMNS.iter().zip(values).map(|(&(_, w, _, right), (text, col))| cell(w, right).text_color(col).child(text)))
            .child(cell(None, false).text_color(color::muted()).font_family("Noto Sans").child(u.description.clone()))
    }

    pub(super) fn svc_select(&mut self, key: (Scope, String)) {
        if self.svc.selected.as_ref() == Some(&key) {
            return;
        }
        self.services.journal(key.0, key.1.clone());
        self.svc.journal = None;
        self.svc.selected = Some(key);
    }

    fn svc_details(&self, u: Unit, cx: &mut Context<Self>) -> Div {
        let action = |id: &'static str, label: &str, verb: &'static str, col: Rgba, cx: &mut Context<Self>| {
            let (scope, name) = (u.scope, u.name.clone());
            button(id, label, col).on_click(cx.listener(move |this, _, _, cx| {
                this.message = Some((true, format!("{verb} {name}…")));
                this.services.action(scope, verb, name.clone());
                let (s, n) = (scope, name.clone());
                // Fresh journal lines after the action has had a moment to run.
                let svc = this.services.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    svc.journal(s, n);
                });
                cx.notify();
            }))
        };
        let running = u.active == "active";
        let mut actions = div().flex().flex_wrap().gap_2();
        actions = if running {
            actions.child(action("sv-stop", "Stop", "stop", color::warn(), cx)).child(action("sv-restart", "Restart", "restart", color::text(), cx)).child(action(
                "sv-reload",
                "Reload",
                "reload",
                color::text(),
                cx,
            ))
        } else {
            actions.child(action("sv-start", "Start", "start", color::mem(), cx))
        };
        actions = match u.file_state.as_str() {
            "enabled" => actions.child(action("sv-disable", "Disable", "disable", color::warn(), cx)),
            "disabled" => actions.child(action("sv-enable", "Enable", "enable", color::mem(), cx)),
            _ => actions,
        };
        if u.active == "failed" {
            actions = actions.child(action("sv-reset", "Reset failed", "reset-failed", color::text(), cx));
        }
        let pin_key = format!("svc.{}.{}", u.scope.name(), u.name);
        actions = actions.child(self.pins(cx).button(pin_key));
        actions = actions.child(
            button("sv-close", "Close", color::muted()).on_click(cx.listener(|this, _, _, cx| {
                this.svc.selected = None;
                cx.notify();
            })),
        );

        let journal = match &self.svc.journal {
            Some((s, n, text)) if *s == u.scope && *n == u.name => text.clone(),
            _ => "reading journal…".into(),
        };
        let lines = journal.lines().map(|l| div().whitespace_nowrap().child(l.to_owned()));
        let info = div()
            .w(px(320.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_color(active_color(&u.active)).child(u.name.clone()))
            .child(sub(u.description.clone()))
            .child(stat("Manager", u.scope.name()))
            .child(stat("State", format!("{} · {}", u.active, u.sub)))
            .child(stat("Unit file", if u.file_state.is_empty() { "—".to_owned() } else { u.file_state.clone() }))
            .when_some(u.usage.mem, |d, m| d.child(stat("Memory", bytes(m))))
            .when_some(u.usage.tasks, |d, t| d.child(stat("Tasks", t.to_string())))
            .child(actions);
        card().h(px(260.)).flex_shrink_0().flex_row().gap_3().child(info).child(
            div()
                .id("svc-journal")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .rounded_md()
                .bg(color::bg())
                .p_2()
                .text_xs()
                .font_family(MONO)
                .text_color(color::muted())
                .children(lines),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::Usage;

    fn unit(scope: Scope, name: &str, active: &str, cpu: f32) -> Unit {
        Unit {
            scope,
            name: name.into(),
            load: "loaded".into(),
            active: active.into(),
            sub: String::new(),
            description: format!("{name} daemon"),
            file_state: "enabled".into(),
            usage: Usage { cpu: Some(cpu), ..Usage::default() },
        }
    }

    #[test]
    fn filters_and_sorts() {
        let mut st = SvcState {
            units: vec![
                unit(Scope::System, "ssh.service", "active", 0.1),
                unit(Scope::System, "bad.service", "failed", 0.0),
                unit(Scope::User, "pipewire.service", "active", 3.0),
            ],
            ..SvcState::default()
        };
        st.rebuild();
        assert_eq!(st.rows, vec![2, 0, 1], "by cpu, busiest first");
        st.show = Show::Failed;
        st.rebuild();
        assert_eq!(st.rows, vec![1]);
        st.show = Show::All;
        st.scope = Some(Scope::User);
        st.rebuild();
        assert_eq!(st.rows, vec![2]);
        st.scope = None;
        st.filter = "SSH".into();
        st.rebuild();
        assert_eq!(st.rows, vec![0]);
        st.selected = Some((Scope::System, "ssh.service".into()));
        assert_eq!(st.selected_index(), Some(0));
    }
}
