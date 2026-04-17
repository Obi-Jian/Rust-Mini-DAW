mod audio;
use audio::AudioEngine;
use cpal::traits::StreamTrait;

fn main() {
    let engine = AudioEngine::new();

    // Carichi i dati
    let data_a = AudioEngine::load_wav("/Users/generalkenobi/LocalDocuments/Rust/audio/gong.wav");
    let data_b = AudioEngine::load_wav("/Users/generalkenobi/LocalDocuments/Rust/audio/laughter.wav");

    // Crei lo stream passandogli i campioni
    let stream = engine.setup_stream(data_a, data_b).unwrap();

    // Lo stream è pronto, basta un .play()
    stream.play().unwrap();
    std::thread::sleep(std::time::Duration::from_secs(10));

}