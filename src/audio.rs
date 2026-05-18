use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{FromSample, Sample, SampleFormat};
use fundsp::prelude::AudioUnit;
use fundsp::prelude32::{dc, sine_hz};
use hound;

pub struct WavData {
    pub samples: Vec<f32>,
    pub spec: hound::WavSpec,
}

pub struct AudioEngine {
    pub device: cpal::Device,
    pub config: cpal::StreamConfig,
    pub sample_format: cpal::SampleFormat,
}

pub struct Track {
    pub data: WavData,
    pub muted: Arc<AtomicBool>,
    pub volume: Arc<AtomicU32>,
    pub filters: Vec<Filter>,
}

pub struct SynthTrack {
    pub muted: Arc<AtomicBool>,
    pub volume: Arc<AtomicU32>,
    // Penserò dopo ai filtri
    // pub filters: Vec<Filter>,
    // pub wave_type: Arc<Mutex<WaveType>>,
    pub node: Arc<Mutex<Box<dyn AudioUnit + Send>>>,
    pub sequence: Vec<Note>,
    pub position_synth: Arc<AtomicU64>,
    pub sequence_idx: Arc<AtomicU32>,
}

impl SynthTrack {
    pub fn new() -> Self {
        SynthTrack {
            muted: Arc::new(AtomicBool::new(false)),
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            // filters: Vec::new(),
            // wave_type: Arc::new(Mutex::new(WaveType::Sine)),
            node: Arc::new(Mutex::new(Box::new(sine_hz(0.0)))),
            sequence: Vec::new(),
            position_synth: Arc::new(AtomicU64::new(0)),
            sequence_idx: Arc::new(AtomicU32::new(0)),
        }
    }
    
    pub fn to_stream(&self) -> SynthTrack {
        SynthTrack {
            muted: Arc::clone(&self.muted),
            volume: Arc::clone(&self.volume),
            node: Arc::clone(&self.node),
            sequence: self.sequence.clone(),
            position_synth: Arc::clone(&self.position_synth),
            sequence_idx: Arc::clone(&self.sequence_idx),
        }
    }
}

pub struct DrumTrack {
    pub name: String,                    // "Kick", "Snare", "Hat"
    pub selected_sample: usize,          // indice nel catalogo
    pub sample_data: Option<Vec<f32>>,   // wav caricato
    pub pattern: [bool; 16],             // griglia 16 step
    pub muted: Arc<AtomicBool>,
    pub volume: Arc<AtomicU32>,
    pub bpm: Arc<AtomicU32>,
    pub sample_pos: Arc<AtomicU64>,  // posizione dentro il sample corrente, u64::MAX = non sta suonando
}

impl DrumTrack {
    pub fn to_stream(&self) -> DrumTrack {
        DrumTrack {
            name: self.name.clone(),
            muted: Arc::clone(&self.muted),
            volume: Arc::clone(&self.volume),
            selected_sample: 0,
            sample_data: self.sample_data.clone(),
            pattern: self.pattern,
            bpm: Arc::clone(&self.bpm),
            sample_pos: Arc::clone(&self.sample_pos),
        }
    }
}

// dobbiamo rendere clonabile Note perchè se impostassimo semplicemente sequence (vettore di Note) come puntatore atomico Arc
// (come con tutti gli altri parametri), nella funzione build_output_stream dovremmo chiamare un lock aggiuntivo ad ogni frame,
// possibilmente bloccando la UI
#[derive(Clone)]
pub struct Note {
    pub frequency: NoteSource,
    pub length: u64, // l'input è in secondi ma verrà convertito dal frontend in frames
}


#[derive(Clone)]
pub enum NoteSource {
    Frequency(f32),
    Sample(Vec<f32>),
}

pub struct Source {
    pub tracks: Vec<Track>,
    pub synth_tracks: Vec<SynthTrack>,
    pub drum_tracks: Vec<DrumTrack>,
}

pub enum WaveType {
    Sine,
    Triangle,
    Square,
}

// #[derive(Clone, Copy, PartialEq)]
/* pub enum FilterType {
    None,
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
}

impl FilterType {
    pub fn to_u8(self) -> u8 { self as u8 }
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => FilterType::Lowpass,
            2 => FilterType::Highpass,
            3 => FilterType::Bandpass,
            4 => FilterType::Notch,
            _ => FilterType::None,
        }
    }
} */
pub struct Filter {
    pub filter_enabled: Arc<AtomicU8>,
    pub filter_cutoff: Arc<AtomicU32>,   // Hz come f32 bits
    pub filter: Arc<Mutex<Option<Box<dyn AudioUnit + Send>>>>,
}
/* impl Track {
    fn new(samples: WavData, muted: Arc<AtomicBool>, volume: Arc<AtomicU32>) -> Self {
        let lowpass_enabled = Arc::<AtomicBool>::new(false.into());
        let lowpass_size= Arc::<AtomicU32>::new(0.into());
        Self { data: samples, muted, volume, lowpass_enabled, lowpass_size}
    }
} */

impl AudioEngine {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("Nessun device di output trovato");
        
        let supported_config = device.supported_output_configs()
            .expect("Errore nel recupero delle config")
            .next()
            .expect("Nessuna config supportata")
            .with_max_sample_rate();

        let sample_format = supported_config.sample_format();
        let config = supported_config.into();

        Self { device, config, sample_format }
    }

    // Carica un file wav e restituisce i campioni normalizzati
    pub fn load_wav(path: &str) -> WavData {
        let mut reader = hound::WavReader::open(path).expect("Errore nell'apertura del file");
        let spec = reader.spec();
        let samples = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
            hound::SampleFormat::Int => {
                let max_val = (1_i32 << (spec.bits_per_sample - 1)) as f32;
                reader.samples::<i32>().map(|s| s.unwrap() as f32 / max_val).collect()
            }
        };
        WavData { samples, spec }
    }

    // Questa è la tua build_stream trasformata in metodo
    // DA MODIFICARE
    pub fn setup_stream(
        &self,
        // tracks: Vec<Track>,
        source: Source,
        position: Arc<AtomicU64>,

    ) -> Result<cpal::Stream, cpal::BuildStreamError> {
        // let tracks = Track::new( samples, muted, volume);
        let err_fn = |err| eprintln!("Errore nello stream: {}", err);
        let config = self.config.clone();

        match self.sample_format {
            /* SampleFormat::F32 => self.create_stream::<f32>(&config,samples, muted, volume, position, err_fn),
            SampleFormat::I16 => self.create_stream::<i16>(&config, samples, muted,/* samples_a, samples_b, */ volume, position, err_fn),
            SampleFormat::U16 => self.create_stream::<u16>(&config, samples, muted,/* samples_a, samples_b, */volume , position, err_fn), */
            SampleFormat::F32 => self.create_stream::<f32>(&config, source, position, err_fn),
            SampleFormat::I16 => self.create_stream::<i16>(&config, source, position, err_fn),
            SampleFormat::U16 => self.create_stream::<u16>(&config, source, position, err_fn),
            _ => panic!("Formato non supportato"),
        }
    }

    // questi sono filtri esempio per comprenderli meglio, quelli effettivi sono gestiti da fundsp
    pub fn lowpass_wrong(
        samples: Vec<u32>,
        size: u32,
    ) -> Vec<u32> {
        let mut filtered: Vec<u32> = Vec::new();
        for frame in samples.windows(size as usize*2+1) { 
            let mut media = 0;
            for output in frame.iter() {
                media += *output;           // somma tutto
            }
            media /= frame.len() as u32;   // dividi per il numero reale di elementi
            filtered.push(media);
        }
        filtered
    }

    pub fn lowpass(samples: &[f32], size: usize) -> Vec<f32> {
        let mut filtered = Vec::with_capacity(samples.len());
        for i in 0..samples.len() {
            // NOTA: i è l'index, non il contenuto
            // per ogni i di samples iteriamo una finestra da start a end
            // dopodichè facciamo la media degli elementi e pushiamo su filtered
            // ci serve una finestra che cambia, quindi per ogni elemento di samples la ricalcoliamo
            // start ed end cambiano perchè per gli elementi da 0 a size deve essere i
            // da samples.len() - size a sample.len() deve essere i - samples.len()
            // in tutti gli altri casi va da i - size a i + size + 1
            let start = i.saturating_sub(size);
            let end = (i + size + 1).min(samples.len());
            let window = &samples[start..end];
            let media: f32 = window.iter().sum::<f32>() / window.len() as f32;
            filtered.push(media);
        }
        filtered
    }

    // Funzione privata helper per gestire i generici
    fn create_stream<T>(
        &self,
        config: &cpal::StreamConfig,
        //tracks: Vec<Track>,
        source: Source,
        /* data: Vec<WavData>,
        muted: Vec<Arc<AtomicBool>>,
        volume: Vec<Arc<AtomicU32>>, */
        position: Arc<AtomicU64>,
        err_fn: impl Fn(cpal::StreamError) + Send + 'static,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: Sample + cpal::SizedSample + FromSample<f32>,
    {   
        let tracks: Vec<Track> = source.tracks;
        let synth_tracks: Vec<SynthTrack> = source.synth_tracks;
        let drum_tracks: Vec<DrumTrack> = source.drum_tracks;
        let mut global_frame: u64 = 0; // posizione attuale globale
        let channels_device = config.channels as usize;
        let channels: Vec<usize> = tracks.iter()
            .map(|d| {
                let ch = d.data.spec.channels as usize;
                assert!(ch > 0, "Traccia con 0 canali!");
                ch
            })
            .collect();

        // ratio è il rapporto tra sample rate del file e del device, con n tracce audio ci sono n elementi nel vettore ratio
        // si trova dividendo il primo per il secondo e poi moltiplicando per il numero di canali del file
        // es. con device a 48.1Hz traccia stereo a 44.1Hz si fa (441000/48100)*2 = 1.8
        let ratio: Vec<f64> = tracks.iter()
            .map(|d| (d.data.spec.sample_rate as f64 / config.sample_rate as f64 ) * d.data.spec.channels as f64)
            .collect();

        let sample_rate = config.sample_rate as f64; // estrai il valore prima della closure

        let mut pos: Vec<f64> = vec![0.0; tracks.len()];

        let get_interpolated = |samples: &[f32], pos: f64| -> f32 {

            // cosa facciamo qui?
            // 1. prendiamo la posizione (es 2.1768), la tronchiamo (NON arrotondiamo)
            // 2. troviamo gli interi più vicini (sample[2] e sample[3] nel nostro caso)
            // 3. calcoliamo t, che sarebbe 0.1768
            // 4. troviamo la frequenza (ossia un f32 di sample) che si trova 17.68% tra sample[2] e sample[3]
            // es. se sample[1] = 15.79 e sample[2] = 18.11, dato che noi non vogliamo esattamente sample[1], ma nemmeno sample[2], ->
            // -> calcoliamo cosa succede al 17.68% della strada tra uno e l'altro facendo s1 + (t * (s2-s1)) = 15.79 + (0.1768 * (18.11 - 15.79)) = 16.20

            let idx = pos as usize;
            let s1 = samples.get(idx).copied().unwrap_or(0.0);
            let s2 = samples.get(idx + 1).copied().unwrap_or(s1);
            let t = (pos - idx as f64) as f32;
            s1 + t * (s2 - s1)
        };

        self.device.build_output_stream(
            config,
            move |data_callback: &mut [T], _| {
                for frame in data_callback.chunks_mut(channels_device) { // un frame (se audio stereo) contiene due canali quindi facciamo un for per ogni frame e poi un for per ogni canale nel frame

                    // PRIMA di calcolare l'outfit effettivo nel prossimo loop for dobbiamo settare correttamente il nodo di ogni traccia synth
                    // La logica è che in base alla posizione (frame) attuale, comprendiamo in che punto del sequencer ci troviamo e vediamo quale nota suonare (come impostare il nodo)
                    // l'output finale per ogni canale di ogni frame è wav_sum + synth_sum
                    // 
                    for st in &synth_tracks {
                        // if st.muted.load(Ordering::Relaxed) { return 0.0; }
                        let posizione = st.position_synth.load(Ordering::Relaxed); // posizione è il frame all'interno di ogni nota in cui ci troviamo
                        let idx = &st.sequence_idx.load(Ordering::Relaxed); // l'index è alla nota del sequencer in cui ci troviamo
                        if st.sequence.is_empty() { continue; } // guard per sequenza vuota
                        let note = &st.sequence[*idx as usize];
                        if posizione >= note.length /* && *idx as usize <= st.sequence.len()  */{ // se siamo in una posizione maggiore della durata di una nota, cambia il nodo e imposta la posizione a 1
                            let index: u32 = (*idx +1) % st.sequence.len() as u32;
                            st.sequence_idx.store(index, Ordering::Relaxed);
                            st.position_synth.store(1, Ordering::Relaxed); // 1 perchè lo 0 lo abbiamo appena usato
                            let mut node = st.node.lock().unwrap();
                            let next_note = &st.sequence[index as usize];


                            *node = match &next_note.frequency {
                                NoteSource::Frequency(freq) if *freq == 0.0 => Box::new(dc(0.0)),
                                NoteSource::Frequency(freq) => Box::new(sine_hz(*freq)),
                                NoteSource::Sample(_) => Box::new(dc(0.0)), // placeholder per ora
                            };
                            /* *node = if next_note.frequency == 0.0 {
                                Box::new(dc(0.0)) // silenzio
                            } else {
                                Box::new(sine_hz(next_note.frequency))
                            };
                            println!("{}", next_note.frequency); */
                        } else { st.position_synth.store((posizione + 1) as u64, Ordering::Relaxed); }
                    }

                    for (ch, output) in frame.iter_mut().enumerate() {
                        // per ogni canale del device, leggiamo il canale corrispondente
                        // del file (wrappando se il file ha meno canali, es. mono su stereo)
                        /* let chan: Vec<usize> = channels.iter()
                            .map(|d| ch % d )
                            .collect(); */ // abbiamo implementato l'iter dei canali direttamente in val

                        // .zip() prende due iteratori e li "accoppia" elemento per elemento, producendo tuple
                        // [1, 2, 3].iter().zip([10, 20, 30].iter()) produce: (&1, &10), (&2, &20), (&3, &30)
                        let val: Vec<f32> = tracks.iter()
                            //.zip(muted.iter()) // produce: (d, m)
                            .zip(channels.iter()) // produce: ((d, m), num_ch)
                            .zip(pos.iter()) // produe: (((d, m), num_ch), p)
                            //.zip(volume.iter())
                            .map(|((track, &num_ch), &p)| {
                                // load significa che prende la variabile all'interno di track.muted (in questo caso booleano)
                                // Ordering serve per capire chi accede prima alla risorsa (Arc è un puntatore atomico, per variabili concorrenti)
                                if track.muted.load(Ordering::Relaxed) {
                                    0.0
                                } else {
                                    // ch è il canale nel frame che stiamo controllando
                                    // num_ch è preso da channels ed è 1 (se mono) o 2 (se stereo)
                                    // 1%1 = 0, 1%2 = 1 2%2 = 0, 2%1 = 0 (impossibile, non può essere il secondo ch se audio ha solo 1 canale)
                                    let c = ch % num_ch;
                                    // p all'inizio è tutto 0, poi viene aggiornato dopo ogni frame aggiornando il rateo (vedi più in basso)
                                    let mut sample = get_interpolated(&track.data.samples, p + c as f64);
                                    // qui modifichiamo ogni sample, in base al tipo di filtro che abbiamo impostato (da ui, sennò di default è 0)
                                    for filt in &track.filters { 
                                        if filt.filter_enabled.load(Ordering::Relaxed) != 0 {
                                            // non ho capito perchè ha fatto così, io avrei usato un match, ma non andava
                                            let mut guard = filt.filter.lock().unwrap();
                                            if let Some(f) = guard.as_mut() {
                                                let mut out = [0.0f32];
                                                f.tick(&[sample], &mut out);
                                                sample = out[0];
                                            }
                                        }
                                    }
                                    let v = f32::from_bits(track.volume.load(Ordering::Relaxed));
                                    sample * v
                                }
                            })
                            .collect();

                        let drum_sum: f32 = drum_tracks.iter().map(|drum| {
                            if drum.muted.load(Ordering::Relaxed) { return 0.0; }

                            let frames_per_step = (sample_rate as f64 / (drum.bpm.load(Ordering::Relaxed) as f64 / 60.0 * 4.0)) as u64;
                            let current_step = (global_frame / frames_per_step) % 16;
                            let step_start = current_step * frames_per_step;

                            // siamo esattamente all'inizio di uno step attivo?
                            if global_frame == step_start && drum.pattern[current_step as usize] {
                                drum.sample_pos.store(0, Ordering::Relaxed);
                            }

                            let pos = drum.sample_pos.load(Ordering::Relaxed);
                            if let Some(samples) = &drum.sample_data {
                                if pos < samples.len() as u64 {
                                    let s = samples[pos as usize];
                                    drum.sample_pos.store(pos + 1, Ordering::Relaxed);
                                    let v = f32::from_bits(drum.volume.load(Ordering::Relaxed));
                                    return s * v;
                                }
                            }
                            0.0
                        }).sum();

                        // somma è un vettore che contiene con un sample per traccia
                        // let somma : f32 = val.iter().sum();
                        // let divisore = 1.0 / tracks.len() as f32; // questo era solo per le tracks
                        // campioni dalle tracce wav
                        // wav_sum è un vettore che contiene un sample per traccia WAV
                        let wav_sum: f32 = val.iter().sum();

                        // campioni dai synth
                        let synth_sum: f32 = synth_tracks.iter().map(|st| {
                            if st.muted.load(Ordering::Relaxed) { return 0.0; }
                            let mut node_guard = st.node.lock().unwrap(); // vedi punto 3
                            let mut out = [0.0f32];
                            node_guard.tick(&[], &mut out);
                            let v = f32::from_bits(st.volume.load(Ordering::Relaxed));
                            out[0] * v
                        }).sum();

                        let total = tracks.len() + synth_tracks.len() + drum_tracks.len() ;
                        *output = T::from_sample((wav_sum + synth_sum + drum_sum) / total as f32);

                        // *output = T::from_sample(somma * divisore);
                    
                    }
                    
                    // per ogni frame che il device consuma, devo avanzare di "rateo" posizioni nel vec del file
                    // il rateo se sample rate di device e del file coincidessero, sarebbe 1 (mono) o 2 (stereo)
                    // il codice esegue tante iterazioni quante frame_device
                    // frame_device = durata_secondi × 44100
                    // chiamate_callback = frame_device / numero frame file
                    for (p, r) in pos.iter_mut().zip(ratio.iter()) {
                        *p += r ;
                    }
                    global_frame += 1; // aggiorna posizione attuale globale
                    position.store(global_frame, Ordering::Relaxed); // e la stora nel puntatore

                    
                }
            },
            err_fn,
            None,
        )
    }    
}