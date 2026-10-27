use anyhow::{anyhow, Result};
use silero_rs::{VadConfig, VadSession};
use log::{debug, info, warn};
use std::collections::VecDeque;
use std::time::Duration;

/// Silero VAD only operates at 16kHz; input is resampled to this rate, and every
/// sample count and timestamp inside this module is expressed in it.
const VAD_SAMPLE_RATE: u32 = 16000;

/// Silero scores the audio in 30ms frames.
const FRAME_SAMPLES: usize = VAD_SAMPLE_RATE as usize * 30 / 1000;

/// Silero's default thresholds: a frame above the positive threshold starts speech,
/// and a frame below the negative one counts towards the silence that ends it.
const POSITIVE_SPEECH_THRESHOLD: f32 = 0.50;
const NEGATIVE_SPEECH_THRESHOLD: f32 = 0.35;

/// Audio kept before the first and after the last speech frame of a segment.
const PRE_SPEECH_PAD_MS: usize = 300;
const POST_SPEECH_PAD_MS: usize = 400;

/// Speech shorter than this is a false start and is dropped. 250ms keeps out the tiny
/// fragments that Whisper rejects (it needs more than 100ms).
const MIN_SPEECH_MS: usize = 250;

/// A segment that reaches its maximum length is cut at the quietest frame of its
/// last quarter.
const SPLIT_SEARCH_DIVISOR: usize = 4;

fn ms_to_samples(ms: usize) -> usize {
    ms * VAD_SAMPLE_RATE as usize / 1000
}

fn samples_to_ms(samples: usize) -> f64 {
    samples as f64 * 1000.0 / VAD_SAMPLE_RATE as f64
}

/// Represents a complete speech segment detected by VAD
#[derive(Debug, Clone)]
pub struct SpeechSegment {
    pub samples: Vec<f32>,
    pub start_timestamp_ms: f64,
    pub end_timestamp_ms: f64,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SpeechState {
    Silence,
    /// The speech probability crossed the positive threshold, but the speech has not
    /// lasted `MIN_SPEECH_MS` yet.
    Candidate { speech_samples: usize },
    Speech,
}

/// Speech probability and loudness of one frame of the current segment.
struct FrameScore {
    /// Where the frame starts in `current_speech`
    offset: usize,
    probability: f32,
    rms: f32,
}

/// Turns a continuous audio stream into speech segments.
///
/// Silero only scores each 30ms frame; the segmentation itself (padding, minimum speech
/// length, ending a segment after `redemption_time` of silence, maximum length) happens
/// here. Audio is kept only for the segment in progress, so memory stays flat however
/// long the stream runs.
pub struct ContinuousVadProcessor {
    session: VadSession,
    sample_rate: u32,
    redemption_samples: usize,
    max_segment_samples: Option<usize>,
    /// Resampled audio that does not fill a whole frame yet
    buffer: Vec<f32>,
    /// The last `PRE_SPEECH_PAD_MS` of scored audio, for the start of the next segment
    recent: VecDeque<f32>,
    speech_segments: VecDeque<SpeechSegment>,
    state: SpeechState,
    /// Audio of the current segment, from `speech_start_sample` to `processed_samples`
    current_speech: Vec<f32>,
    frame_scores: Vec<FrameScore>,
    processed_samples: usize,
    /// Consecutive samples in frames below the negative threshold
    silent_samples: usize,
    speech_start_sample: usize,
    /// End of the last emitted segment; the pre-speech padding never reaches back past it
    last_segment_end: usize,
    /// While flushing, segments must not extend into the padding of the last frame
    flush_end_sample: Option<usize>,
}

impl ContinuousVadProcessor {
    /// `redemption_time_ms` is how long a silence must last to end a segment. The batch
    /// paths (`import.rs`, `retranscription.rs`) pass 2000ms to bridge natural pauses;
    /// the live path (`pipeline.rs`) passes 500ms to keep latency low.
    pub fn new(input_sample_rate: u32, redemption_time_ms: u32) -> Result<Self> {
        crate::ensure_onnx_runtime_available()?;

        // The session is only used to score frames, so only its sample rate matters
        let config = VadConfig {
            sample_rate: VAD_SAMPLE_RATE as usize,
            ..VadConfig::default()
        };
        let session = VadSession::new(config)
            .map_err(|e| anyhow!("Failed to create VAD session: {:?}", e))?;

        info!(
            "VAD processor created: input={}Hz, vad={}Hz, frame={} samples, redemption={}ms",
            input_sample_rate, VAD_SAMPLE_RATE, FRAME_SAMPLES, redemption_time_ms
        );

        Ok(Self {
            session,
            sample_rate: input_sample_rate, // Store input rate for resampling ratio in resample_to_16k()
            redemption_samples: ms_to_samples(redemption_time_ms as usize),
            max_segment_samples: None,
            buffer: Vec::with_capacity(FRAME_SAMPLES * 2),
            recent: VecDeque::with_capacity(ms_to_samples(PRE_SPEECH_PAD_MS)),
            speech_segments: VecDeque::new(),
            state: SpeechState::Silence,
            current_speech: Vec::new(),
            frame_scores: Vec::new(),
            processed_samples: 0,
            silent_samples: 0,
            speech_start_sample: 0,
            last_segment_end: 0,
            flush_end_sample: None,
        })
    }

    /// Ends segments that reach `max` even if nobody pauses, cutting at the quietest
    /// frame of the segment's last quarter. Without a limit, continuous speech or a
    /// voice in the background keeps one segment open for minutes: nothing is
    /// transcribed meanwhile, and the speech engine then needs gigabytes of memory.
    pub fn with_max_segment(mut self, max: Duration) -> Self {
        self.max_segment_samples = Some(ms_to_samples(max.as_millis() as usize).max(FRAME_SAMPLES * 4));
        self
    }

    /// Process incoming audio samples and return any complete speech segments
    /// Handles resampling from input sample rate to 16kHz for VAD processing
    pub fn process_audio(&mut self, samples: &[f32]) -> Result<Vec<SpeechSegment>> {
        // Resample to 16kHz if needed
        let resampled_audio = if self.sample_rate == VAD_SAMPLE_RATE {
            samples.to_vec()
        } else {
            self.resample_to_16k(samples)?
        };
        self.buffer.extend_from_slice(&resampled_audio);

        let whole_frames = self.buffer.len() / FRAME_SAMPLES * FRAME_SAMPLES;
        if whole_frames > 0 {
            let rest = self.buffer.split_off(whole_frames);
            let ready = std::mem::replace(&mut self.buffer, rest);
            for frame in ready.chunks_exact(FRAME_SAMPLES) {
                let probability = self.speech_probability(frame)?;
                self.advance(frame, probability);
            }
        }

        Ok(self.speech_segments.drain(..).collect())
    }

    fn speech_probability(&mut self, frame: &[f32]) -> Result<f32> {
        let output = self
            .session
            .forward(frame.to_vec())
            .map_err(|e| anyhow!("VAD processing failed: {}", e))?;
        let (_, values) = output
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow!("VAD processing failed: {}", e))?;
        values
            .first()
            .copied()
            .ok_or_else(|| anyhow!("VAD processing failed: no speech probability"))
    }

    /// Improved resampling from input sample rate to 16kHz with anti-aliasing
    /// Uses linear interpolation and basic low-pass filtering for better quality
    fn resample_to_16k(&self, samples: &[f32]) -> Result<Vec<f32>> {
        if self.sample_rate == 16000 {
            return Ok(samples.to_vec());
        }

        // Calculate downsampling ratio
        let ratio = self.sample_rate as f64 / 16000.0;
        let output_len = (samples.len() as f64 / ratio) as usize;
        let mut resampled = Vec::with_capacity(output_len);

        // Apply simple low-pass filter before downsampling to reduce aliasing
        let cutoff_freq = 0.4; // Normalized frequency (0.4 * Nyquist)
        let mut filtered_samples = Vec::with_capacity(samples.len());
        
        // Simple moving average filter (basic low-pass)
        let filter_size = (self.sample_rate as f64 / (cutoff_freq * self.sample_rate as f64)) as usize;
        let filter_size = std::cmp::max(1, std::cmp::min(filter_size, 5)); // Limit filter size
        
        for i in 0..samples.len() {
            let start = if i >= filter_size { i - filter_size } else { 0 };
            let end = std::cmp::min(i + filter_size + 1, samples.len());
            let sum: f32 = samples[start..end].iter().sum();
            filtered_samples.push(sum / (end - start) as f32);
        }

        // Linear interpolation downsampling
        for i in 0..output_len {
            let source_pos = i as f64 * ratio;
            let source_index = source_pos as usize;
            let fraction = source_pos - source_index as f64;
            
            if source_index + 1 < filtered_samples.len() {
                // Linear interpolation
                let sample1 = filtered_samples[source_index];
                let sample2 = filtered_samples[source_index + 1];
                let interpolated = sample1 + (sample2 - sample1) * fraction as f32;
                resampled.push(interpolated);
            } else if source_index < filtered_samples.len() {
                resampled.push(filtered_samples[source_index]);
            }
        }

        debug!("Resampled from {} samples ({}Hz) to {} samples (16kHz) with anti-aliasing",
               samples.len(), self.sample_rate, resampled.len());

        Ok(resampled)
    }

    /// Flush any remaining audio and return final speech segments
    pub fn flush(&mut self) -> Result<Vec<SpeechSegment>> {
        debug!(
            "VAD flush: state={:?}, current_speech_len={}, buffer_len={}, speech_segments_queued={}",
            self.state,
            self.current_speech.len(),
            self.buffer.len(),
            self.speech_segments.len()
        );

        // Preserve the real post-resampling endpoint before padding the final VAD frame.
        let real_end_sample = self.processed_samples + self.buffer.len();
        self.flush_end_sample = Some(real_end_sample);

        if !self.buffer.is_empty() {
            let mut frame = std::mem::take(&mut self.buffer);
            frame.resize(FRAME_SAMPLES, 0.0);
            let probability = self.speech_probability(&frame)?;
            self.advance(&frame, probability);
        }

        // Force-end speech that is still going on, at the real end of the audio
        if self.state == SpeechState::Speech {
            if real_end_sample <= self.speech_start_sample {
                return Err(anyhow!(
                    "VAD flush invariant violated: active speech interval [{}, {}) is empty or reversed",
                    self.speech_start_sample,
                    real_end_sample
                ));
            }
            debug!(
                "VAD flush: force-ending speech - start={}ms, end={}ms",
                samples_to_ms(self.speech_start_sample),
                samples_to_ms(real_end_sample)
            );
            self.finish_segment(real_end_sample, 0.8); // Estimated confidence for forced end
        } else {
            // A candidate that never lasted MIN_SPEECH_MS is dropped
            self.discard_segment();
        }
        self.flush_end_sample = None;

        Ok(self.speech_segments.drain(..).collect())
    }

    /// Advances the segmentation by one scored frame. Mirrors silero-rs' own state
    /// machine (same thresholds, padding and timing), plus the maximum segment length.
    fn advance(&mut self, frame: &[f32], probability: f32) {
        let frame_start = self.processed_samples;
        let frame_end = frame_start + frame.len();
        if probability < NEGATIVE_SPEECH_THRESHOLD {
            self.silent_samples += frame.len();
        } else {
            self.silent_samples = 0;
        }

        match self.state {
            SpeechState::Silence => {
                if probability > POSITIVE_SPEECH_THRESHOLD {
                    let start = frame_start
                        .saturating_sub(ms_to_samples(PRE_SPEECH_PAD_MS))
                        .max(self.last_segment_end);
                    let pad = (frame_start - start).min(self.recent.len());
                    self.current_speech.clear();
                    self.current_speech
                        .extend(self.recent.iter().skip(self.recent.len() - pad));
                    self.frame_scores.clear();
                    self.speech_start_sample = frame_start - pad;
                    self.push_frame(frame, probability);
                    self.state = SpeechState::Candidate { speech_samples: 0 };
                }
            }
            SpeechState::Candidate { speech_samples } => {
                self.push_frame(frame, probability);
                let speech_samples = speech_samples + frame.len();
                if speech_samples > ms_to_samples(MIN_SPEECH_MS) {
                    debug!("VAD: Speech started at {:.0}ms", samples_to_ms(self.speech_start_sample));
                    self.state = SpeechState::Speech;
                } else if probability < NEGATIVE_SPEECH_THRESHOLD {
                    self.discard_segment();
                } else {
                    self.state = SpeechState::Candidate { speech_samples };
                }
            }
            SpeechState::Speech => {
                self.push_frame(frame, probability);
                if self.silent_samples > self.redemption_samples {
                    // Speech ended where the silence began; keep POST_SPEECH_PAD_MS of it
                    let speech_end = frame_end - self.silent_samples;
                    let end = (speech_end + ms_to_samples(POST_SPEECH_PAD_MS)).min(frame_end);
                    self.finish_segment(end, 0.9);
                } else if let Some(max) = self.max_segment_samples {
                    if self.current_speech.len() >= max {
                        self.split_long_segment();
                    }
                }
            }
        }

        self.recent.extend(frame.iter().copied());
        let excess = self.recent.len().saturating_sub(ms_to_samples(PRE_SPEECH_PAD_MS));
        self.recent.drain(..excess);
        self.processed_samples = frame_end;
    }

    fn push_frame(&mut self, frame: &[f32], probability: f32) {
        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
        self.frame_scores.push(FrameScore {
            offset: self.current_speech.len(),
            probability,
            rms,
        });
        self.current_speech.extend_from_slice(frame);
    }

    /// Emits the current segment up to `end_sample` (absolute) and returns to silence.
    fn finish_segment(&mut self, end_sample: usize, confidence: f32) {
        let end_sample = self.flush_end_sample.map_or(end_sample, |limit| end_sample.min(limit));
        let len = end_sample
            .saturating_sub(self.speech_start_sample)
            .min(self.current_speech.len());
        let mut samples = std::mem::take(&mut self.current_speech);
        samples.truncate(len);
        self.emit(samples, confidence);
        self.last_segment_end = self.speech_start_sample + len;
        self.discard_segment();
    }

    fn discard_segment(&mut self) {
        self.current_speech.clear();
        self.frame_scores.clear();
        self.state = SpeechState::Silence;
    }

    /// Ends a segment that reached the maximum length without a pause. It is cut in the
    /// middle of the frame that is least speech-like and quietest (speech probability
    /// plus loudness relative to the loudest frame, averaged with its neighbours) within
    /// the last quarter, which usually falls between two words; the next segment starts
    /// right there, so no audio is lost or duplicated.
    fn split_long_segment(&mut self) {
        let len = self.current_speech.len();
        let search_from = len - len / SPLIT_SEARCH_DIVISOR;
        let window: Vec<usize> = (0..self.frame_scores.len())
            .filter(|&i| {
                let offset = self.frame_scores[i].offset;
                offset >= search_from && offset + FRAME_SAMPLES <= len
            })
            .collect();
        let loudest = window
            .iter()
            .map(|&i| self.frame_scores[i].rms)
            .fold(f32::EPSILON, f32::max);
        let frame_score =
            |j: usize| self.frame_scores[j].probability + self.frame_scores[j].rms / loudest;
        let last = self.frame_scores.len() - 1;
        let score = |i: usize| {
            (frame_score(i.saturating_sub(1)) + 2.0 * frame_score(i) + frame_score((i + 1).min(last)))
                / 4.0
        };
        let cut = window
            .iter()
            .copied()
            .min_by(|&a, &b| score(a).total_cmp(&score(b)))
            .map_or(len, |i| self.frame_scores[i].offset + FRAME_SAMPLES / 2);

        let rest = self.current_speech.split_off(cut);
        let samples = std::mem::replace(&mut self.current_speech, rest);
        info!(
            "VAD: Speech ran {:.1}s without a pause, cutting it at {:.1}s",
            samples_to_ms(len) / 1000.0,
            samples_to_ms(cut) / 1000.0
        );
        self.emit(samples, 0.9);
        self.speech_start_sample += cut;
        self.last_segment_end = self.speech_start_sample;
        self.frame_scores.retain(|score| score.offset >= cut);
        for score in &mut self.frame_scores {
            score.offset -= cut;
        }
    }

    fn emit(&mut self, samples: Vec<f32>, confidence: f32) {
        if samples.is_empty() {
            return;
        }
        let segment = SpeechSegment {
            start_timestamp_ms: samples_to_ms(self.speech_start_sample),
            end_timestamp_ms: samples_to_ms(self.speech_start_sample + samples.len()),
            samples,
            confidence,
        };
        info!(
            "VAD: Completed speech segment: {:.1}ms duration, {} samples",
            segment.end_timestamp_ms - segment.start_timestamp_ms,
            segment.samples.len()
        );
        self.speech_segments.push_back(segment);
    }
}

/// Legacy function for backward compatibility - now uses the optimized approach
pub fn extract_speech_16k(samples_mono_16k: &[f32]) -> Result<Vec<f32>> {
    let mut processor = ContinuousVadProcessor::new(16000, 400)?;

    // Process all audio
    let mut all_segments = processor.process_audio(samples_mono_16k)?;
    let final_segments = processor.flush()?;
    all_segments.extend(final_segments);

    // Concatenate all speech segments
    let mut result = Vec::new();
    let num_segments = all_segments.len();
    for segment in &all_segments {
        result.extend_from_slice(&segment.samples);
    }

    // Apply balanced energy filtering for very short segments
    if result.len() < 1600 { // Less than 100ms at 16kHz
        let input_energy: f32 = samples_mono_16k.iter().map(|&x| x * x).sum::<f32>() / samples_mono_16k.len() as f32;
        let rms = input_energy.sqrt();
        let peak = samples_mono_16k.iter().map(|&x| x.abs()).fold(0.0f32, f32::max);

        // BALANCED FIX: Lowered thresholds to preserve quiet speech while still filtering silence
        // Previous aggressive values (0.08/0.15) were discarding valid quiet speech
        // New values (0.03/0.08) are more balanced - catch quiet speech, reject pure silence
        if rms < 0.2 || peak < 0.20 {
            info!("-----VAD detected silence/noise (RMS: {:.6}, Peak: {:.6}), skipping to prevent hallucinations-----", rms, peak);
            return Ok(Vec::new());
        } else {
            info!("VAD detected speech with sufficient energy (RMS: {:.6}, Peak: {:.6})", rms, peak);
            return Ok(samples_mono_16k.to_vec());
        }
    }

    debug!("VAD: Processed {} samples, extracted {} speech samples from {} segments",
           samples_mono_16k.len(), result.len(), num_segments);

    Ok(result)
}

/// Simple convenience function to get speech chunks from audio
/// Uses the optimized ContinuousVadProcessor with configurable redemption time
pub fn get_speech_chunks(samples_mono_16k: &[f32], redemption_time_ms: u32) -> Result<Vec<SpeechSegment>> {
    get_speech_chunks_with_progress(samples_mono_16k, redemption_time_ms, |_, _| true)
}

/// Get speech chunks with progress callback and cancellation support
/// The callback receives (progress_percent, segments_found) and returns false to cancel
pub fn get_speech_chunks_with_progress<F>(
    samples_mono_16k: &[f32],
    redemption_time_ms: u32,
    mut progress_callback: F,
) -> Result<Vec<SpeechSegment>>
where
    F: FnMut(u32, usize) -> bool,
{
    let mut processor = ContinuousVadProcessor::new(16000, redemption_time_ms)?;

    let total_samples = samples_mono_16k.len();

    // For large files (>1 minute at 16kHz = 960,000 samples), process in chunks with progress logging
    const LARGE_FILE_THRESHOLD: usize = 960_000;
    const CHUNK_SIZE: usize = 160_000; // 10 seconds at 16kHz

    let mut all_segments = Vec::new();

    if total_samples > LARGE_FILE_THRESHOLD {
        info!("VAD: Processing large file ({} samples = {:.1}s), will log progress...",
              total_samples, total_samples as f64 / 16000.0);

        let mut processed = 0;
        let mut last_progress = 0u32;
        let mut chunk_count = 0;
        let total_chunks = (total_samples + CHUNK_SIZE - 1) / CHUNK_SIZE;

        for chunk in samples_mono_16k.chunks(CHUNK_SIZE) {
            chunk_count += 1;

            let start_time = std::time::Instant::now();
            let segments = processor.process_audio(chunk)?;
            let elapsed = start_time.elapsed();

            // Debug log for chunk processing details
            debug!("VAD: Chunk {}/{} processed in {:?}, found {} segments",
                  chunk_count, total_chunks, elapsed, segments.len());

            // Warn if chunk processing took too long (>1 second)
            if elapsed.as_secs() > 1 {
                warn!("VAD: Chunk {} took {:?} - possible performance issue", chunk_count, elapsed);
            }

            all_segments.extend(segments);

            processed += chunk.len();
            let progress = ((processed * 100) / total_samples) as u32;

            // Call progress callback every 5%
            if progress >= last_progress + 5 {
                debug!("VAD: Progress {}% ({} segments found so far)", progress, all_segments.len());

                // Check for cancellation
                if !progress_callback(progress, all_segments.len()) {
                    info!("VAD: Cancelled by callback at {}%", progress);
                    return Err(anyhow!("VAD processing cancelled"));
                }

                last_progress = progress;
            }
        }

        let final_segments = processor.flush()?;
        all_segments.extend(final_segments);

        info!("VAD: Complete! Found {} speech segments", all_segments.len());
    } else {
        // Small file - process all at once
        all_segments = processor.process_audio(samples_mono_16k)?;
        let final_segments = processor.flush()?;
        all_segments.extend(final_segments);
    }

    Ok(all_segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate synthetic speech-like audio with alternating speech/silence
    fn generate_test_audio_with_speech(duration_seconds: f32, sample_rate: u32) -> Vec<f32> {
        let total_samples = (duration_seconds * sample_rate as f32) as usize;
        let mut samples = vec![0.0f32; total_samples];

        // Create speech-like patterns: bursts of sine waves with varying amplitude
        // Speech every 10 seconds for 5 seconds
        let speech_interval = 10.0; // seconds between speech starts
        let speech_duration = 5.0;  // seconds of speech

        for i in 0..total_samples {
            let time = i as f32 / sample_rate as f32;
            let cycle_time = time % speech_interval;

            // Speech occurs in the first `speech_duration` seconds of each cycle
            if cycle_time < speech_duration {
                // Generate speech-like signal: multiple frequencies with amplitude modulation
                let freq1 = 200.0 + (time * 50.0).sin() * 100.0; // Varying fundamental
                let freq2 = freq1 * 2.0; // Harmonic
                let freq3 = freq1 * 3.0; // Another harmonic

                let amplitude = 0.3 + 0.1 * (time * 5.0).sin(); // Amplitude modulation
                samples[i] = amplitude * (
                    0.5 * (2.0 * std::f32::consts::PI * freq1 * time).sin() +
                    0.3 * (2.0 * std::f32::consts::PI * freq2 * time).sin() +
                    0.2 * (2.0 * std::f32::consts::PI * freq3 * time).sin()
                );
            }
            // else: silence (already 0.0)
        }

        samples
    }

    #[test]
    fn test_vad_chunked_vs_single_processing() {
        // Generate 60 seconds of audio with speech patterns at 16kHz
        let audio = generate_test_audio_with_speech(60.0, 16000);
        println!("Generated {} samples ({:.1}s)", audio.len(), audio.len() as f32 / 16000.0);

        // Process all at once (like small files)
        let segments_single = get_speech_chunks(&audio, 2000).expect("Single processing failed");
        println!("Single processing found {} segments", segments_single.len());

        // Process in chunks (like large files)
        let segments_chunked = get_speech_chunks_with_progress(&audio, 2000, |progress, segments| {
            println!("Chunked progress: {}%, {} segments", progress, segments);
            true // Don't cancel
        }).expect("Chunked processing failed");
        println!("Chunked processing found {} segments", segments_chunked.len());

        // Both should find the same number of segments (approximately)
        // Allow some variance due to chunk boundary effects
        let diff = (segments_single.len() as i32 - segments_chunked.len() as i32).abs();
        assert!(diff <= 1,
            "Chunked and single processing found different segment counts: {} vs {} (diff: {})",
            segments_single.len(), segments_chunked.len(), diff);
    }

    #[test]
    fn test_vad_large_file_progress() {
        // Generate 120 seconds (2 minutes) of audio - triggers large file threshold
        let audio = generate_test_audio_with_speech(120.0, 16000);
        let total_samples = audio.len();
        println!("Generated {} samples ({:.1}s)", total_samples, total_samples as f32 / 16000.0);

        // This should trigger the large file path (>960,000 samples)
        assert!(total_samples > 960_000, "Audio should be large enough to trigger chunked processing");

        let mut progress_updates = Vec::new();
        let segments = get_speech_chunks_with_progress(&audio, 2000, |progress, segments| {
            progress_updates.push((progress, segments));
            true // Don't cancel
        }).expect("Processing failed");

        println!("Found {} segments with {} progress updates", segments.len(), progress_updates.len());

        // The synthetic signal is not real speech, so Silero may merge it into
        // one long segment. This test is specifically for the large-file path:
        // it must still emit speech and report monotonic progress through 100%.
        assert!(!segments.is_empty(), "Expected at least one speech segment");
        assert!(
            segments.iter().all(|segment| !segment.samples.is_empty()
                && segment.end_timestamp_ms > segment.start_timestamp_ms),
            "Expected all speech segments to contain audio with positive duration"
        );

        // Should have received progress updates
        assert!(!progress_updates.is_empty(), "Expected progress updates for large file");
        assert_eq!(
            progress_updates.last().map(|(progress, _)| *progress),
            Some(100),
            "Expected progress to reach 100%"
        );
        assert!(
            progress_updates
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0),
            "Expected progress updates to increase monotonically: {:?}",
            progress_updates
        );
    }

    #[test]
    fn test_vad_cancellation() {
        let audio = generate_test_audio_with_speech(120.0, 16000);

        // Cancel at 50%
        let result = get_speech_chunks_with_progress(&audio, 2000, |progress, _| {
            progress < 50 // Cancel when reaching 50%
        });

        // Should return error due to cancellation
        assert!(result.is_err(), "Expected cancellation error");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("cancelled"), "Error should mention cancellation: {}", err_msg);
    }

    #[test]
    fn test_vad_continuous_processor_state_across_chunks() {
        // Test that VAD state is correctly maintained across chunk boundaries
        let mut processor = ContinuousVadProcessor::new(16000, 2000).expect("Failed to create processor");

        // Generate audio with a speech segment that spans a chunk boundary
        let chunk_size = 160_000; // 10 seconds
        let audio = generate_test_audio_with_speech(30.0, 16000); // 30 seconds

        // Process in 10-second chunks
        let mut all_segments = Vec::new();
        for (i, chunk) in audio.chunks(chunk_size).enumerate() {
            let segments = processor.process_audio(chunk).expect("Processing failed");
            println!("Chunk {}: processed {} samples, found {} segments", i, chunk.len(), segments.len());
            all_segments.extend(segments);
        }

        // Flush remaining
        let final_segments = processor.flush().expect("Flush failed");
        all_segments.extend(final_segments);

        println!("Total segments found: {}", all_segments.len());

        // Should find speech segments
        assert!(all_segments.len() >= 1, "Expected at least 1 speech segment");
    }

    #[test]
    fn test_vad_400ms_vs_2000ms_segmentation() {
        // Demonstrates why 2000ms redemption is needed for batch processing:
        // 400ms creates excessive fragmentation, 2000ms bridges natural pauses.
        //
        // Audio pattern: 60s with 5s speech / 5s silence cycles
        // Natural pauses within speech (sentence gaps) are 500ms-1.5s
        let audio = generate_test_audio_with_speech(60.0, 16000);

        let segments_400 = get_speech_chunks(&audio, 400).expect("400ms processing failed");
        let segments_2000 = get_speech_chunks(&audio, 2000).expect("2000ms processing failed");

        println!(
            "400ms redemption: {} segments, 2000ms redemption: {} segments",
            segments_400.len(),
            segments_2000.len()
        );

        // 2000ms should produce fewer or equal segments (bridges more pauses)
        assert!(
            segments_2000.len() <= segments_400.len(),
            "2000ms redemption ({} segments) should not produce more segments than 400ms ({} segments)",
            segments_2000.len(),
            segments_400.len()
        );

        // Verify segments have reasonable durations with 2000ms
        for (i, seg) in segments_2000.iter().enumerate() {
            let duration_ms = seg.end_timestamp_ms - seg.start_timestamp_ms;
            println!("2000ms segment {}: {:.0}ms duration", i, duration_ms);
            // Each segment should be at least 250ms (min_speech_time)
            assert!(duration_ms >= 200.0, "Segment {} too short: {:.0}ms", i, duration_ms);
        }
    }
    /// Leading silence, then speech that runs to the end of the buffer.
    ///
    /// This is the shape that matters for the flush path: an utterance that begins
    /// late in a long session and is still in progress when recording stops.
    fn generate_late_speech_audio(
        silence_seconds: f32,
        speech_seconds: f32,
        sample_rate: u32,
    ) -> Vec<f32> {
        let silence_samples = (silence_seconds * sample_rate as f32) as usize;
        let speech = generate_test_audio_with_speech(speech_seconds, sample_rate);

        let mut samples = vec![0.0f32; silence_samples];
        samples.extend_from_slice(&speech);
        samples
    }

    /// `speech_start_sample` records where the current utterance began, so it can
    /// never point past the number of samples the VAD has actually seen.
    ///
    /// It used to, because it was computed as `processed_samples + timestamp_ms` where
    /// silero's `timestamp_ms` is ALREADY session-absolute
    /// (`processed_duration() - pre_speech_pad`), which doubled the position. The only
    /// reader is the force-end branch in `flush()`, so in production the corruption
    /// escaped as one phantom segment per recording, timestamped past the end of the
    /// audio. The error grows with how late the utterance starts, which is why it took
    /// a long recording to surface.
    #[test]
    fn test_speech_start_sample_never_exceeds_processed_samples() {
        // 20s of silence, then 3s of speech still running when the buffer ends.
        let audio = generate_late_speech_audio(20.0, 3.0, 16000);

        let mut processor =
            ContinuousVadProcessor::new(16000, 2000).expect("Failed to create processor");
        processor
            .process_audio(&audio)
            .expect("process_audio failed");

        assert!(
            processor.state == SpeechState::Speech,
            "expected to still be mid-speech at the end of the buffer; the invariant \
             below would not be exercised otherwise"
        );

        assert!(
            processor.speech_start_sample <= processor.processed_samples,
            "speech_start_sample ({}) is past processed_samples ({}) - \
             session-absolute timestamp double-count regression. \
             In seconds: start={:.2}s vs processed={:.2}s",
            processor.speech_start_sample,
            processor.processed_samples,
            processor.speech_start_sample as f64 / VAD_SAMPLE_RATE as f64,
            processor.processed_samples as f64 / VAD_SAMPLE_RATE as f64,
        );
    }

    /// A forced segment's timestamps and samples must describe the same real audio interval.
    #[test]
    fn test_flush_segment_timestamps_stay_within_audio_duration() {
        let audio = generate_late_speech_audio(20.0, 3.0, 16000);
        let audio_duration_ms = (audio.len() as f64 / VAD_SAMPLE_RATE as f64) * 1000.0;

        assert_eq!(audio.len(), 368_000);
        assert_eq!(
            audio.len() % 480,
            320,
            "fixture must require 160 samples of terminal VAD padding"
        );

        let mut processor =
            ContinuousVadProcessor::new(16000, 2000).expect("Failed to create processor");

        let segments = processor
            .process_audio(&audio)
            .expect("process_audio failed");
        assert!(
            segments.is_empty(),
            "process_audio completed a segment, so flush() would not exercise force-end"
        );
        assert!(
            processor.state == SpeechState::Speech,
            "expected to still be mid-speech before flush()"
        );

        let flushed = processor.flush().expect("flush failed");
        assert_eq!(
            flushed.len(),
            1,
            "force-end must emit exactly one segment"
        );

        let segment = &flushed[0];
        let start_sample = ((segment.start_timestamp_ms / 1000.0)
            * VAD_SAMPLE_RATE as f64)
            .round() as usize;

        assert!(
            segment.start_timestamp_ms <= audio_duration_ms,
            "segment starts at {:.0}ms, beyond the {:.0}ms of audio supplied",
            segment.start_timestamp_ms,
            audio_duration_ms
        );
        assert_eq!(
            segment.end_timestamp_ms, 23_000.0,
            "forced segment must end at the real audio endpoint"
        );
        assert!(
            segment.end_timestamp_ms <= audio_duration_ms,
            "segment ends at {:.0}ms, beyond the {:.0}ms of audio supplied",
            segment.end_timestamp_ms,
            audio_duration_ms
        );
        assert!(
            segment.end_timestamp_ms >= segment.start_timestamp_ms,
            "segment ends before it starts: {:.0}ms -> {:.0}ms",
            segment.start_timestamp_ms,
            segment.end_timestamp_ms
        );
        assert_eq!(
            segment.samples.as_slice(),
            &audio[start_sample..],
            "forced payload must contain the exact real audio interval named by its timestamps"
        );
        assert_eq!(segment.samples.len(), audio.len() - start_sample);

        let timestamp_sample_count = (((segment.end_timestamp_ms
            - segment.start_timestamp_ms)
            / 1000.0)
            * VAD_SAMPLE_RATE as f64)
            .round() as usize;
        assert_eq!(
            timestamp_sample_count,
            segment.samples.len(),
            "timestamp duration and payload length must describe the same sample interval"
        );

        let payload_end_ms = segment.start_timestamp_ms
            + (segment.samples.len() as f64 / VAD_SAMPLE_RATE as f64) * 1000.0;
        assert!(
            (payload_end_ms - segment.end_timestamp_ms).abs() < 0.001,
            "payload ends at {payload_end_ms:.3}ms, timestamp ends at {:.3}ms",
            segment.end_timestamp_ms
        );

        assert!(
            processor.flush().expect("second flush failed").is_empty(),
            "flush() must not emit the same forced segment twice"
        );
    }

    /// Feeds frames with the given speech probabilities straight into the segmentation,
    /// skipping the model. Speech frames are loud, the others quiet.
    fn feed(processor: &mut ContinuousVadProcessor, probabilities: &[f32]) -> Vec<SpeechSegment> {
        for &probability in probabilities {
            let level = if probability > NEGATIVE_SPEECH_THRESHOLD { 0.5 } else { 0.01 };
            processor.advance(&vec![level; FRAME_SAMPLES], probability);
        }
        processor.speech_segments.drain(..).collect()
    }

    fn frames(count: usize, probability: f32) -> Vec<f32> {
        vec![probability; count]
    }

    #[test]
    fn segment_keeps_padding_and_ends_after_the_redemption_time() {
        let mut processor = ContinuousVadProcessor::new(16000, 500).unwrap();
        let mut probabilities = frames(40, 0.05);
        probabilities.extend(frames(60, 0.9));
        probabilities.extend(frames(30, 0.05));

        let segments = feed(&mut processor, &probabilities);

        assert_eq!(segments.len(), 1);
        // Speech starts at frame 40 (1.2s) and stops at frame 100 (3.0s): 300ms of
        // padding before it, 400ms after it
        assert_eq!(segments[0].start_timestamp_ms, 900.0);
        assert_eq!(segments[0].end_timestamp_ms, 3400.0);
        assert_eq!(segments[0].samples.len(), 40_000);
    }

    #[test]
    fn false_starts_are_dropped() {
        let mut processor = ContinuousVadProcessor::new(16000, 500).unwrap();
        let mut probabilities = frames(5, 0.9); // 150ms, below MIN_SPEECH_MS
        probabilities.extend(frames(40, 0.05));

        assert!(feed(&mut processor, &probabilities).is_empty());
        assert!(processor.flush().unwrap().is_empty());
    }

    #[test]
    fn padding_never_overlaps_the_previous_segment() {
        let mut processor = ContinuousVadProcessor::new(16000, 500).unwrap();
        let mut probabilities = frames(30, 0.9);
        probabilities.extend(frames(18, 0.05)); // just over the redemption time
        probabilities.extend(frames(30, 0.9));
        probabilities.extend(frames(20, 0.05));

        let segments = feed(&mut processor, &probabilities);

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].start_timestamp_ms, segments[0].end_timestamp_ms);
    }

    #[test]
    fn continuous_speech_is_cut_at_the_quietest_frame() {
        let mut processor = ContinuousVadProcessor::new(16000, 500)
            .unwrap()
            .with_max_segment(Duration::from_secs(20));
        // Speech starts at frame 10, so the segment starts at sample 0 (with padding).
        // One less speech-like, quieter frame sits at about 17s, inside the last quarter.
        let quiet_frame = 10 + 556;
        for i in 0..10 + 4000 {
            let (level, probability) = if i < 10 {
                (0.01, 0.05)
            } else if i == quiet_frame {
                (0.1, 0.45)
            } else {
                (0.5, 0.9)
            };
            processor.advance(&vec![level; FRAME_SAMPLES], probability);
        }
        let mut segments: Vec<SpeechSegment> = processor.speech_segments.drain(..).collect();
        segments.extend(processor.flush().unwrap());

        // The first cut lands in the middle of the quiet frame
        let quiet_frame_middle = quiet_frame * FRAME_SAMPLES + FRAME_SAMPLES / 2;
        assert_eq!(segments[0].start_timestamp_ms, 0.0);
        assert_eq!(segments[0].samples.len(), quiet_frame_middle);
        // 4000 frames of speech (120s) arrive in pieces of at most 20s, back to back,
        // without losing or repeating audio
        assert!(segments.len() >= 6);
        for pair in segments.windows(2) {
            assert_eq!(pair[1].start_timestamp_ms, pair[0].end_timestamp_ms);
        }
        let max_samples = ms_to_samples(20_000);
        assert!(segments.iter().all(|s| s.samples.len() <= max_samples));
        let total: usize = segments.iter().map(|s| s.samples.len()).sum();
        assert_eq!(total, (10 + 4000) * FRAME_SAMPLES);
    }

    #[test]
    fn memory_stays_flat_during_long_silence_and_speech() {
        let mut processor = ContinuousVadProcessor::new(16000, 500)
            .unwrap()
            .with_max_segment(Duration::from_secs(20));

        feed(&mut processor, &frames(20_000, 0.02)); // 10 minutes of silence
        assert!(processor.recent.len() <= ms_to_samples(PRE_SPEECH_PAD_MS));
        assert!(processor.current_speech.is_empty());

        feed(&mut processor, &frames(20_000, 0.9)); // 10 minutes without a pause
        assert!(processor.current_speech.len() <= ms_to_samples(20_000));
        assert!(processor.frame_scores.len() <= ms_to_samples(20_000) / FRAME_SAMPLES + 1);
    }
}
