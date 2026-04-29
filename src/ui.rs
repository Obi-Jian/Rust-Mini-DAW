use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering}};
use egui_file_dialog::FileDialog;
use fundsp::prelude::{AudioUnit, lowpass_hz};
use crate::audio::{AudioEngine, Track};
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
    track_volumes: Vec<f32>,
    track_volume_atomics: Vec<Arc<AtomicU32>>,
    playback_pos: Arc<AtomicU64>,
    playback_len: u64,
    track_filter_enabled: Vec<Arc<AtomicBool>>,
    track_filter_cutoff: Vec<Arc<AtomicU32>>,
    track_filter: Vec<Arc<Mutex<Box<dyn AudioUnit + Send>>>>,
}

impl MyDawApp {
    pub fn new() -> Self {
        Self {
            engine: AudioEngine::new(), // Inizializza host/device
            stream: None,
            is_playing: false,
            is_paused: false,
            track_muted: Vec::new(),
            track_volumes: Vec::new(),
            track_volume_atomics: Vec::new(),
            file_dialog: FileDialog::new(),
            track_files: Vec::new(),
            playback_pos: Arc::<AtomicU64>::new(0.into()),
            playback_len: 0,
            track_filter_enabled: Vec::new(),
            track_filter_cutoff: Vec::new(),
            track_filter: Vec::new(),
        }
    }
    fn recalculate_playback_len(&mut self) {
    let device_rate = self.engine.config.sample_rate as f64;
    self.playback_len = self.track_files.iter()
        .map(|path| {
            let data = AudioEngine::load_wav(path.to_str().unwrap());
            let ratio = (data.spec.sample_rate as f64 / device_rate) * data.spec.channels as f64;
            (data.samples.len() as f64 / ratio) as u64
        })
        .max()
        .unwrap_or(0);
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
                self.recalculate_playback_len();
            }

            if ui.button(if self.is_playing && !self.is_paused { "Pause" } else { "Play" }).clicked() {
                    if self.track_files.is_empty() {
                        return; // niente da fare
                    }
                if !self.is_playing && !self.is_paused{
                    // Carichiamo i file
                let tracks: Vec<Track> = self.track_files
                    .iter()
                    .map(|path| {
                        let cutoff = 1000.0f32;
                        let filter = lowpass_hz(cutoff, 0.7);                      
                        Track {
                            data: AudioEngine::load_wav(path.to_str().unwrap()),
                            // crea un AtomicBool per ogni traccia, tutti a false (non mutati)
                            muted: Arc::new(AtomicBool::new(false)),
                            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
                            filter_enabled: Arc::new(AtomicBool::new(false)),
                            filter_cutoff: Arc::new(AtomicU32::new(3)),
                            filter: Arc::new(Mutex::new(Box::new(filter))),
                        }
                    })
                    .collect();

                self.track_filter_enabled = tracks.iter().map(|t| Arc::clone(&t.filter_enabled)).collect();
                self.track_filter_cutoff = tracks.iter().map(|t| Arc::clone(&t.filter_cutoff)).collect();
                self.track_filter = tracks.iter().map(|t| Arc::clone(&t.filter)).collect();  
                

                    let device_rate = self.engine.config.sample_rate as f64;
                    self.playback_len = tracks.iter()
                        .map(|d| {
                            let ratio = (d.data.spec.sample_rate as f64 / device_rate) * d.data.spec.channels as f64;
                            (d.data.samples.len() as f64 / ratio) as u64
                        })
                        .max()
                        .unwrap_or(0);
                    self.playback_pos.store(0 as u64, Ordering::Relaxed);

                    self.track_muted = tracks.iter().map(|t| Arc::clone(&t.muted)).collect();
                    self.track_volume_atomics = tracks.iter().map(|t| Arc::clone(&t.volume)).collect();
                    self.track_volumes = vec![1.0f32; tracks.len()];

                    let position_for_stream = Arc::clone(&self.playback_pos);                    
                    // Creiamo lo stream usando la logica che hai già scritto

                    if let Ok(s) = self.engine.setup_stream(tracks, position_for_stream) {
                        use cpal::traits::StreamTrait;
                        s.play().unwrap();
                        self.stream = Some(s); // Salviamo lo stream per non farlo morire
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
            if self.playback_len > 0 {
                let current = self.playback_pos.load(Ordering::Relaxed); // * 10;
                if current >= self.playback_len {
                    self.stream = None;
                    self.is_playing = false;
                    self.is_paused = false;
                    self.playback_pos.store(0 as u64, Ordering::Relaxed);
                }
            }
            if ui.button("Stop").clicked() {
                    self.stream = None; // Fermiamo lo stream distruggendolo
                    self.is_playing = false;
                    self.is_paused = false;
                    self.playback_pos.store(0 as u64, Ordering::Relaxed);
            }

            // bottoni per applicare un filtro a una traccia specifica
            for (i, muted) in self.track_muted.iter().enumerate() {
                let is_muted = muted.load(Ordering::Relaxed);
                let label = format!("Traccia {} — {}", i + 1, if is_muted { "Resume" } else { "Mute" });
                if ui.button(label).clicked() {
                    muted.store(!is_muted, Ordering::Relaxed);
                }
            }
            // non possiamo modificare un elemento di un vettore mentre lo stiamo iterando
            // ma possiamo segnarci cosa vogliamo eliminare alla fine dell'iterazione
            // qui inizializiamo la variabile dove ci segneremo il valore in track_files da eliminare
            let mut to_remove = None;

            for (i, _track) in self.track_files.iter().enumerate() {
                let label = format!("Rimuovi traccia {}", i + 1);
                if ui.button(label).clicked() {
                    to_remove = Some(i); // qui salviamo ciò che vogliamo eliminare -> vedi riga 193 per eliminazione
                }
            }

            // bottoni ai generated
            for i in 0..self.track_files.len() {
                if i >= self.track_volumes.len() { continue; }

                // slider volume esistente
                let mut v = self.track_volumes[i];
                if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).text("Volume")).changed() {
                    self.track_volumes[i] = v;
                    self.track_volume_atomics[i].store(v.to_bits(), Ordering::Relaxed);
                }

                // toggle filtro
                let mut enabled = self.track_filter_enabled[i].load(Ordering::Relaxed);
                if ui.checkbox(&mut enabled, "Lowpass").changed() {
                    self.track_filter_enabled[i].store(enabled, Ordering::Relaxed);
                }

                // slider cutoff
                if enabled {
                    let mut cutoff = f32::from_bits(self.track_filter_cutoff[i].load(Ordering::Relaxed));
                    if ui.add(egui::Slider::new(&mut cutoff, 200.0..=8000.0).text("Cutoff Hz")).changed() {
                        self.track_filter_cutoff[i].store(cutoff.to_bits(), Ordering::Relaxed);
                        // ricrea il filtro con il nuovo cutoff
                        *self.track_filter[i].lock().unwrap() = Box::new(lowpass_hz(cutoff, 0.7));
                    }
                }
            }
            
            // alla fine delle varie iterazioni, procediamo a eliminare ciò che ci siamo segnati
            if let Some(i) = to_remove {
                self.track_files.remove(i); // prima rimuovi il file

                if i < self.track_muted.len() { // poi rimuove tasto mute della traccia appena eliminata
                    self.track_muted.remove(i);
                }
                if i < self.track_volumes.len() { // infine rimuove i parametri di volume associati
                    self.track_volumes.remove(i);
                    self.track_volume_atomics.remove(i);
                }

                self.recalculate_playback_len();    // ricalcola DOPO la rimozione
            }

            // PROGRESS BAR
            // AI GENERATED, onestamente frega una sega
            // converti la posizione in secondi
            let device_rate = self.engine.config.sample_rate as f64;
            let current = self.playback_pos.load(Ordering::Relaxed);
            let progress = if self.playback_len > 0 {
                current as f32 / self.playback_len as f32
            } else {
                0.0
            };
            let current_secs = current as f64 / device_rate;
            let total_secs = self.playback_len as f64 / device_rate;

            ui.add(
                egui::ProgressBar::new(progress)
                    .text(format!("{:.1} / {:.1} sec", current_secs, total_secs))
            );        
            if self.is_playing && !self.is_paused {
                ctx.request_repaint_after(std::time::Duration::from_millis(16)); // ~60fps
            }
        });
    }
}