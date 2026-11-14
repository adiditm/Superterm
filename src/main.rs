// Windows Subsystem: hide black console window in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod ui;

use eframe::egui;
use ui::app::{configure_superterm_theme, SupertermNativeApp};

fn create_app_icon() -> egui::IconData {
    let size = 32usize;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) * 4;
            let dx = (x as f32) - 15.5;
            let dy = (y as f32) - 15.5;
            let in_rounded_box = dx.abs() <= 14.0 && dy.abs() <= 14.0 && (dx.abs() + dy.abs() <= 25.0);
            if in_rounded_box {
                let t = (x + y) as f32 / 64.0;
                rgba[idx] = (124.0 * (1.0 - t) + 79.0 * t) as u8;
                rgba[idx + 1] = (58.0 * (1.0 - t) + 70.0 * t) as u8;
                rgba[idx + 2] = (237.0 * (1.0 - t) + 229.0 * t) as u8;
                rgba[idx + 3] = 255;

                let is_chevron = x >= 8
                    && x <= 14
                    && ((y as isize - 12).abs() == (x as isize - 8)
                        || (y as isize - 18).abs() == (14 - x as isize));
                let is_underscore = y >= 19 && y <= 21 && x >= 16 && x <= 23;
                if is_chevron || is_underscore {
                    rgba[idx] = 255;
                    rgba[idx + 1] = 255;
                    rgba[idx + 2] = 255;
                }
            }
        }
    }
    egui::IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}

fn main() -> eframe::Result<()> {
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to initialize Tokio runtime");
    let rt_handle = tokio_runtime.handle().clone();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Superterm — Pure Native Rust SSH & Tunneling Vault")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_icon(std::sync::Arc::new(create_app_icon()))
            .with_active(true)
            .with_decorations(true),
        ..Default::default()
    };

    eframe::run_native(
        "Superterm",
        native_options,
        Box::new(move |cc| {
            configure_superterm_theme(&cc.egui_ctx, true);
            Ok(Box::new(SupertermNativeApp::new(rt_handle)))
        }),
    )
}
