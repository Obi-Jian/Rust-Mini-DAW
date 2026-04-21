use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Sample, SampleFormat, FromSample};
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
    pub fn setup_stream(
        &self,
        samples: Vec<WavData>,
        muted: Vec<Arc<AtomicBool>>,
        volume: Vec<Arc<AtomicU32>>,

    ) -> Result<cpal::Stream, cpal::BuildStreamError> {
        let err_fn = |err| eprintln!("Errore nello stream: {}", err);
        let config = self.config.clone();

        match self.sample_format {
            SampleFormat::F32 => self.create_stream::<f32>(&config,samples, muted, volume, err_fn),
            SampleFormat::I16 => self.create_stream::<i16>(&config, samples, muted,/* samples_a, samples_b, */ volume, err_fn),
            SampleFormat::U16 => self.create_stream::<u16>(&config, samples, muted,/* samples_a, samples_b, */volume , err_fn),
            _ => panic!("Formato non supportato"),
        }
    }

    // Funzione privata helper per gestire i generici
    fn create_stream<T>(
        &self,
        config: &cpal::StreamConfig,
        data: Vec<WavData>,
        muted: Vec<Arc<AtomicBool>>,
        volume: Vec<Arc<AtomicU32>>,
        err_fn: impl Fn(cpal::StreamError) + Send + 'static,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: Sample + cpal::SizedSample + FromSample<f32>,
    {
        let channels_device = config.channels as usize;
        let channels: Vec<usize> = data.iter()
            .map(|d| {
                let ch = d.spec.channels as usize;
                assert!(ch > 0, "Traccia con 0 canali!");
                ch
            })
            .collect();

        // ratio è il rapporto tra sample rate del file e del device, con n tracce audio ci sono n elementi nel vettore ratio
        // si trova dividendo il primo per il secondo e poi moltiplicando per il numero di canali del file
        // es. con device a 48.1Hz traccia stereo a 44.1Hz si fa (441000/48100)*2 = 1.8
        let ratio: Vec<f64> = data.iter()
            .map(|d| (d.spec.sample_rate as f64 / config.sample_rate as f64 ) * d.spec.channels as f64)
            .collect();

        let mut pos: Vec<f64> = vec![0.0; data.len()];

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
                    for (ch, output) in frame.iter_mut().enumerate() {
                        // per ogni canale del device, leggiamo il canale corrispondente
                        // del file (wrappando se il file ha meno canali, es. mono su stereo)
                        
                        /* let chan: Vec<usize> = channels.iter()
                            .map(|d| ch % d )
                            .collect(); */ // abbiamo implementato l'iter dei canali direttamente in val

                        // .zip() prende due iteratori e li "accoppia" elemento per elemento, producendo tuple
                        // [1, 2, 3].iter().zip([10, 20, 30].iter()) produce: (&1, &10), (&2, &20), (&3, &30)
                        let val: Vec<f32> = data.iter()
                            .zip(muted.iter()) // produce: (d, m)
                            .zip(channels.iter()) // produce: ((d, m), num_ch)
                            .zip(pos.iter()) // produe: (((d, m), num_ch), p)
                            .zip(volume.iter())
                            .map(|((((d, m), &num_ch), &p), vol)| {
                                if m.load(Ordering::Relaxed) {
                                    0.0
                                } else {
                                    // ch è il canale nel frame che stiamo controllando
                                    // num_ch è preso da channels ed è 1 (se mono) o 2 (se stereo)
                                    // 1%1 = 0, 1%2 = 1 2%2 = 0, 2%1 = 0 (impossibile, non può essere il secondo ch se audio ha solo 1 canale)
                                    let c = ch % num_ch;
                                    // p all'inizio è tutto 0, poi viene aggiornato dopo ogni frame aggiornando il rateo (vedi più in basso)
                                    let sample = get_interpolated(&d.samples, p + c as f64);
                                    let v = f32::from_bits(vol.load(Ordering::Relaxed));
                                    sample * v
                                }
                            })
                            .collect();
                        
                        let divisore = 1.0 / data.len() as f32;
                        let somma : f32 = val.iter().sum();

                        *output = T::from_sample(somma * divisore);
                    }
                    
                    // per ogni frame che il device consuma, devo avanzare di "rateo" posizioni nel vec del file
                    // il rateo se sample rate di device e del file coincidessero, sarebbe 1 (mono) o 2 (stereo)
                    // il codice esegue tante iterazioni quante frame_device
                    // frame_device = durata_secondi × 44100
                    // chiamate_callback = frame_device / numero frame file
                    for (p, r) in pos.iter_mut().zip(ratio.iter()) {
                        *p += r ;
                    }
                }
            },
            err_fn,
            None,
        )
    }    
}