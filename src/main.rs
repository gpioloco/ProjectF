#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod apply;
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
mod config;
mod design;
mod exovoid;
mod palette;
mod store;
mod theme;
mod ui;
mod wallpaper;

fn unique_id() -> String {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{time:x}-{:x}", std::process::id())
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 737.0])
            .with_min_inner_size([775.0, 540.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                    .expect("embedded icon"),
            ),
        ..Default::default()
    };
    let result = eframe::run_native(
        "WTE · Windows Theme Engine",
        options,
        Box::new(|cc| {
            design::setup(&cc.egui_ctx);
            let store = store::Store::open()?;
            let library = store.load()?;
            Ok(Box::new(ui::App::new(store, library)))
        }),
    );
    if let Err(e) = &result {
        rfd::MessageDialog::new()
            .set_title("Windows Theme Engine")
            .set_description(e.to_string())
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
    result
}
