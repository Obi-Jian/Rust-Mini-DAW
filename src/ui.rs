use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use crate::audio::AudioEngine;
use eframe::egui;
use cpal::traits::StreamTrait;

pub struct MyDawApp {
    engine: AudioEngine,
    stream: Option<cpal::Stream>,
    is_playing: bool,
    is_paused: bool,
    track_muted: Vec<Arc<AtomicBool>>
}

impl MyDawApp {
    pub fn new() -> Self {
        Self {
            engine: AudioEngine::new(), // Inizializza host/device
            stream: None,
            is_playing: false,
            is_paused: false,
            track_muted: Vec::new(),
        }
    }
}

impl eframe::App for MyDawApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Controllo Audio");
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
    }
}