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
        /*samples_a: Vec<f32>,
        samples_b: Vec<f32>, */
        samples: Vec<WavData>,
        samples_a: WavData,
        samples_b: WavData,

    ) -> Result<cpal::Stream, cpal::BuildStreamError> {
        let err_fn = |err| eprintln!("Errore nello stream: {}", err);
        let config = self.config.clone();

        match self.sample_format {
            SampleFormat::F32 => self.create_stream::<f32>(&config,samples, samples_a, samples_b, err_fn),
            SampleFormat::I16 => self.create_stream::<i16>(&config, samples, samples_a, samples_b, err_fn),
            SampleFormat::U16 => self.create_stream::<u16>(&config, samples, samples_a, samples_b, err_fn),
            _ => panic!("Formato non supportato"),
        }
    }

    // Funzione privata helper per gestire i generici
    fn create_stream<T>(
        &self,
        config: &cpal::StreamConfig,
        data_a: WavData,
        data_b: WavData,
        err_fn: impl Fn(cpal::StreamError) + Send + 'static,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: Sample + cpal::SizedSample + FromSample<f32>,
    {
        let channels_device = config.channels as usize;
        let channels_a = data_a.spec.channels as usize;
        let channels_b = data_b.spec.channels as usize;

        // ratio ora tiene conto dei canali: avanziamo nel Vec interleaved
        // di (channels) posizioni per ogni frame
        let ratio_a = (data_a.spec.sample_rate as f64 / config.sample_rate as f64) * channels_a as f64;
        let ratio_b = (data_b.spec.sample_rate as f64 / config.sample_rate as f64) * channels_b as f64;

        let mut pos_a = 0.0f64;
        let mut pos_b = 0.0f64;

        let get_interpolated = |samples: &[f32], pos: f64| -> f32 {
            let idx = pos as usize;
            let s1 = samples.get(idx).copied().unwrap_or(0.0);
            let s2 = samples.get(idx + 1).copied().unwrap_or(s1);
            let t = (pos - idx as f64) as f32;
            s1 + t * (s2 - s1)
        };

        self.device.build_output_stream(
            config,
            move |data: &mut [T], _| {
                for (ch_idx, frame) in data.chunks_mut(channels_device).enumerate() {
                    for (ch, output) in frame.iter_mut().enumerate() {
                        // per ogni canale del device, leggiamo il canale corrispondente
                        // del file (wrappando se il file ha meno canali, es. mono su stereo)
                        let ch_a = ch % channels_a;
                        let ch_b = ch % channels_b;

                        let val_a = get_interpolated(&data_a.samples, pos_a + ch_a as f64);
                        let val_b = get_interpolated(&data_b.samples, pos_b + ch_b as f64);

                        *output = T::from_sample((val_a + val_b) * 0.5);
                    }
                    pos_a += ratio_a;
                    pos_b += ratio_b;
                }
            },
            err_fn,
            None,
        )
    }
}