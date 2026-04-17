mod audio;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Data, Sample, SampleFormat, FromSample};

fn main() {
    // apriamo un wav con hound
    let mut reader_a = hound::WavReader::open("/Users/generalkenobi/LocalDocuments/Rust/audio/gong.wav").expect("Failed to open first WAV file");
    let spec_a = reader_a.spec();
    let sample_a: Vec<f32> = match spec_a.sample_format {
        hound::SampleFormat::Float => reader_a
            .samples::<f32>()
            .map(|s| s.unwrap())
            .collect(),
        hound::SampleFormat::Int => {
            let max_val = (1_i32 << (spec_a.bits_per_sample - 1)) as f32;
            reader_a
                .samples::<i32>()
                .map(|s| s.unwrap() as f32 / max_val)
                .collect()
        }
    };

    // apriamo un secondo wav con hound
    let mut reader_b = hound::WavReader::open("/Users/generalkenobi/LocalDocuments/Rust/audio/laughter.wav").expect("Failed to open second WAV file");
    let spec_b = reader_b.spec();
    let sample_b: Vec<f32> = match spec_b.sample_format {
        hound::SampleFormat::Float => reader_b
            .samples::<f32>()
            .map(|s| s.unwrap())
            .collect(),
        hound::SampleFormat::Int => {
            let max_val_b = (1_i32 << (spec_b.bits_per_sample - 1)) as f32;
            reader_b
                .samples::<i32>()
                .map(|s| s.unwrap() as f32 / max_val_b)
                .collect()
        }
    };

    // inizializziamo host e device cpal
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device available");
    
    // impostiamo configurazione dello stream
    let mut supported_configs_range = device.supported_output_configs()
        .expect("error while querying configs");
    let supported_config = supported_configs_range.next()
        .expect("no supported config?!")
        .with_max_sample_rate();



    let spec_b = reader_b.spec();
    let spec_a = reader_a.spec();

    let sample_format = supported_config.sample_format();

    // controlliamo che siano uguali
    assert_eq!(
        spec_a.sample_rate, spec_b.sample_rate,
        "i due file hanno sample rate diversi: {} vs {}",
        spec_a.sample_rate, spec_b.sample_rate
    );
    assert_eq!(
        spec_a.channels, spec_b.channels,
        "i due file hanno canali diversi: {} vs {}",
        spec_a.channels, spec_b.channels
    );

    let config = cpal::StreamConfig {
        channels: spec_a.channels as u16,
        sample_rate: spec_a.sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    // verifica che il device supporti il sample rate del file
    /* let supported = device.supported_output_configs()
        .unwrap()
        .any(|c| {
            c.min_sample_rate() <= spec.sample_rate 
            && spec.sample_rate <= c.max_sample_rate()
        });

    if !supported {
        panic!("il device non supporta {} Hz", spec.sample_rate);
    } */

    // debug
    println!("file:   {} Hz, {} canali", spec_a.sample_rate, spec_a.channels);
    println!("device: {} Hz, {} canali", config.sample_rate, config.channels);

    
    let err_fn = |err| eprintln!("an error occurred on the output audio stream: {}", err);
    let stream = match sample_format {
        SampleFormat::F32 => build_stream::<f32>(&device, &config, &sample_a, &sample_b, err_fn),
        SampleFormat::I16 => build_stream::<i16>(&device, &config, &sample_a, &sample_b, err_fn),
        SampleFormat::U16 => build_stream::<u16>(&device, &config, &sample_a, &sample_b, err_fn),
        fmt => panic!("Formato non supportato {}", fmt),
    }.unwrap();
    // costruiamo lo stream
    /* let stream = match sample_format {
        SampleFormat::F32 => build_sine_stream::<f32>(&device, &config, sample_rate, channels, err_fn),
        SampleFormat::I16 => build_sine_stream::<i16>(&device, &config, sample_rate, channels, err_fn),
        SampleFormat::U16 => build_sine_stream::<u16>(&device, &config, sample_rate, channels, err_fn),
        fmt => panic!("Formato non supportato {}", fmt),
    }.unwrap();

    // creiamo uno stream di silenzio
    let stream = match sample_format {
        SampleFormat::F32 => device.build_output_stream(&config, write_silence::<f32>, err_fn, None),
        SampleFormat::I16 => device.build_output_stream(&config, write_silence::<i16>, err_fn, None),
        SampleFormat::U16 => device.build_output_stream(&config, write_silence::<u16>, err_fn, None),
        sample_format => panic!("Unsupported sample format '{sample_format}'")
    }.unwrap();

    fn write_silence<T: Sample>(data: &mut [T], _: &cpal::OutputCallbackInfo) {
        for sample in data.iter_mut() {
            *sample = Sample::EQUILIBRIUM;
        }
    } */



    stream.play().unwrap();

    // aspettiamo che finisca (calcolo approssimativo della durata)
    if sample_a.len() >= sample_b.len() {
        let duration_secs = sample_a.len() as f64 / (spec_a.sample_rate as f64 * spec_a.channels as f64);
        std::thread::sleep(std::time::Duration::from_secs(duration_secs as u64 + 1));
    } else {
        let duration_secs = sample_b.len() as f64 / (spec_b.sample_rate as f64 * spec_b.channels as f64);
        std::thread::sleep(std::time::Duration::from_secs(duration_secs as u64 + 1));
    };

}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples_a: &[f32],
    samples_b: &[f32],
    err_fn: impl Fn(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: Sample + cpal::SizedSample + FromSample<f32>,
{
    let mut position = 0_usize;
    let sample_a = samples_a.to_vec();
    let sample_b = samples_b.to_vec(); 

    device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            for output in data.iter_mut() {
                /* if position < sample.len() {
                    *output = T::from_sample(sample[position]);
                    position += 1;
                }
                else {
                    *output = Sample::EQUILIBRIUM;
                } */
                let a = sample_a.get(position).copied().unwrap_or(0.0);
                let b = sample_b.get(position).copied().unwrap_or(0.0);

                let mixed = (a + b) / 2.0; // <-- mixing con normalizzazione
                *output = T::from_sample(mixed);
                position += 1;
            }
        },
        err_fn,
        None,
    )
}

fn build_sine_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_rate: f32,
    channels: usize,
    err_fn: impl Fn(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: Sample + cpal::SizedSample + FromSample<f32>,
{
    let frequency = 440.0_f32; // La nota A4

    // questo è il contatore di campioni che persiste tra una callback e l'altra
    let mut sample_clock = 0_u32;

    device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            write_sine::<T>(data, channels, &mut sample_clock, sample_rate, frequency);
        },
        err_fn,
        None,
    )
}

fn write_sine<T>(
    data: &mut [T],
    channels: usize,
    sample_clock: &mut u32,
    sample_rate: f32,
    frequency: f32,
)
where
    T: Sample + FromSample<f32>,
{
    // data è un buffer "piatto": [L, R, L, R, L, R, ...]
    // i campioni sono interleaved per canale
    for frame in data.chunks_mut(channels) {
        // sinusoide ha formula 2π * f * t in cui:
        // f è la frequenza (quante volte al secondo oscilla il valore della sinusoide)
        // t si calcola capendo il numero del sample che abbiamo (es il 50 esimo) diviso il numero di sample (per es 4000)
        // quindi t al 50 esimo sample è 50/4000 = 0,0125 s
        // sapendo poi la freq (es 440) e moltiplicando per 2π troviamo value
        let value = (2.0 * std::f32::consts::PI * frequency * (*sample_clock as f32) / sample_rate).sin();

        // sample clock abbiamo visto essere il punto della sinusoide di cui ci serve il valore, quindi ora che abbiamo trovato value passiamo a al prossimo sample_clock
        *sample_clock = sample_clock.wrapping_add(1);

        // scriviamo lo stesso valore su tutti i canali (mono su stereo)
        let sample = T::from_sample(value); // value è f32, ci serve nel formato di T (può essere i16 per es.). from_sample fa questo in automatico
        for channel_sample in frame.iter_mut() { // channel = 2 se stereo, 1 se mono. inserisco tot sample (es 2 se channel = 2) che abbiamo trovato (ossia value della sinusoide) in base a com'è fatto il buffer che verrà letto
            *channel_sample = sample;
        }
    }
}