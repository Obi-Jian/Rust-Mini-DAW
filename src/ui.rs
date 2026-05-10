use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering}};
use egui_file_dialog::FileDialog;
use fundsp::{prelude::{AudioUnit, bandpass_hz, highpass_hz, lowpass_hz, notch_hz}};
use crate::audio::{AudioEngine, Filter, Track};
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
    track_filters: Vec<Vec<Filter>>,
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
            /* track_filter_enabled: Vec::new(),
            track_filter_cutoff: Vec::new(),
            track_filter: Vec::new(), */
            track_filters: Vec::new(),
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

            if let Some(paths) = self.file_dialog.take_selected_multiple() {
                self.track_files.extend(paths);
                // sincronizza track_filters con track_files
                while self.track_filters.len() < self.track_files.len() {
                    self.track_filters.push(Vec::new());
                }
                self.recalculate_playback_len();
            }

            ui.horizontal(|ui| {

                if ui.button(if self.is_playing && !self.is_paused { "Pause" } else { "Play" }).clicked() {
                        if self.track_files.is_empty() {
                            return; // niente da fare
                        }
                    if !self.is_playing && !self.is_paused{
                        // Carichiamo i file
                        let tracks: Vec<Track> = self.track_files
                            .iter()
                            .enumerate()
                            .map(|(idx, path)| {
                                // recuperiamo il tipo di filtro e il suo cutoff da track_filter_enabled / track_filter_cutoff, per ogni track
                                // questo perchè dopo che una traccia termina, vogliamo mantenere i suoi filtri
                                
                                // per ogni traccia (idx) stiamo andando a controllare il vettore corrispondente in track_filters,
                                // per vedere se esistono già filtri (serve perchè quando una traccia termina, per esempio, si interrompe e va ricreata,
                                // in questo modo si caricano i filtri che avevamo salvato, nella "nuova" traccia, ossia la stessa ricaricata).
                                let existing_filters: Vec<Filter> = self.track_filters
                                    .get(idx)
                                    .map(|filters| filters.iter().map(|f| {
                                        let enabled = f.filter_enabled.load(Ordering::Relaxed); // ordering semplicemente indica come si accede a filter_enabled in modo concorrente
                                        let cutoff = f32::from_bits(f.filter_cutoff.load(Ordering::Relaxed));
                                        let new_node: Option<Box<dyn AudioUnit + Send>> = match enabled {
                                            1 => Some(Box::new(lowpass_hz(cutoff, 0.7))),
                                            2 => Some(Box::new(highpass_hz(cutoff, 0.7))),
                                            3 => Some(Box::new(bandpass_hz(cutoff, 0.7))),
                                            4 => Some(Box::new(notch_hz(cutoff, 0.7))),
                                            _ => None,
                                        };
                                        Filter {
                                            filter_enabled: Arc::clone(&f.filter_enabled),
                                            filter_cutoff: Arc::clone(&f.filter_cutoff),
                                            filter: Arc::new(Mutex::new(new_node)),
                                        }
                                    }).collect())
                                    .unwrap_or_default();
                                    // Quando non ci sono ancora filtri, self.track_filters.get(idx) restituisce None —
                                    // perché o track_filters è vuoto, o non ha ancora una entry per quell'indice.
                                    // .map(...) su None non esegue nulla e restituisce None. Poi .unwrap_or_default() su None
                                    // restituisce il valore di default del tipo —
                                    // e il default di Vec<Filter> è semplicemente Vec::new(), un vettore vuoto.

                                // uguale per volume, così rimane quando riparte la traccia quando premiamo play
                                let volume = self.track_volume_atomics
                                    .get(idx)
                                    .map(|vol| vol.load(Ordering::Relaxed)/* AtomicU32::new(vol.to_bits()) */)
                                    .unwrap_or(1.0f32.to_bits());

                                // let filter = lowpass_hz(cutoff, 0.7);                      
                                Track {
                                    data: AudioEngine::load_wav(path.to_str().unwrap()),
                                    // crea un AtomicBool per ogni traccia, tutti a false (non mutati)
                                    muted: Arc::new(AtomicBool::new(false)),
                                    volume: Arc::new(AtomicU32::new(volume)),
                                    /* filter_enabled: Arc::new(AtomicU8::new(filter_enabled)),
                                    filter_cutoff: Arc::new(AtomicU32::new(filter_cutoff)),
                                    filter: Arc::new(Mutex::new(filter)), */
                                    filters: existing_filters,
                                }
                            })
                            .collect();

                        // popoliamo track filters con i filtri delle tracce che abbiamo appena creato (appena inserito il file)
                        // track_filters è un vettore di, appunto, filtri in cui abbiamo puntatori ai valori dei filtri di ogni traccia.
                        // ci servono perchè egui non può agire direttamente sui puntatori delle tracce, ma ha bisogno di altre variabili (clonate, quindi puntano agli stessi valori)
                        self.track_filters = tracks
                            .iter()
                            .map(|t| t.filters.iter().map(|f| Filter {
                                filter_enabled: Arc::clone(&f.filter_enabled),
                                filter_cutoff: Arc::clone(&f.filter_cutoff),
                                filter: Arc::clone(&f.filter),
                            }).collect())
                            .collect();

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
                        
                        // Dopo la creazione dei Track — sincronizza lo slider della UI
                        // 1. prima calcola track_volumes dai vecchi atomici
                        self.track_volumes = (0..tracks.len())
                            .map(|i| {
                                self.track_volume_atomics
                                    .get(i)
                                    .map(|a| f32::from_bits(a.load(Ordering::Relaxed)))
                                    .unwrap_or(1.0)
                            })
                            .collect();

                        // 2. poi sovrascrive gli atomici con i nuovi Arc
                        self.track_volume_atomics = tracks.iter().map(|t| Arc::clone(&t.volume)).collect();
                        // self.track_volumes = vec![1.0f32; tracks.len()];

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
                if ui.button("Stop").clicked() {
                        self.stream = None; // Fermiamo lo stream distruggendolo
                        self.is_playing = false;
                        self.is_paused = false;
                        self.playback_pos.store(0 as u64, Ordering::Relaxed);
                }
            });
            if self.playback_len > 0 {
                let current = self.playback_pos.load(Ordering::Relaxed); // * 10;
                if current >= self.playback_len {
                    self.stream = None;
                    self.is_playing = false;
                    self.is_paused = false;
                    self.playback_pos.store(0 as u64, Ordering::Relaxed);
                }
            }
            
            ui.horizontal(|ui| {

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

                        // ferma lo stream se sta suonando
                        self.stream = None;
                        self.is_playing = false;
                        self.is_paused = false;
                        self.playback_pos.store(0, Ordering::Relaxed);
                        // ui.add_space(10.0);      // spazio vuoto in pixel

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
                        if i < self.track_filters.len() {  // ← aggiunto
                            self.track_filters.remove(i);
                        }
                    self.recalculate_playback_len();    // ricalcola DOPO la rimozione
                }

            });
            

            // bottoni NON ai generated (lo erano, ho cambiato tutto, non funzionavano più e li ho rifatti a mano)
            for i in 0..self.track_files.len() {
                if i >= self.track_volumes.len() { continue; }

                if ui.button("Add filter").clicked() {

                    /* if let Some(paths) = self.file_dialog.take_selected_multiple() {
                        self.track_files.extend(paths);
                        // aggiungi un Vec<Filter> vuoto per ogni nuova traccia
                        for _ in 0..self.track_files.len() - self.track_filters.len() {
                            self.track_filters.push(Vec::new());
                        }
                        self.recalculate_playback_len();
                    } */


                    if let Some(filters) = self.track_filters.get_mut(i) {
                        filters.push(Filter {
                            filter_enabled: Arc::new(AtomicU8::new(0)),
                            filter_cutoff: Arc::new(AtomicU32::new(200.0f32.to_bits())),
                            filter: Arc::new(Mutex::new(None)),
                        });
                    }
                }

                /* // Il bottone precedente l'avevo creato così, ma il metodo precedente fatto dall'AI è più efficiente
                if ui.button("Add filter").clicked() {
                    self.track_filters
                        .get_mut(i) // IMPORTANTE get_mut e non get, altrimenti sarebbe impossibile pushare a filters
                        .map(move |filters| {
                            let enabled = Arc::<AtomicU8>::new(0.into());
                            let cutoff = Arc::<AtomicU32>::new(200.into());
                            /* let new_node: Option<Box<dyn AudioUnit + Send>> = match enabled {
                                1 => Some(Box::new(lowpass_hz(cutoff, 0.7))),
                                2 => Some(Box::new(highpass_hz(cutoff, 0.7))),
                                3 => Some(Box::new(bandpass_hz(cutoff, 0.7))),
                                4 => Some(Box::new(notch_hz(cutoff, 0.7))),
                                _ => None,
                            }; */
                            let filtro = Filter {
                                filter_enabled: enabled,
                                filter_cutoff: Arc::clone(&cutoff),
                                filter: Arc::new(Mutex::new(None)),
                            };
                            filters.push(filtro);
                        });
                }*/

                // slider volume esistente
                let mut v = self.track_volumes[i];
                if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).text("Volume")).changed() {
                    self.track_volumes[i] = v;
                    self.track_volume_atomics[i].store(v.to_bits(), Ordering::Relaxed);
                }

                self.track_filters
                    .get(i)
                    .map(|a| {
                        for filter in a {
                            let current_filter = filter.filter_enabled.load(Ordering::Relaxed);

                            ui.horizontal(|ui| {

                                for (label, value) in [("Lowpass", 1u8), ("Highpass", 2), ("Bandpass", 3), ("Notch", 4)] {
                                    // if i >= filter.filter_enabled { continue; }

                                    let mut selected = current_filter == value;
                                        if ui.checkbox(&mut selected, label).changed() {
                                            if selected {
                                                // attiva questo filtro e ricrea il nodo fundsp
                                                filter.filter_enabled.store(value, Ordering::Relaxed);
                                                let cutoff = f32::from_bits(filter.filter_cutoff.load(Ordering::Relaxed));
                                                *filter.filter.lock().unwrap() = Some(match value {
                                                    1 => Box::new(lowpass_hz(cutoff, 0.7)),
                                                    2 => Box::new(highpass_hz(cutoff, 0.7)),
                                                    3 => Box::new(bandpass_hz(cutoff, 0.7)),
                                                    4 => Box::new(notch_hz(cutoff, 0.7)),
                                                    _ => unreachable!(),
                                                });
                                            } else {
                                                eprintln!("filtro disattivato per traccia {}, current={}", i, current_filter);
                                                // deselezioni quello attivo → nessun filtro
                                                filter.filter_enabled.store(0, Ordering::Relaxed);
                                            }
                                        }
                                }
                            });

                            // slider cutoff
                            if current_filter != 0 {
                                let mut cutoff = f32::from_bits(filter.filter_cutoff.load(Ordering::Relaxed));
                                if ui.add(egui::Slider::new(&mut cutoff, 200.0..=8000.0).text("Cutoff Hz")).changed() {
                                    filter.filter_cutoff.store(cutoff.to_bits(), Ordering::Relaxed);
                                    *filter.filter.lock().unwrap() = Some(match current_filter {
                                        1 => Box::new(lowpass_hz(cutoff, 0.7)),
                                        2 => Box::new(highpass_hz(cutoff, 0.7)),
                                        3 => Box::new(bandpass_hz(cutoff, 0.7)),
                                        4 => Box::new(notch_hz(cutoff, 0.7)),
                                        _ => unreachable!(),
                                    });
                                }
                            }
                        }
                        a
                    });                
            }

            // PROGRESS BAR
            // AI GENERATED, onestamente frega una sega
            // converti la posizione in secondi
            ui.add_space(10.0);      // spazio vuoto in pixel
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