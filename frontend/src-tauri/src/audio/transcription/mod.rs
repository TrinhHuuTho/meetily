// audio/transcription/mod.rs
//
// Transcription module: engine management and worker pool.

pub mod error;
pub mod engine;
pub mod worker;

// Re-export commonly used types
pub use error::TranscriptionError;
pub use engine::{
    TranscriptionEngine,
    validate_transcription_model_ready,
    get_or_init_transcription_engine,
    get_or_init_whisper
};
pub use worker::{
    start_transcription_task,
    reset_speech_detected_flag,
    TranscriptUpdate
};
