//! Memory and segmentation probes for long recordings (ignored; run by hand):
//!
//! TRINOTE_MODELS_DIR=~/.local/share/<id>/models TRINOTE_BENCH_SAMPLES=<dir with manifest.json>
//! cargo test --features vulkan --lib memory_probe -- --ignored --nocapture --test-threads=1

use std::path::PathBuf;

/// (VmRSS, VmHWM) of this process in MB.
fn rss_mb() -> (f64, f64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |key: &str| {
        status
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
            / 1024.0
    };
    (field("VmRSS:"), field("VmHWM:"))
}

fn read_wav_16k_mono(path: &std::path::Path) -> Vec<f32> {
    let bytes = std::fs::read(path).unwrap();
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        if id == b"data" {
            return bytes[pos + 8..pos + 8 + size]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                .collect();
        }
        pos += 8 + size + (size & 1);
    }
    panic!("no data chunk in {}", path.display());
}

/// The benchmark meeting as one 16 kHz stream, with a 0.6 s pause between utterances.
fn meeting_audio() -> Vec<f32> {
    let dir = PathBuf::from(std::env::var("TRINOTE_BENCH_SAMPLES").expect("TRINOTE_BENCH_SAMPLES"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    let mut audio = Vec::new();
    for entry in manifest.as_array().unwrap() {
        audio.extend(read_wav_16k_mono(&dir.join(entry["file"].as_str().unwrap())));
        audio.extend(std::iter::repeat(0.0f32).take(9_600));
    }
    audio
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn memory_probe_device_enumeration() {
    let (start, _) = rss_mb();
    println!("device enumeration: start RSS {:.1} MB", start);
    for i in 1..=600 {
        let _ = crate::audio::devices::list_audio_devices().await;
        if i % 100 == 0 {
            let (rss, _) = rss_mb();
            println!("  after {:>3} enumerations (= {:>2} min of recording): RSS {:.1} MB (+{:.1})", i, i * 2 / 60, rss, rss - start);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn memory_probe_parakeet_segment_length() {
    let models_dir = PathBuf::from(std::env::var("TRINOTE_MODELS_DIR").expect("TRINOTE_MODELS_DIR"));
    let audio = meeting_audio();
    let parakeet = crate::parakeet_engine::ParakeetEngine::new_with_models_dir(Some(models_dir)).unwrap();
    parakeet.discover_models().await.unwrap();
    parakeet.load_model("parakeet-tdt-0.6b-v3-int8").await.unwrap();
    let (loaded, _) = rss_mb();
    println!("Parakeet loaded: RSS {:.1} MB, meeting audio {:.0} s", loaded, audio.len() as f64 / 16000.0);

    // One segment of each length, shortest first, so each new VmHWM is that length's peak
    for seconds in [5usize, 15, 30, 60, 120, 240, 340] {
        let len = (seconds * 16_000).min(audio.len());
        let started = std::time::Instant::now();
        let text = parakeet.transcribe_audio(audio[..len].to_vec()).await.unwrap();
        let (rss, peak) = rss_mb();
        println!(
            "  segment {:>3} s: {:>5.1} s to transcribe, RSS after {:>6.1} MB, peak {:>6.1} MB (+{:.0} over loaded), {} chars",
            seconds, started.elapsed().as_secs_f64(), rss, peak, peak - loaded, text.len()
        );
    }

    // Many sentence-sized segments of varying length: does memory keep rising?
    let mut seed = 12345u64;
    let (before, _) = rss_mb();
    for i in 1..=200 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let seconds = 1.0 + (seed >> 33) as f64 / (1u64 << 31) as f64 * 19.0;
        let len = (seconds * 16_000.0) as usize;
        let start = (seed as usize >> 7) % (audio.len() - len);
        parakeet.transcribe_audio(audio[start..start + len].to_vec()).await.unwrap();
        if i % 40 == 0 {
            let (rss, _) = rss_mb();
            println!("  after {:>3} segments of 1-20 s: RSS {:.1} MB ({:+.1} since the series started)", i, rss, rss - before);
        }
    }
}

/// Segment lengths the live VAD settings produce, for normal turn-taking and for two
/// people talking over each other (or a voice in the background), which leaves no pauses.
#[test]
#[ignore]
fn memory_probe_vad_segment_lengths() {
    let audio = meeting_audio();
    let overlap_offset = 7 * 16_000;
    let crosstalk: Vec<f32> = (0..audio.len())
        .map(|i| (audio[i] + 0.5 * audio[(i + overlap_offset) % audio.len()]).clamp(-1.0, 1.0))
        .collect();

    for (name, input) in [("turn-taking", &audio), ("crosstalk", &crosstalk)] {
        let mut vad = crate::audio::vad::ContinuousVadProcessor::new(16_000, 500)
            .unwrap()
            .with_max_segment(std::time::Duration::from_secs(20));
        let mut durations = Vec::new();
        for window in input.chunks(9_600) {
            // 600 ms windows, like the live pipeline
            for segment in vad.process_audio(window).unwrap() {
                durations.push((segment.end_timestamp_ms - segment.start_timestamp_ms) / 1000.0);
            }
        }
        for segment in vad.flush().unwrap() {
            durations.push((segment.end_timestamp_ms - segment.start_timestamp_ms) / 1000.0);
        }
        let mut sorted = durations.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pick = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
        println!(
            "{:<12} {:.0} s of audio -> {} segments: median {:.1} s, p90 {:.1} s, longest {:.1} s",
            name,
            input.len() as f64 / 16000.0,
            durations.len(),
            pick(0.5),
            pick(0.9),
            sorted.last().copied().unwrap_or(0.0)
        );
    }
}

/// Writes every segment boundary (ms) for the benchmark meeting to TRINOTE_VAD_DUMP, for
/// comparing two VAD implementations.
#[test]
#[ignore]
fn memory_probe_vad_boundaries_dump() {
    let audio = meeting_audio();
    let overlap_offset = 7 * 16_000;
    let crosstalk: Vec<f32> = (0..audio.len())
        .map(|i| (audio[i] + 0.5 * audio[(i + overlap_offset) % audio.len()]).clamp(-1.0, 1.0))
        .collect();
    let mut out = String::new();
    for (name, input) in [("turn-taking", &audio), ("crosstalk", &crosstalk)] {
        for redemption in [500u32, 2000] {
            let mut vad = crate::audio::vad::ContinuousVadProcessor::new(16_000, redemption).unwrap();
            let mut segments = Vec::new();
            for window in input.chunks(9_600) {
                segments.extend(vad.process_audio(window).unwrap());
            }
            segments.extend(vad.flush().unwrap());
            for s in &segments {
                out.push_str(&format!(
                    "{name} {redemption} {:.0} {:.0} {}\n",
                    s.start_timestamp_ms, s.end_timestamp_ms, s.samples.len()
                ));
            }
        }
    }
    std::fs::write(std::env::var("TRINOTE_VAD_DUMP").expect("TRINOTE_VAD_DUMP"), out).unwrap();
}

/// Prints the words on both sides of every forced cut (a segment that starts exactly where
/// the previous one ended), to check that the cuts fall between words.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn memory_probe_vad_cut_points() {
    let models_dir = PathBuf::from(std::env::var("TRINOTE_MODELS_DIR").expect("TRINOTE_MODELS_DIR"));
    let audio = meeting_audio();
    let overlap_offset = 7 * 16_000;
    let crosstalk: Vec<f32> = (0..audio.len())
        .map(|i| (audio[i] + 0.5 * audio[(i + overlap_offset) % audio.len()]).clamp(-1.0, 1.0))
        .collect();
    let parakeet = crate::parakeet_engine::ParakeetEngine::new_with_models_dir(Some(models_dir)).unwrap();
    parakeet.discover_models().await.unwrap();
    parakeet.load_model("parakeet-tdt-0.6b-v3-int8").await.unwrap();

    for (name, input) in [("turn-taking", &audio), ("crosstalk", &crosstalk)] {
        let mut vad = crate::audio::vad::ContinuousVadProcessor::new(16_000, 500)
            .unwrap()
            .with_max_segment(std::time::Duration::from_secs(20));
        let mut segments = Vec::new();
        for window in input.chunks(9_600) {
            segments.extend(vad.process_audio(window).unwrap());
        }
        segments.extend(vad.flush().unwrap());
        let mut texts = Vec::new();
        for s in &segments {
            texts.push(parakeet.transcribe_audio(s.samples.clone()).await.unwrap());
        }
        println!("== {name}");
        for i in 1..segments.len() {
            if segments[i].start_timestamp_ms == segments[i - 1].end_timestamp_ms {
                let before: Vec<&str> = texts[i - 1].split_whitespace().collect();
                let after: Vec<&str> = texts[i].split_whitespace().collect();
                println!(
                    "  cut at {:>6.1}s: ...{} | {}...",
                    segments[i].start_timestamp_ms / 1000.0,
                    before[before.len().saturating_sub(5)..].join(" "),
                    after[..after.len().min(5)].join(" ")
                );
            }
        }
    }
}
