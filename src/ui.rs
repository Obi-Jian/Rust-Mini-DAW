use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use egui_file_dialog::FileDialog;
use crate::audio::{AudioEngine, WavData};
use eframe::egui;
use cpal::traits::StreamTrait;
use std::path::PathBuf;

pub struct MyDawApp {
    engine: AudioEngine,
    stream: Option<cpal::Stream>,
    is_playing: bool,
    is_paused: bool,
    track_muted: Vec<Arc<AtomicBool>>,
    file_dialog: FileDialog,
    track_files: Vec<PathBuf>,
}

impl MyDawApp {
    pub fn new() -> Self {
        Self {
            engine: AudioEngine::new(), // Inizializza host/device
            stream: None,
            is_playing: false,
            is_paused: false,
            track_muted: Vec::new(),
            file_dialog: FileDialog::new(),
            track_files: Vec::new(),
        }
    }
}

impl eframe::App for MyDawApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Controllo Audio");

            if ui.button("Pick file").clicked() {
                // Open the file dialog to pick a file.
                // self.file_dialog.select_file();
                self.file_dialog.select_multiple();
            }

            if self.track_files.is_empty() {
                ui.label("Nessun file caricato");
            } else {
                for (i, path) in self.track_files.iter().enumerate() {
                    ui.label(format!("Traccia {}: {}", i + 1, path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()));
                }
            }

            // Update the dialog
            self.file_dialog.update(ctx);

            // Check if the user picked a file.
            if let Some(paths) = self.file_dialog.take_selected_multiple() {
                self.track_files.extend(paths); // take_selected_multiple() restituisce un Option<Vec<PathBuf>>> che, dopo l'unwrap dall'option, è esattamente uguale al nostro track_files, quindi usiamo il risultato per estendere la nostra lista.
            }

            if ui.button(if self.is_playing && !self.is_paused { "Pause" } else { "Play" }).clicked() {
                if !self.is_playing && !self.is_paused{
                    // Carichiamo i file
                    let data: Vec<WavData> = self.track_files
                        .iter()
                        .map(|path| AudioEngine::load_wav(path.to_str().unwrap()))
                        .collect();


                    // crea un AtomicBool per ogni traccia, tutti a false (non mutati)
                    // questo l'ha fatto claude ma ha senso e funziona
                    let muted: Vec<Arc<AtomicBool>> = (0..data.len())
                        .map(|_| Arc::new(AtomicBool::new(false)))
                        .collect();

                    // clona per passarli allo stream (la UI tiene gli originali)
                    let muted_for_stream: Vec<Arc<AtomicBool>> = muted.iter().map(|m| Arc::clone(m)).collect();

                    
                    // Creiamo lo stream usando la logica che hai già scritto
                    if let Ok(s) = self.engine.setup_stream(data, muted_for_stream) {
                        use cpal::traits::StreamTrait;
                        s.play().unwrap();
                        self.stream = Some(s); // Salviamo lo stream per non farlo morire
                        self.track_muted = muted;  // la UI tiene i suoi Arc
                        self.is_playing = true;
                        self.is_paused = false;
                        ctx.request_repaint();  // ← forza un nuovo frame, non so cosa cambia ma funziona anche senza
                    } else {println!("Errore nel caricamento dello stream")}
                } else if self.is_playing && !self.is_paused {
                    self.is_playing = false;
                    if let Some(s) = &self.stream {
                        s.pause().unwrap();
                        self.is_paused = true;
                        self.is_playing = false;
                    }  
                } else {
                    if let Some(s) = &self.stream {
                        s.play().unwrap();
                        self.is_paused = false;
                        self.is_playing = true;
                    }
                }
            }
            if ui.button("Stop").clicked() {
                    self.stream = None; // Fermiamo lo stream distruggendolo
                    self.is_playing = false;
                    self.is_paused = false;
            }
            for (i, muted) in self.track_muted.iter().enumerate() {
                let is_muted = muted.load(Ordering::Relaxed);
                let label = format!("Traccia {} — {}", i + 1, if is_muted { "Resume" } else { "Mute" });
                if ui.button(label).clicked() {
                    muted.store(!is_muted, Ordering::Relaxed);
                }
            }

            let mut to_remove = None;

            for (i, _track) in self.track_files.iter().enumerate() {
                let label = format!("Rimuovi traccia {}", i + 1);
                if ui.button(label).clicked() {
                    to_remove = Some(i);
                }
            }

            if let Some(i) = to_remove {
                self.track_files.remove(i);
            }
        });
    }
    
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}
        
    fn auto_save_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(30)
    }
    
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // NOTE: a bright gray makes the shadows of the windows look weird.
        // We use a bit of transparency so that if the user switches on the
        // `transparent()` option they get immediate results.
        egui::Color32::from_rgba_unmultiplied(12, 12, 12, 180).to_normalized_gamma_f32()
    
        // _visuals.window_fill() would also be a natural choice
    }
    
    fn persist_egui_memory(&self) -> bool {
        true
    }
    
    fn raw_input_hook(&mut self, _ctx: &egui::Context, _raw_input: &mut egui::RawInput) {}
    
    /* fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Controllo Audio");


            if ui.button("Pick file").clicked() {
                // Open the file dialog to pick a file.
                self.file_dialog.pick_file();
            }

            ui.label(format!("Picked file: {:?}", self.track_files));

            // Update the dialog
            self.file_dialog.update(ui);

            // Check if the user picked a file.
            if let Some(path) = self.file_dialog.take_picked() {
                self.track_files = Some(path.to_path_buf());
            }


            if ui.button(if self.is_playing && !self.is_paused { "Pause" } else { "Play" }).clicked() {
                if !self.is_playing && !self.is_paused{
                    // Carichiamo i file
                    let data_a = AudioEngine::load_wav("gong.wav");
                    let data_b = AudioEngine::load_wav("laughter.wav");
                    let data_c = AudioEngine::load_wav("handel.wav");
                    let data = vec![data_a, data_b, data_c];


                    // crea un AtomicBool per ogni traccia, tutti a false (non mutati)
                    let muted: Vec<Arc<AtomicBool>> = (0..data.len())
                        .map(|_| Arc::new(AtomicBool::new(false)))
                        .collect();

                    // clona per passarli allo stream (la UI tiene gli originali)
                    let muted_for_stream: Vec<Arc<AtomicBool>> = muted.iter().map(|m| Arc::clone(m)).collect();

                    
                    // Creiamo lo stream usando la logica che hai già scritto
                    if let Ok(s) = self.engine.setup_stream(data, muted_for_stream) {
                        use cpal::traits::StreamTrait;
                        s.play().unwrap();
                        self.stream = Some(s); // Salviamo lo stream per non farlo morire
                        self.track_muted = muted;  // la UI tiene i suoi Arc
                        self.is_playing = true;
                        self.is_paused = false;
                    } else {println!("Errore nel caricamento dello stream")}
                } else if self.is_playing && !self.is_paused {
                    self.is_playing = false;
                    if let Some(s) = &self.stream {
                        s.pause().unwrap();
                        self.is_paused = true;
                        self.is_playing = false;
                    }  
                } else {
                    if let Some(s) = &self.stream {
                        s.play().unwrap();
                        self.is_paused = false;
                        self.is_playing = true;
                    }
                }
            }
            if ui.button("Stop").clicked() {
                    self.stream = None; // Fermiamo lo stream distruggendolo
                    self.is_playing = false;
                    self.is_paused = false;
            }
            for (i, muted) in self.track_muted.iter().enumerate() {
                let is_muted = muted.load(Ordering::Relaxed);
                let label = format!("Traccia {} — {}", i + 1, if is_muted { "Resume" } else { "Mute" });
                if ui.button(label).clicked() {
                    muted.store(!is_muted, Ordering::Relaxed);
                }
            }
        });
    } */
}