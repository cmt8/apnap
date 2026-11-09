//! apnap desktop app (eframe/egui, spec §11).

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("apnap")
            .with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "apnap",
        options,
        Box::new(|_cc| Ok(Box::new(app::ui::App::default()))),
    )
}
