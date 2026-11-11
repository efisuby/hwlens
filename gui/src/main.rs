//! hwlens-gui: a desktop client of hwlensd built with GPUI.

mod client;
mod model;
mod patch;
mod ui;

use std::path::PathBuf;

use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions, prelude::*, px, size,
};

fn socket_path() -> PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--socket" | "-s" => {
                if let Some(p) = args.next() {
                    return p.into();
                }
            }
            "--help" | "-h" => {
                println!("usage: hwlens-gui [--socket PATH]\n\nDefault socket: $HWLENSD_SOCKET or {}", client::DEFAULT_SOCKET);
                std::process::exit(0);
            }
            other => {
                eprintln!("hwlens-gui: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    std::env::var_os("HWLENSD_SOCKET").map(PathBuf::from).unwrap_or_else(|| client::DEFAULT_SOCKET.into())
}

fn main() {
    let socket = socket_path();
    Application::new().run(move |cx: &mut App| {
        let (tx, rx) = futures::channel::mpsc::unbounded();
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
            |window, cx| cx.new(|cx| ui::HwlensView::new(requests, rx, window, cx)),
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
