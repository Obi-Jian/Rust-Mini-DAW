mod audio;
mod ui;
use ui::MyDawApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        // Definiamo una dimensione iniziale per evitare problemi di rendering
        viewport: egui::ViewportBuilder::default().with_inner_size([400.0, 300.0]),
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