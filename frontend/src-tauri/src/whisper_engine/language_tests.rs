// Tests for language selection, plus an opt-in benchmark comparing engines/modes.

use super::whisper_engine::LanguageMode;

#[test]
fn parses_language_preferences() {
    assert_eq!(LanguageMode::parse(None), LanguageMode::Auto);
    assert_eq!(LanguageMode::parse(Some("auto")), LanguageMode::Auto);
    assert_eq!(LanguageMode::parse(Some("auto-translate")), LanguageMode::AutoTranslate);
    assert_eq!(LanguageMode::parse(Some("vi")), LanguageMode::Fixed("vi".to_string()));
    assert_eq!(
        LanguageMode::parse(Some("auto:en, VI,zh")),
        LanguageMode::Restricted(vec!["en".into(), "vi".into(), "zh".into()])
    );
}

#[test]
fn restricted_preference_drops_unknown_codes() {
    assert_eq!(
        LanguageMode::parse(Some("auto:en,xx")),
        LanguageMode::Fixed("en".to_string())
    );
    assert_eq!(LanguageMode::parse(Some("auto:xx")), LanguageMode::Auto);
}

#[test]
fn picks_most_likely_allowed_language() {
    let mut probs = vec![0.0f32; whisper_rs::get_lang_max_id() as usize + 1];
    probs[whisper_rs::get_lang_id("ja").unwrap() as usize] = 0.6; // not allowed
    probs[whisper_rs::get_lang_id("zh").unwrap() as usize] = 0.3;
    probs[whisper_rs::get_lang_id("vi").unwrap() as usize] = 0.1;
    let codes = vec!["en".to_string(), "vi".to_string(), "zh".to_string()];
    assert_eq!(super::whisper_engine::pick_language(&probs, &codes), Some("zh".to_string()));
}

#[test]
fn fits_encoder_context_to_short_segments_only() {
    use super::whisper_engine::fitted_audio_ctx;
    assert_eq!(fitted_audio_ctx(4.0), Some(200 + 128));
    assert_eq!(fitted_audio_ctx(26.0), Some(1300 + 128));
    assert_eq!(fitted_audio_ctx(29.0), None);
}

/// Benchmark live-style per-segment transcription on the production engine.
///
/// Run with:
/// TRINOTE_MODELS_DIR=~/.local/share/<id>/models TRINOTE_BENCH_SAMPLES=<dir with manifest.json>
/// cargo test -p trinote --features cuda --lib bench_language_modes -- --ignored --nocapture
///
/// Optional: TRINOTE_BENCH_SKIP_PARAKEET=1 skips the CPU Parakeet baseline (e.g. when only
/// comparing GPU backends), TRINOTE_BENCH_OUT=<file.json> saves timings and transcripts.
#[tokio::test]
#[ignore]
async fn bench_language_modes() {
    use std::path::PathBuf;
    use std::time::Instant;

    let models_dir = PathBuf::from(std::env::var("TRINOTE_MODELS_DIR").expect("TRINOTE_MODELS_DIR"));
    let samples_dir = PathBuf::from(std::env::var("TRINOTE_BENCH_SAMPLES").expect("TRINOTE_BENCH_SAMPLES"));
    let whisper_model =
        std::env::var("TRINOTE_BENCH_WHISPER_MODEL").unwrap_or_else(|_| "large-v3-turbo-q5_0".into());

    let manifest: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(samples_dir.join("manifest.json")).unwrap()).unwrap();
    let segments: Vec<(String, String, Vec<f32>)> = manifest
        .iter()
        .map(|m| {
            let file = m["file"].as_str().unwrap();
            (
                file.to_string(),
                m["lang"].as_str().unwrap().to_string(),
                read_wav_16k_mono(&samples_dir.join(file)),
            )
        })
        .collect();
    let audio_seconds: f64 = segments.iter().map(|s| s.2.len() as f64 / 16000.0).sum();
    println!("\n{} segments, {:.1}s of speech\n", segments.len(), audio_seconds);

    let mut rows = Vec::new();

    // Parakeet baseline (current default engine)
    if std::env::var_os("TRINOTE_BENCH_SKIP_PARAKEET").is_none() {
        let parakeet = crate::parakeet_engine::ParakeetEngine::new_with_models_dir(Some(models_dir.clone())).unwrap();
        parakeet.discover_models().await.unwrap();
        parakeet.load_model("parakeet-tdt-0.6b-v3-int8").await.unwrap();
        let _ = parakeet.transcribe_audio(segments[0].2.clone()).await; // warm-up
        let start = Instant::now();
        for (_, _, audio) in &segments {
            parakeet.transcribe_audio(audio.clone()).await.unwrap();
        }
        rows.push(("parakeet (CPU)".to_string(), start.elapsed().as_secs_f64(), "-".to_string()));
        parakeet.unload_model().await;
    }

    let whisper = super::WhisperEngine::new_with_models_dir(Some(models_dir)).unwrap();
    whisper.discover_models().await.unwrap();
    whisper.load_model(&whisper_model).await.unwrap();
    let _ = whisper.transcribe_audio_with_confidence(segments[0].2.clone(), Some("en".into())).await; // warm-up
    let _ = whisper.transcribe_audio_with_confidence(segments[0].2.clone(), Some("auto:en,vi,zh".into())).await;

    let mut outputs: Vec<Vec<String>> = Vec::new();
    let mut languages: Vec<Vec<Option<String>>> = Vec::new();
    for (label, preference, fit) in [
        ("fixed, full ctx", None, false),
        ("fixed, fitted ctx", None, true),
        ("auto:en,vi,zh, fitted ctx", Some("auto:en,vi,zh"), true),
    ] {
        whisper.set_fit_audio_ctx(fit);
        let start = Instant::now();
        let mut correct = 0;
        let mut texts = Vec::new();
        let mut detected = Vec::new();
        for (_, expected, audio) in &segments {
            let pref = preference.map(str::to_string).unwrap_or_else(|| expected.clone());
            let result = whisper.transcribe_audio_with_confidence(audio.clone(), Some(pref)).await.unwrap();
            if result.language.as_deref() == Some(expected.as_str()) {
                correct += 1;
            }
            texts.push(result.text);
            detected.push(result.language);
        }
        rows.push((label.to_string(), start.elapsed().as_secs_f64(), format!("{correct}/{}", segments.len())));
        outputs.push(texts);
        languages.push(detected);
    }

    // Quality: how much the fitted-context transcripts differ from full-context ones
    println!("Segments where fitted ctx differs from full ctx (word similarity < 0.9):");
    let mut similarity_sum = 0.0;
    for (i, (file, _, _)) in segments.iter().enumerate() {
        let sim = word_similarity(&outputs[0][i], &outputs[1][i]);
        similarity_sum += sim;
        if sim < 0.9 {
            println!("  {file} sim={sim:.2}\n    full: {}\n    fit : {}", outputs[0][i], outputs[1][i]);
        }
    }
    println!("Mean word similarity full vs fitted: {:.3}\n", similarity_sum / segments.len() as f64);

    println!("{:<28} {:>9} {:>8} {:>10}", "engine / mode", "seconds", "RTF", "lang ok");
    for (name, secs, accuracy) in &rows {
        println!("{:<28} {:>9.2} {:>8.3} {:>10}", name, secs, secs / audio_seconds, accuracy);
    }

    if let Ok(out) = std::env::var("TRINOTE_BENCH_OUT") {
        let report = serde_json::json!({
            "audio_seconds": audio_seconds,
            "rows": rows.iter().map(|(name, secs, accuracy)| serde_json::json!({
                "mode": name, "seconds": secs, "rtf": secs / audio_seconds, "lang_ok": accuracy,
            })).collect::<Vec<_>>(),
            "segments": segments.iter().enumerate().map(|(i, (file, expected, _))| serde_json::json!({
                "file": file,
                "expected": expected,
                "full_ctx": outputs[0][i],
                "fitted_ctx": outputs[1][i],
                "auto": outputs[2][i],
                "auto_language": languages[2][i],
            })).collect::<Vec<_>>(),
        });
        std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
        println!("Saved report to {out}");
    }
}

/// Print the detector's en/vi/zh probabilities per segment on GPU and CPU, to see how close
/// detection decisions are. Same env vars as `bench_language_modes`; set
/// TRINOTE_BENCH_DETECTOR to pick the detector model file (default ggml-base.bin).
#[test]
#[ignore]
fn bench_language_detection_margins() {
    use std::path::PathBuf;
    use whisper_rs::{WhisperContext, WhisperContextParameters};

    let models_dir = PathBuf::from(std::env::var("TRINOTE_MODELS_DIR").expect("TRINOTE_MODELS_DIR"));
    let samples_dir = PathBuf::from(std::env::var("TRINOTE_BENCH_SAMPLES").expect("TRINOTE_BENCH_SAMPLES"));
    let detector = std::env::var("TRINOTE_BENCH_DETECTOR").unwrap_or_else(|_| "ggml-base.bin".into());
    let manifest: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(samples_dir.join("manifest.json")).unwrap()).unwrap();

    let gpu = super::acceleration::preferred_gpu(super::acceleration::gpu_devices());
    let contexts: Vec<(&str, WhisperContext)> = [("gpu", true), ("cpu", false)]
        .into_iter()
        .map(|(label, use_gpu)| {
            let params = WhisperContextParameters {
                use_gpu,
                gpu_device: gpu.map_or(0, |d| d.index),
                flash_attn: false,
                ..Default::default()
            };
            (label, WhisperContext::new_with_params(models_dir.join(&detector), params).unwrap())
        })
        .collect();

    let ids: Vec<(String, usize)> = ["en", "vi", "zh"]
        .iter()
        .map(|c| (c.to_string(), whisper_rs::get_lang_id(c).unwrap() as usize))
        .collect();
    println!("\n{:<12} {:<4} {:>22} {:>22}", "file", "exp", "gpu en/vi/zh", "cpu en/vi/zh");
    for m in &manifest {
        let file = m["file"].as_str().unwrap();
        let audio = read_wav_16k_mono(&samples_dir.join(file));
        let mut cols = Vec::new();
        for (_, ctx) in &contexts {
            let mut state = ctx.create_state().unwrap();
            state.pcm_to_mel(&audio, 4).unwrap();
            let (_, probs) = state.lang_detect(0, 4).unwrap();
            cols.push(ids.iter().map(|(_, id)| format!("{:.2}", probs[*id])).collect::<Vec<_>>().join("/"));
        }
        println!("{:<12} {:<4} {:>22} {:>22}", file, m["lang"].as_str().unwrap(), cols[0], cols[1]);
    }
}

/// Bag-of-words overlap (characters for CJK) between two transcripts, 0..1
fn word_similarity(a: &str, b: &str) -> f64 {
    use std::collections::HashMap;
    let tokens = |s: &str| -> HashMap<String, usize> {
        let mut map = HashMap::new();
        for word in s.split_whitespace() {
            let word: String = word.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
            if word.chars().any(|c| (c as u32) >= 0x2E80) {
                for c in word.chars() {
                    *map.entry(c.to_string()).or_insert(0) += 1;
                }
            } else if !word.is_empty() {
                *map.entry(word).or_insert(0) += 1;
            }
        }
        map
    };
    let (ta, tb) = (tokens(a), tokens(b));
    let common: usize = ta.iter().map(|(k, v)| (*v).min(*tb.get(k).unwrap_or(&0))).sum();
    let total = ta.values().sum::<usize>().max(tb.values().sum::<usize>());
    if total == 0 { 1.0 } else { common as f64 / total as f64 }
}

/// Minimal reader for 16-bit PCM mono WAV files at 16kHz
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
