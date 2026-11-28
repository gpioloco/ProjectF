#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod apply;
mod browser;
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
mod config;
mod design;
#[cfg(windows)]
mod desktop;
mod exovoid;
#[cfg(any(windows, test))]
mod nvim;
mod palette;
mod restore;
#[cfg(windows)]
mod rillshift;
mod store;
mod terminal;
mod theme;
mod ui;
mod wallpaper;

fn archive(files: &[(&str, &[u8])]) -> anyhow::Result<Vec<u8>> {
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for &(name, data) in files {
        zip.start_file(name, options)?;
        zip.write_all(data)?;
    }
    Ok(zip.finish()?.into_inner())
}

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
            .with_min_inner_size([480.0, 360.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                    .expect("embedded icon"),
            ),
        ..Default::default()
    };
    let result = eframe::run_native(
        "WTE · Window Theme Engine",
        options,
        Box::new(|cc| {
            let store = store::Store::open()?;
            let library = store.load()?;
            design::setup(&cc.egui_ctx);
            let picker = rfd::FileDialog::new()
                .set_parent(cc)
                .set_title("Choose a wallpaper")
                .add_filter("Pictures (JPEG, PNG)", &["jpg", "jpeg", "png"]);
            Ok(Box::new(ui::App::new(store, library, picker)))
        }),
    );
    if let Err(e) = &result {
        rfd::MessageDialog::new()
            .set_title("Window Theme Engine")
            .set_description(e.to_string())
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
    result
}
