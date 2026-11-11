//! hwlens-gui: a desktop client of hwlensd built with GPUI.

mod client;
mod model;
mod patch;
mod services;
mod ui;

use std::path::PathBuf;

use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions, prelude::*, px, size,
};

struct Args {
    socket: PathBuf,
    page: ui::Page,
}

fn args() -> Args {
    let mut socket = None;
    let mut page = ui::Page::Overview;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--socket" | "-s" => socket = args.next().map(PathBuf::from),
            "--page" | "-p" => {
                let name = args.next().unwrap_or_default();
                page = ui::Page::from_name(&name).unwrap_or_else(|| {
                    eprintln!("hwlens-gui: unknown page {name:?}; pages: {}", ui::Page::names().join(", "));
                    std::process::exit(2);
                });
            }
            "--help" | "-h" => {
                println!(
                    "usage: hwlens-gui [--socket PATH] [--page NAME]\n\nDefault socket: $HWLENSD_SOCKET or {}\nPages: {}",
                    client::DEFAULT_SOCKET,
                    ui::Page::names().join(", ")
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("hwlens-gui: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let socket = socket
        .or_else(|| std::env::var_os("HWLENSD_SOCKET").map(PathBuf::from))
        .unwrap_or_else(|| client::DEFAULT_SOCKET.into());
    Args { socket, page }
}

fn main() {
    let Args { socket, page } = args();
    Application::new().run(move |cx: &mut App| {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let services = services::Services::start(tx.clone());
        let requests = client::start(socket, tx);
        let bounds = Bounds::centered(None, size(px(1280.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some("hwlens".into()), ..Default::default() }),
                window_decorations: Some(WindowDecorations::Server),
                app_id: Some("hwlens-gui".into()),
                window_min_size: Some(size(px(900.), px(600.))),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| ui::HwlensView::new(requests, services, page, rx, window, cx)),
        )
        .expect("open the window");
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
