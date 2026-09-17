use std::{
    collections::HashMap,
    io::Cursor,
    path::Path,
    sync::Arc,
    thread,
};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use jbonsai::{Condition, Engine};
use jpreprocess::{
    kind::JPreprocessDictionaryKind, JPreprocess, SystemDictionaryConfig,
};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tracing::{error, info};
use utoipa::ToSchema;

use crate::{error::ApiError, models::{load_config, voice_path}};

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SynthesisRequest {
    #[schema(min_length = 1, max_length = 500, example = "こんにちは")]
    pub text: String,

    #[schema(example = "mei_normal")]
    pub voice: String,

    #[schema(minimum = 0.5, maximum = 2.0, example = 1.0)]
    pub speed: Option<f64>,

    /// Pitch shift in semitones.
    #[schema(minimum = -12.0, maximum = 12.0, example = 0.0)]
    pub pitch: Option<f64>,

    /// HTS all-pass constant. Omit to use the value embedded in the voice model.
    #[schema(minimum = 0.0, maximum = 1.0)]
    pub alpha: Option<f64>,

    /// HTS postfilter coefficient.
    #[schema(minimum = 0.0, maximum = 1.0, example = 0.0)]
    pub beta: Option<f64>,

    /// Output gain in dB.
    #[schema(minimum = -40.0, maximum = 20.0, example = 0.0)]
    pub volume_db: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct VoiceDefaults {
    pub sampling_frequency: usize,
    pub frame_period: usize,
    pub speed: f64,
    pub pitch: f64,
    pub alpha: f64,
    pub beta: f64,
    pub volume_db: f64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct VoiceInfo {
    pub id: String,
    pub speaker: String,
    pub style: String,
    pub display_name: String,
    pub defaults: VoiceDefaults,
}

#[derive(Clone)]
struct LoadedVoice {
    info: VoiceInfo,
    engine: Engine,
    defaults: Condition,
}

struct Job {
    request: SynthesisRequest,
    response: oneshot::Sender<Result<Vec<u8>, WorkerError>>,
}

#[derive(Debug, thiserror::Error)]
enum WorkerError {
    #[error("unknown voice: {0}")]
    UnknownVoice(String),
    #[error("text preprocessing failed: {0}")]
    Preprocess(String),
    #[error("speech synthesis failed: {0}")]
    Synthesis(String),
    #[error("WAV encoding failed: {0}")]
    Wav(String),
}

/// Bounded CPU worker pool for JPreprocess + JBonsai.
///
/// Axum/Tokio remains asynchronous. CPU-heavy text preprocessing and synthesis
/// run on dedicated OS threads, and the bounded channel provides backpressure.
pub struct TtsService {
    sender: Sender<Job>,
    voices: Arc<Vec<VoiceInfo>>,
    ready: bool,
}

impl TtsService {
    pub fn load(models_config: impl AsRef<Path>, workers: usize, queue_capacity: usize) -> anyhow::Result<Self> {
        let models_config = models_config.as_ref();
        let base_voices = load_voices(models_config)?;
        let ready = !base_voices.is_empty();

        if !ready {
            tracing::warn!(path = %models_config.display(), "no enabled voice models configured; speech endpoint will return 503");
        }

        let voice_infos = Arc::new(base_voices.iter().map(|voice| voice.info.clone()).collect());
        let (sender, receiver) = crossbeam_channel::bounded(queue_capacity.max(1));

        if ready {
            let workers = workers.max(1);
            for worker_id in 0..workers {
                spawn_worker(worker_id, receiver.clone(), base_voices.clone())?;
            }
            info!(workers, queue_capacity, voices = base_voices.len(), "TTS worker pool initialized");
        }

        Ok(Self {
            sender,
            voices: voice_infos,
            ready,
        })
    }

    pub fn voices(&self) -> Vec<VoiceInfo> {
        self.voices.as_ref().clone()
    }

    pub async fn synthesize(&self, request: SynthesisRequest) -> Result<Vec<u8>, ApiError> {
        validate(&request)?;

        if !self.ready {
            return Err(ApiError::NotReady);
        }

        if !self.voices.iter().any(|voice| voice.id == request.voice) {
            return Err(ApiError::UnknownVoice(request.voice));
        }

        let (response_tx, response_rx) = oneshot::channel();
        let job = Job {
            request,
            response: response_tx,
        };

        match self.sender.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(ApiError::QueueFull),
            Err(TrySendError::Disconnected(_)) => return Err(ApiError::NotReady),
        }

        match response_rx.await {
            Ok(Ok(wav)) => Ok(wav),
            Ok(Err(WorkerError::UnknownVoice(voice))) => Err(ApiError::UnknownVoice(voice)),
            Ok(Err(error)) => {
                error!(%error, "TTS worker failed");
                Err(ApiError::SynthesisFailed)
            }
            Err(_) => Err(ApiError::NotReady),
        }
    }
}

fn spawn_worker(worker_id: usize, receiver: Receiver<Job>, voices: Vec<LoadedVoice>) -> anyhow::Result<()> {
    // Build the dictionary before spawning so startup fails immediately if the
    // embedded dictionary cannot be initialized.
    let dictionary = SystemDictionaryConfig::Bundled(JPreprocessDictionaryKind::NaistJdic)
        .load()
        .map_err(|e| anyhow::anyhow!("failed to load bundled NAIST-JDIC: {e}"))?;
    let jpreprocess = JPreprocess::with_dictionaries(dictionary, None);

    thread::Builder::new()
        .name(format!("koeserve-tts-{worker_id}"))
        .spawn(move || {
            let mut engines: HashMap<String, (Engine, Condition)> = voices
                .into_iter()
                .map(|voice| (voice.info.id, (voice.engine, voice.defaults)))
                .collect();

            info!(worker_id, "TTS worker started");
            while let Ok(job) = receiver.recv() {
                // A dropped receiver means the HTTP request was cancelled. We can
                // cheaply skip work while the job is still waiting in the queue.
                if job.response.is_closed() {
                    continue;
                }

                let result = synthesize_job(&jpreprocess, &mut engines, &job.request);
                let _ = job.response.send(result);
            }
            info!(worker_id, "TTS worker stopped");
        })?;

    Ok(())
}

fn synthesize_job(
    jpreprocess: &JPreprocess<jpreprocess::DefaultTokenizer>,
    engines: &mut HashMap<String, (Engine, Condition)>,
    request: &SynthesisRequest,
) -> Result<Vec<u8>, WorkerError> {
    let (engine, defaults) = engines
        .get_mut(&request.voice)
        .ok_or_else(|| WorkerError::UnknownVoice(request.voice.clone()))?;

    // Reset every request so options from a previous request never leak into
    // the next synthesis performed by this worker.
    engine.condition = defaults.clone();

    if let Some(speed) = request.speed {
        engine.condition.set_speed(speed);
    }
    if let Some(pitch) = request.pitch {
        engine.condition.set_additional_half_tone(pitch);
    }
    if let Some(alpha) = request.alpha {
        engine.condition.set_alpha(alpha);
    }
    if let Some(beta) = request.beta {
        engine.condition.set_beta(beta);
    }
    if let Some(volume_db) = request.volume_db {
        engine.condition.set_volume(volume_db);
    }

    let labels = jpreprocess
        .extract_fullcontext(&request.text)
        .map_err(|e| WorkerError::Preprocess(e.to_string()))?;

    let samples = engine
        .synthesize(labels)
        .map_err(|e| WorkerError::Synthesis(e.to_string()))?;

    encode_wav(&samples, engine.condition.get_sampling_frequency())
}

fn encode_wav(samples: &[f64], sample_rate: usize) -> Result<Vec<u8>, WorkerError> {
    let mut cursor = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sample_rate as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| WorkerError::Wav(e.to_string()))?;
        for &sample in samples {
            // JBonsai uses HTS-compatible PCM amplitude values (i16-scale), not
            // normalized -1.0..=1.0 samples.
            let sample = sample.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16;
            writer
                .write_sample(sample)
                .map_err(|e| WorkerError::Wav(e.to_string()))?;
        }
        writer
            .finalize()
            .map_err(|e| WorkerError::Wav(e.to_string()))?;
    }

    Ok(cursor.into_inner())
}

fn load_voices(config_path: &Path) -> anyhow::Result<Vec<LoadedVoice>> {
    let config = load_config(config_path)?;
    let mut voices = Vec::new();

    for voice in config.voices.into_iter().filter(|voice| voice.enabled) {
        let path = voice_path(config_path, &voice);
        if !path.is_file() {
            anyhow::bail!(
                "configured voice file does not exist: {} (voice id: {})",
                path.display(),
                voice.id
            );
        }

        let engine = Engine::load([&path])
            .map_err(|e| anyhow::anyhow!("failed to load {}: {e}", path.display()))?;
        let defaults = engine.condition.clone();
        let info = VoiceInfo {
            id: voice.id,
            speaker: voice.speaker,
            style: voice.style,
            display_name: voice.display_name,
            defaults: VoiceDefaults {
                sampling_frequency: defaults.get_sampling_frequency(),
                frame_period: defaults.get_fperiod(),
                speed: defaults.get_speed(),
                pitch: defaults.get_additional_half_tone(),
                alpha: defaults.get_alpha(),
                beta: defaults.get_beta(),
                volume_db: defaults.get_volume(),
            },
        };

        info!(voice = %info.id, path = %path.display(), "loaded configured HTS voice");
        voices.push(LoadedVoice {
            info,
            engine,
            defaults,
        });
    }

    voices.sort_by(|a, b| a.info.id.cmp(&b.info.id));
    Ok(voices)
}

fn validate(request: &SynthesisRequest) -> Result<(), ApiError> {
    let chars = request.text.chars().count();
    if chars == 0 || chars > 500 {
        return Err(ApiError::InvalidRequest(
            "text must contain between 1 and 500 characters".into(),
        ));
    }

    if request.voice.trim().is_empty() {
        return Err(ApiError::InvalidRequest("voice must not be empty".into()));
    }

    if let Some(speed) = request.speed {
        if !(0.5..=2.0).contains(&speed) {
            return Err(ApiError::InvalidRequest("speed must be between 0.5 and 2.0".into()));
        }
    }

    if let Some(pitch) = request.pitch {
        if !(-12.0..=12.0).contains(&pitch) {
            return Err(ApiError::InvalidRequest("pitch must be between -12 and 12".into()));
        }
    }

    for (name, value) in [("alpha", request.alpha), ("beta", request.beta)] {
        if let Some(value) = value {
            if !(0.0..=1.0).contains(&value) {
                return Err(ApiError::InvalidRequest(format!(
                    "{name} must be between 0.0 and 1.0"
                )));
            }
        }
    }

    if let Some(volume_db) = request.volume_db {
        if !(-40.0..=20.0).contains(&volume_db) {
            return Err(ApiError::InvalidRequest(
                "volume_db must be between -40 and 20".into(),
            ));
        }
    }

    Ok(())
}
