mod tiny_daw;
mod ui;
use ui::MyDawApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1080.0, 720.0])   // larghezza x altezza
            .with_min_inner_size([400.0, 300.0]), // minimo ridimensionabile
        ..Default::default()
    };

    eframe::run_native(
        "Rust Mini DAW",
        native_options,
        // Nota: aggiungiamo Ok(...) e la closure ora accetta _cc
        Box::new(|_cc| {
            Ok(Box::new(MyDawApp::new()))
        }),
    )
}