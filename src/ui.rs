use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::StreamTrait;
use eframe::egui::{self, ScrollArea};
use egui_dnd::dnd;
use egui_file_dialog::FileDialog;
use fundsp::prelude::{bandpass_hz, highpass_hz, lowpass_hz, notch_hz, AudioUnit};
use fundsp::prelude32::{sine_hz, square_hz, triangle_hz};

use crate::audio::{AudioEngine, DrumTrack, Filter, Note, Source, SynthTrack, Track, WavClip};

enum ClipAction {
    MoveUp { from_lane: usize, clip_idx: usize },
    MoveDown { from_lane: usize, clip_idx: usize },
    MoveToNewLane { from_lane: usize, clip_idx: usize },
    Delete { from_lane: usize, clip_idx: usize },
}
fn create_filter_node(filter_type: u8, cutoff: f32) -> Option<Box<dyn AudioUnit + Send>> {
    match filter_type {
        1 => Some(Box::new(lowpass_hz(cutoff, 0.7))),
        2 => Some(Box::new(highpass_hz(cutoff, 0.7))),
        3 => Some(Box::new(bandpass_hz(cutoff, 0.7))),
        4 => Some(Box::new(notch_hz(cutoff, 0.7))),
        _ => None,
    }
}

pub struct MyDawApp {
    kick_samples: Vec<(String, PathBuf)>,   // (nome display, path)
    snare_samples: Vec<(String, PathBuf)>,
    hihat_samples: Vec<(String, PathBuf)>,
    file_dialog_kick: FileDialog,
    file_dialog_snare: FileDialog,
    file_dialog_hihat: FileDialog,
    engine: AudioEngine,
    stream: Option<cpal::Stream>,
    is_playing: bool,
    is_paused: bool,
    lanes: Vec<Vec<WavClip>>,
    next_clip_id: u64,
    file_dialog: FileDialog,
    //file_dialog_drums: FileDialog,
    playback_pos: Arc<AtomicU64>,
    playback_len: u64,
    synth_tracks: Vec<SynthTrack>,
    drum_tracks: Vec<DrumTrack>,
    bpm: f32,
    selected_lane: usize,
}

impl MyDawApp {
    pub fn new() -> Self {
        Self {
            kick_samples: Vec::new(),
            snare_samples: Vec::new(),
            hihat_samples: Vec::new(),
            file_dialog_kick: FileDialog::new(),
            file_dialog_snare: FileDialog::new(),
            file_dialog_hihat: FileDialog::new(),
            engine: AudioEngine::new(),
            stream: None,
            is_playing: false,
            is_paused: false,
            lanes: vec![Vec::new()], // Inizia con una corsia vuota (Corsia 1)
            next_clip_id: 1,
            file_dialog: FileDialog::new(),
            //file_dialog_drums: FileDialog::new(),
            playback_pos: Arc::<AtomicU64>::new(0.into()),
            playback_len: 0,
            synth_tracks: Vec::new(),
            drum_tracks: Vec::new(),
            bpm: 120.0,
            selected_lane: 0,
        }
    }

    fn recalculate_playback_len(&mut self) {
        let mut max_len = 0;
        for lane in &self.lanes {
            let lane_len: u64 = lane.iter().map(|c: &WavClip| c.duration_frames_device).sum();
            if lane_len > max_len {
                max_len = lane_len;
            }
        }
        self.playback_len = max_len;
    }

    fn has_audio_content(&self) -> bool {
        let has_clips = self.lanes.iter().any(|l: &Vec<WavClip>| !l.is_empty());
        has_clips || !self.synth_tracks.is_empty() || !self.drum_tracks.is_empty()
    }
}

impl eframe::App for MyDawApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Controllo Audio");

            // Barra dei controlli principali (Play / Pause / Stop)
            ui.horizontal(|ui| {
                let play_text = if self.is_playing && !self.is_paused {
                    "⏸ Pause"
                } else {
                    "▶ Play"
                };

                if ui.button(play_text).clicked() {
                    if !self.has_audio_content() {
                        return;
                    }

                    if !self.is_playing && !self.is_paused {
                        // Creazione tracce dalle corsie
                        let mut tracks: Vec<Track> = Vec::new();
                        for (lane_idx, lane) in self.lanes.iter().enumerate() {
                            let mut lane_start_frame: u64 = 0;
                            for clip in lane {
                                let existing_filters: Vec<Filter> = clip
                                    .filters
                                    .iter()
                                    .map(|f| {
                                        let enabled = f.filter_enabled.load(Ordering::Relaxed);
                                        let cutoff =
                                            f32::from_bits(f.filter_cutoff.load(Ordering::Relaxed));
                                        let mut guard = f.filter.lock().unwrap();
                                        if guard.is_none() && enabled != 0 {
                                            *guard = create_filter_node(enabled, cutoff);
                                        }
                                        Filter {
                                            filter_enabled: Arc::clone(&f.filter_enabled),
                                            filter_cutoff: Arc::clone(&f.filter_cutoff),
                                            filter: Arc::clone(&f.filter),
                                        }
                                    })
                                    .collect();

                                tracks.push(Track {
                                    data: Arc::clone(&clip.data),
                                    muted: Arc::clone(&clip.muted),
                                    volume: Arc::clone(&clip.volume_atomic),
                                    filters: existing_filters,
                                    start_frame: lane_start_frame,
                                    lane: lane_idx,
                                });

                                // In sequenza nella stessa corsia
                                lane_start_frame += clip.duration_frames_device;
                            }
                        }

                        self.recalculate_playback_len();
                        self.playback_pos.store(0 as u64, Ordering::Relaxed);

                        let synths: Vec<SynthTrack> =
                            self.synth_tracks.iter().map(|t| t.to_stream()).collect();
                        let drums: Vec<DrumTrack> =
                            self.drum_tracks.iter().map(|t| t.to_stream()).collect();

                        let source = Source {
                            tracks,
                            synth_tracks: synths,
                            drum_tracks: drums,
                        };

                        let position_for_stream = Arc::clone(&self.playback_pos);
                        if let Ok(s) = self.engine.setup_stream(source, position_for_stream) {
                            let _ = s.play();
                            self.stream = Some(s);
                            self.is_playing = true;
                            self.is_paused = false;
                            ctx.request_repaint();
                        } else {
                            eprintln!("Errore nel caricamento dello stream");
                        }
                    } else if self.is_playing && !self.is_paused {
                        self.is_playing = false;
                        if let Some(s) = &self.stream {
                            let _ = s.pause();
                            self.is_paused = true;
                            self.is_playing = false;
                        }
                    } else {
                        if let Some(s) = &self.stream {
                            let _ = s.play();
                            self.is_paused = false;
                            self.is_playing = true;
                        }
                    }
                }

                if ui.button("⏹ Stop").clicked() {
                    for track in &self.synth_tracks {
                        let idx = track.sequence_idx.load(Ordering::Relaxed) as usize;
                        if let Some(nota) = track.sequence.get(idx) {
                            track.sequence_idx.store(0, Ordering::Relaxed);
                            track.position_synth.store(nota.length, Ordering::Relaxed);
                        }
                    }
                    self.stream = None;
                    self.is_playing = false;
                    self.is_paused = false;
                    self.playback_pos.store(0 as u64, Ordering::Relaxed);
                }
            });

            // Progress bar
            let device_rate = self.engine.config.sample_rate as f64;
            let current = self.playback_pos.load(Ordering::Relaxed);
            let progress = if self.playback_len > 0 {
                (current as f32 / self.playback_len as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let current_secs = current as f64 / device_rate;
            let total_secs = self.playback_len as f64 / device_rate;

            if self.playback_len > 0 {
                ui.add(
                    egui::ProgressBar::new(progress)
                        .text(format!("{:.1} / {:.1} sec", current_secs, total_secs)),
                );
            }

            if self.playback_len > 0 && self.drum_tracks.is_empty() {
                if current >= self.playback_len {
                    self.stream = None;
                    self.is_playing = false;
                    self.is_paused = false;
                    self.playback_pos.store(0 as u64, Ordering::Relaxed);
                }
            }

            ui.add_space(8.0);
            ui.separator();

            // ==========================================
            // SEZIONE TRACCE AUDIO WAV (CORSIE + DRAG & DROP)
            // ==========================================
            ui.horizontal(|ui| {
                ui.heading("Tracce Audio WAV (Griglia Corsie)");

                if ui.button("📁 Pick file (.wav)").clicked() {
                    self.file_dialog.select_multiple();
                }

                if ui.button("+ Nuova corsia").clicked() {
                    self.lanes.push(Vec::new());
                    self.selected_lane = self.lanes.len() - 1;
                }

                if self.lanes.len() > 1 {
                    egui::ComboBox::from_label("Aggiungi file a")
                        .selected_text(format!("Corsia {}", self.selected_lane + 1))
                        .show_ui(ui, |ui| {
                            for i in 0..self.lanes.len() {
                                ui.selectable_value(
                                    &mut self.selected_lane,
                                    i,
                                    format!("Corsia {}", i + 1),
                                );
                            }
                        });
                }
            });

            ui.label(
                "💡 Tracce nella stessa corsia suonano in SEQUENZA. Trascina (⠿) per riordinare. Corsie diverse suonano CONTEMPORANEAMENTE.",
            );

            // Gestione del file dialog
            self.file_dialog.update(ctx);

            if let Some(paths) = self.file_dialog.take_selected_multiple() {
                if self.lanes.is_empty() {
                    self.lanes.push(Vec::new());
                }
                if self.selected_lane >= self.lanes.len() {
                    self.selected_lane = 0;
                }
                let target_lane = self.selected_lane;
                let device_rate = self.engine.config.sample_rate as f64;

                for path in paths {
                    let wav = AudioEngine::load_wav(path.to_str().unwrap());
                    let ratio =
                        (wav.spec.sample_rate as f64 / device_rate) * wav.spec.channels as f64;
                    let duration_frames_device = (wav.samples.len() as f64 / ratio) as u64;
                    let duration_secs = wav.samples.len() as f64
                        / (wav.spec.sample_rate as f64 * wav.spec.channels as f64);
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    let clip = WavClip {
                        id: self.next_clip_id,
                        name,
                        path,
                        data: Arc::new(wav),
                        muted: Arc::new(AtomicBool::new(false)),
                        volume: 1.0,
                        volume_atomic: Arc::new(AtomicU32::new(1.0f32.to_bits())),
                        filters: Vec::new(),
                        duration_frames_device,
                        duration_secs,
                    };
                    self.next_clip_id += 1;
                    // Di default aggiunta in sequenza nella corsia selezionata
                    self.lanes[target_lane].push(clip);
                }
                self.recalculate_playback_len();
            }

            // Rendering delle corsie
            let mut pending_action: Option<ClipAction> = None;
            let mut lane_to_remove: Option<usize> = None;
            let total_lanes = self.lanes.len();

            for lane_idx in 0..total_lanes {
                let lane = &mut self.lanes[lane_idx];
                let lane_secs: f64 = lane.iter().map(|c| c.duration_secs).sum();

                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(format!("Corsia {}", lane_idx + 1));
                        ui.label(format!(
                            "({} clip in sequenza | Durata totale: {:.2}s)",
                            lane.len(),
                            lane_secs
                        ));

                        if total_lanes > 1 && ui.button("🗑 Elimina corsia").clicked() {
                            lane_to_remove = Some(lane_idx);
                        }
                    });

                    if lane.is_empty() {
                        ui.label("Nessuna traccia in questa corsia. Carica un file o sposta qui una traccia con ▲ / ▼.");
                    } else {
                        ScrollArea::horizontal()
                            .id_salt(format!("lane_scroll_{}", lane_idx))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 30.0;
                                    dnd(ui, format!("lane_dnd_{}", lane_idx)).show_vec(
                                        lane,
                                        |ui, clip, handle, state| {
                                            let clip_idx = state.index;
                                            ui.push_id(clip.id, |ui| {
                                                ui.group(|ui|{
                                                    ui.vertical(|ui| {
                                                        ui.set_width(240.0);
                                                        ui.set_height(150.0);
                                                        ui.spacing_mut().item_spacing =
                                                            egui::vec2(6.0, 6.0);

                                                        // Maniglia Drag & Drop per riordinare
                                                        handle.ui(ui, |ui| {
                                                            ui.horizontal(|ui| {
                                                                ui.label(
                                                                    egui::RichText::new("⠿")
                                                                        .size(16.0),
                                                                );
                                                                ui.strong(format!(
                                                                    "#{}: {}",
                                                                    clip_idx + 1,
                                                                    clip.name
                                                                ));
                                                            });
                                                        });
                                                        
                                                        ui.separator();
                                                            // Riga: Durata e Mute
                                                            ui.horizontal(|ui| {
                                                                ui.label(
                                                                    egui::RichText::new(format!(
                                                                        "⏱ {:.2}s",
                                                                        clip.duration_secs
                                                                    ))
                                                                    .weak(),
                                                                );
                                                                ui.with_layout(
                                                                    egui::Layout::right_to_left(
                                                                        egui::Align::Center,
                                                                    ),
                                                                    |ui| {
                                                                        let is_muted = clip
                                                                            .muted
                                                                            .load(Ordering::Relaxed);
                                                                        let mute_text = if is_muted {
                                                                            "🔇 Muted"
                                                                        } else {
                                                                            "🔊 Mute"
                                                                        };
                                                                        if ui.button(mute_text).clicked() {
                                                                            clip.muted.store(
                                                                                !is_muted,
                                                                                Ordering::Relaxed,
                                                                            );
                                                                        }
                                                                    },
                                                                );
                                                            });
                                                            ui.separator();
                                                        // Riga: Slider Volume
                                                        ui.horizontal(|ui| {
                                                            ui.label("Vol:");
                                                            if ui
                                                                .add(
                                                                    egui::Slider::new(
                                                                        &mut clip.volume,
                                                                        0.0..=1.0,
                                                                    )
                                                                    .show_value(true),
                                                                )
                                                                .changed()
                                                            {
                                                                clip.volume_atomic.store(
                                                                    clip.volume.to_bits(),
                                                                    Ordering::Relaxed,
                                                                );
                                                            }
                                                        });

                                                        ui.separator();

                                                        // Riga: Spostamento tra corsie
                                                        ui.horizontal(|ui| {
                                                            ui.label("Corsia:");
                                                            if lane_idx > 0 {
                                                                if ui
                                                                    .button("▲ Su")
                                                                    .on_hover_text(
                                                                        "Sposta nella corsia sopra",
                                                                    )
                                                                    .clicked()
                                                                {
                                                                    pending_action =
                                                                        Some(ClipAction::MoveUp {
                                                                            from_lane: lane_idx,
                                                                            clip_idx,
                                                                        });
                                                                }
                                                            }
                                                            if ui
                                                                .button("▼ Giù")
                                                                .on_hover_text(
                                                                    "Sposta nella corsia sotto",
                                                                )
                                                                .clicked()
                                                            {
                                                                pending_action =
                                                                    Some(ClipAction::MoveDown {
                                                                        from_lane: lane_idx,
                                                                        clip_idx,
                                                                    });
                                                            }
                                                            if ui
                                                                .button("⤓ Nuova")
                                                                .on_hover_text(
                                                                    "Sposta in una nuova corsia (suona contemporaneamente)",
                                                                )
                                                                .clicked()
                                                            {
                                                                pending_action =
                                                                    Some(ClipAction::MoveToNewLane {
                                                                        from_lane: lane_idx,
                                                                        clip_idx,
                                                                    });
                                                            }
                                                        });

                                                        // Riga: Eliminazione traccia
                                                        ui.horizontal(|ui| {
                                                            if ui
                                                                .button("🗑 Elimina traccia")
                                                                .on_hover_text("Rimuovi questa traccia")
                                                                .clicked()
                                                            {
                                                                pending_action =
                                                                    Some(ClipAction::Delete {
                                                                        from_lane: lane_idx,
                                                                        clip_idx,
                                                                    });
                                                            }
                                                        });

                                                        ui.separator();

                                                        // Filtri per traccia
                                                        ui.collapsing("Filtri", |ui| {
                                                            if ui.button("+ Aggiungi filtro").clicked()
                                                            {
                                                                clip.filters.push(Filter {
                                                                    filter_enabled: Arc::new(
                                                                        AtomicU8::new(0),
                                                                    ),
                                                                    filter_cutoff: Arc::new(
                                                                        AtomicU32::new(
                                                                            200.0f32.to_bits(),
                                                                        ),
                                                                    ),
                                                                    filter: Arc::new(Mutex::new(None)),
                                                                });
                                                            }

                                                            let mut filter_to_remove = None;
                                                            for (f_idx, filt) in
                                                                clip.filters.iter_mut().enumerate()
                                                            {
                                                                ui.push_id(f_idx, |ui| {
                                                                    let current_filter = filt
                                                                        .filter_enabled
                                                                        .load(Ordering::Relaxed);
                                                                    ui.group(|ui| {
                                                                        ui.horizontal(|ui| {
                                                                            ui.strong(format!(
                                                                                "Filtro {}",
                                                                                f_idx + 1
                                                                            ));
                                                                            ui.with_layout(
                                                                                egui::Layout::right_to_left(
                                                                                    egui::Align::Center,
                                                                                ),
                                                                                |ui| {
                                                                                    if ui
                                                                                        .button("🗑")
                                                                                        .on_hover_text(
                                                                                            "Rimuovi filtro",
                                                                                        )
                                                                                        .clicked()
                                                                                    {
                                                                                        filter_to_remove =
                                                                                            Some(f_idx);
                                                                                    }
                                                                                },
                                                                            );
                                                                        });

                                                                        let filter_names = [
                                                                            "Nessuno",
                                                                            "Lowpass (LP)",
                                                                            "Highpass (HP)",
                                                                            "Bandpass (BP)",
                                                                            "Notch",
                                                                        ];
                                                                        let cur_label = filter_names
                                                                            .get(
                                                                                current_filter as usize,
                                                                            )
                                                                            .copied()
                                                                            .unwrap_or("Nessuno");

                                                                        egui::ComboBox::from_id_salt(
                                                                            format!(
                                                                                "filter_combo_{}",
                                                                                f_idx
                                                                            ),
                                                                        )
                                                                        .selected_text(cur_label)
                                                                        .show_ui(ui, |ui| {
                                                                            for (val, name) in
                                                                                filter_names
                                                                                    .iter()
                                                                                    .enumerate()
                                                                            {
                                                                                let selected =
                                                                                    current_filter
                                                                                        == val as u8;
                                                                                if ui
                                                                                    .selectable_label(
                                                                                        selected, *name,
                                                                                    )
                                                                                    .clicked()
                                                                                {
                                                                                    filt.filter_enabled
                                                                                        .store(
                                                                                            val as u8,
                                                                                            Ordering::Relaxed,
                                                                                        );
                                                                                    let cutoff = f32::from_bits(
                                                                                        filt.filter_cutoff
                                                                                            .load(Ordering::Relaxed),
                                                                                    );
                                                                                    *filt
                                                                                        .filter
                                                                                        .lock()
                                                                                        .unwrap() =
                                                                                        create_filter_node(
                                                                                            val as u8,
                                                                                            cutoff,
                                                                                        );
                                                                                }
                                                                            }
                                                                        });

                                                                        if current_filter != 0 {
                                                                            let mut cutoff =
                                                                                f32::from_bits(
                                                                                    filt.filter_cutoff
                                                                                        .load(
                                                                                            Ordering::Relaxed,
                                                                                        ),
                                                                                );
                                                                            if ui
                                                                                .add(
                                                                                    egui::Slider::new(
                                                                                        &mut cutoff,
                                                                                        200.0..=8000.0,
                                                                                    )
                                                                                    .text("Cutoff Hz"),
                                                                                )
                                                                                .changed()
                                                                            {
                                                                                filt.filter_cutoff.store(
                                                                                    cutoff.to_bits(),
                                                                                    Ordering::Relaxed,
                                                                                );
                                                                                *filt
                                                                                    .filter
                                                                                    .lock()
                                                                                    .unwrap() =
                                                                                    create_filter_node(
                                                                                        current_filter,
                                                                                        cutoff,
                                                                                    );
                                                                            }
                                                                        }
                                                                    });
                                                                });
                                                            }
                                                            if let Some(fi) = filter_to_remove {
                                                                clip.filters.remove(fi);
                                                            }
                                                        });
                                                    });
                                                });
                                            });
                                        },
                                    );
                                });
                            });
                    }
                });
                ui.add_space(4.0);
            }

            // Applica azioni differite sulle clip
            if let Some(action) = pending_action {
                if self.is_playing {
                    self.stream = None;
                    self.is_playing = false;
                    self.is_paused = false;
                    self.playback_pos.store(0 as u64, Ordering::Relaxed);
                }

                match action {
                    ClipAction::MoveUp {
                        from_lane,
                        clip_idx,
                    } => {
                        if from_lane > 0 && clip_idx < self.lanes[from_lane].len() {
                            let clip = self.lanes[from_lane].remove(clip_idx);
                            self.lanes[from_lane - 1].push(clip);
                        }
                    }
                    ClipAction::MoveDown {
                        from_lane,
                        clip_idx,
                    } => {
                        if clip_idx < self.lanes[from_lane].len() {
                            let clip = self.lanes[from_lane].remove(clip_idx);
                            if from_lane + 1 < self.lanes.len() {
                                self.lanes[from_lane + 1].push(clip);
                            } else {
                                self.lanes.push(vec![clip]);
                            }
                        }
                    }
                    ClipAction::MoveToNewLane {
                        from_lane,
                        clip_idx,
                    } => {
                        if clip_idx < self.lanes[from_lane].len() {
                            let clip = self.lanes[from_lane].remove(clip_idx);
                            self.lanes.push(vec![clip]);
                        }
                    }
                    ClipAction::Delete {
                        from_lane,
                        clip_idx,
                    } => {
                        if clip_idx < self.lanes[from_lane].len() {
                            self.lanes[from_lane].remove(clip_idx);
                        }
                    }
                }
                self.recalculate_playback_len();
            }

            if let Some(lane_idx) = lane_to_remove {
                if self.lanes.len() > 1 && lane_idx < self.lanes.len() {
                    if self.is_playing {
                        self.stream = None;
                        self.is_playing = false;
                        self.is_paused = false;
                        self.playback_pos.store(0 as u64, Ordering::Relaxed);
                    }
                    self.lanes.remove(lane_idx);
                    if self.selected_lane >= self.lanes.len() {
                        self.selected_lane = self.lanes.len().saturating_sub(1);
                    }
                    self.recalculate_playback_len();
                }
            }

            ui.add_space(8.0);
            ui.separator();

            // ==========================================
            // SEZIONE DRUM MACHINE
            // ==========================================
            ui.heading("Drum Machine");

            ui.horizontal(|ui| {
                if ui.button("Add kick").clicked() {
                    self.drum_tracks.push(DrumTrack {
                        name: String::from("Kick"),
                        selected_sample: 0,
                        sample_data: Some(Vec::new()),
                        sample_rate: 0,
                        pattern: [false; 32],
                        volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
                        muted: Arc::new(AtomicBool::new(false)),
                        bpm: Arc::new(AtomicU32::new(self.bpm as u32)),
                        sample_pos: Arc::new(AtomicU64::new(0)),
                    });
                }

                if ui.button("Add snare").clicked() {
                    self.drum_tracks.push(DrumTrack {
                        name: String::from("Snare"),
                        selected_sample: 0,
                        sample_data: Some(Vec::new()),
                        sample_rate: 0,
                        pattern: [false; 32],
                        volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
                        muted: Arc::new(AtomicBool::new(false)),
                        bpm: Arc::new(AtomicU32::new(120)),
                        sample_pos: Arc::new(AtomicU64::new(0)),
                    });
                }

                if ui.button("Add hi-hat").clicked() {
                    self.drum_tracks.push(DrumTrack {
                        name: String::from("Hi-hat"),
                        selected_sample: 0,
                        sample_data: Some(Vec::new()),
                        sample_rate: 0,
                        pattern: [false; 32],
                        volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
                        muted: Arc::new(AtomicBool::new(false)),
                        bpm: Arc::new(AtomicU32::new(120)),
                        sample_pos: Arc::new(AtomicU64::new(0)),
                    });
                }

                ui.label("BPM:");
                if ui.add(egui::DragValue::new(&mut self.bpm).speed(0.1)).changed() {
                    for drum_track in &mut self.drum_tracks {
                        drum_track.bpm = Arc::new(AtomicU32::new(self.bpm as u32));
                    }
                }
            });

            let mut drums_to_remove = None;
            ui.horizontal(|ui| {
                for (i, track) in self.drum_tracks.iter_mut().enumerate() {
                    match track.name.as_str() {
                        "Kick" => {
                            let label = format!("Rimuovi Kick {}", i + 1);
                            if ui.button(label).clicked() {
                                drums_to_remove = Some(i);
                            }
                        }
                        "Snare" => {
                            let label = format!("Rimuovi Snare {}", i + 1);
                            if ui.button(label).clicked() {
                                drums_to_remove = Some(i);
                            }
                        }
                        "Hi-hat" => {
                            let label = format!("Rimuovi Hi-hat {}", i + 1);
                            if ui.button(label).clicked() {
                                drums_to_remove = Some(i);
                            }
                        }
                        _ => continue,
                    }
                }
            });
            if let Some(i) = drums_to_remove {
                self.drum_tracks.remove(i);
            }

            let mut drumscount = 0;
            for drum in self.drum_tracks.iter_mut() {
                ui.push_id(drumscount, |ui| {
                    let (samples, dialog) = match drum.name.as_str() {
                        "Kick"   => (&mut self.kick_samples,  &mut self.file_dialog_kick),
                        "Snare"  => (&mut self.snare_samples, &mut self.file_dialog_snare),
                        "Hi-hat" => (&mut self.hihat_samples, &mut self.file_dialog_hihat),
                        _        => return,
                    };

                    ui.horizontal(|ui| {
                        let selected_name = samples
                            .get(drum.selected_sample)
                            .map(|(name, _)| name.as_str())
                            .unwrap_or("— nessuno —");
                        if !samples.is_empty() {
                            egui::ComboBox::from_label(&drum.name)
                                .selected_text(selected_name)
                                .show_ui(ui, |ui| {
                                    for (i, (name, path)) in samples.iter().enumerate() {
                                        // if name.is_empty() {name="- nessuno -"};
                                        if ui.selectable_value(&mut drum.selected_sample, i, name).changed() {
                                            drum.sample_data = Some(
                                                AudioEngine::load_wav(path.to_str().unwrap()).samples
                                            );
                                            drum.sample_rate = AudioEngine::load_wav(
                                                path.to_str().unwrap()
                                            ).spec.sample_rate;
                                        }
                                    }
                                });
                        }

                        if ui.button("Add sample +").clicked() {
                            dialog.select_multiple();
                        }
                    });

                    // gestisci i file scelti
                    dialog.update(ctx);
                    if let Some(paths) = dialog.take_selected_multiple() {
                        for path in paths {
                            let name = path.file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            samples.push((name, path));
                        }
                    }
                });
                drumscount += 1;

                // griglia 32 step
                ui.horizontal(|ui| {
                    for step in drum.pattern.iter_mut() {
                        let label = if *step { "■" } else { "□" };
                        if ui.button(label).clicked() {
                            *step = !*step;
                        }
                    }
                });
            }

            ui.add_space(8.0);
            ui.separator();

            // ==========================================
            // SEZIONE SYNTH
            // ==========================================
            ui.heading("Synth Modulari");

            if ui.button("Add synth").clicked() {
                self.synth_tracks.push(SynthTrack::new());
            }

            let mut synth_to_remove = None;
            for (i, t) in self.synth_tracks.iter().enumerate() {
                if ui.button(format!("Remove synth {}", i + 1)).clicked() {
                    synth_to_remove = Some(i);
                    let idx = t.sequence_idx.load(Ordering::Relaxed) as usize;
                    if let Some(nota) = t.sequence.get(idx) {
                        t.sequence_idx.store(0, Ordering::Relaxed);
                        t.position_synth.store(nota.length, Ordering::Relaxed);
                    }
                }
            }
            if let Some(i) = synth_to_remove {
                self.synth_tracks.remove(i);
            }

            pub const WAVETYPES: &[(u8, &str)] = &[
                (0, "Sin"),
                (1, "Square"),
                (2, "Triangle"),
            ];

            for (i, st) in self.synth_tracks.iter_mut().enumerate() {
                ui.label(format!("Synth {}", i + 1));

                // volume
                let mut v = f32::from_bits(st.volume.load(Ordering::Relaxed));
                if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).text("Volume")).changed() {
                    st.volume.store(v.to_bits(), Ordering::Relaxed);
                }

                if ui.button(format!("Add note to synth {}", i + 1)).clicked() {
                    let beat = (self.engine.config.sample_rate as f64 * 0.5) as u64;
                    let frequency = 440.0;
                    st.sequence.push(Note {
                        frequency,
                        length: beat,
                    });
                }

                // slider per ogni nota
                for note in st.sequence.iter_mut() {
                    ui.horizontal(|ui| {
                        ui.add(egui::Slider::new(&mut note.frequency, 0.0..=2000.0).text("Hz"));
                        let sample_rate = self.engine.config.sample_rate as f64;
                        let mut secs = note.length as f64 / sample_rate;
                        if ui.add(egui::Slider::new(&mut secs, 0.1..=10.0).text("sec")).changed() {
                            note.length = (secs * sample_rate) as u64;
                        }
                    });
                }

                let mut synth_index = st.wave_type.load(Ordering::Relaxed) as usize;
                ui.push_id(i, |ui| {
                    egui::ComboBox::from_label("Wave Type")
                        .selected_text(WAVETYPES[synth_index].1)
                        .show_ui(ui, |ui| {
                            for (value, wavename) in WAVETYPES.iter() {
                                if ui.selectable_value(&mut synth_index, (*value).into(), *wavename).changed() {
                                    st.wave_type.store(*value as u8, Ordering::Relaxed);
                                    let freq = st
                                        .sequence
                                        .get(st.sequence_idx.load(Ordering::Relaxed) as usize)
                                        .map(|n| n.frequency)
                                        .unwrap_or(440.0);
                                    *st.node.lock().unwrap() = match value {
                                        0 => Box::new(sine_hz(freq)),
                                        1 => Box::new(square_hz(freq)),
                                        2 => Box::new(triangle_hz(freq)),
                                        _ => Box::new(sine_hz(freq)),
                                    };
                                }
                            }
                        });
                });
            }

            if self.is_playing && !self.is_paused {
                ctx.request_repaint_after(std::time::Duration::from_millis(16)); // ~60fps
            }
        });
    }
}