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
/// MEETILY_MODELS_DIR=~/.local/share/<id>/models MEETILY_BENCH_SAMPLES=<dir with manifest.json>
/// cargo test -p meetily --features cuda --lib bench_language_modes -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn bench_language_modes() {
    use std::path::PathBuf;
    use std::time::Instant;

    let models_dir = PathBuf::from(std::env::var("MEETILY_MODELS_DIR").expect("MEETILY_MODELS_DIR"));
    let samples_dir = PathBuf::from(std::env::var("MEETILY_BENCH_SAMPLES").expect("MEETILY_BENCH_SAMPLES"));
    let whisper_model =
        std::env::var("MEETILY_BENCH_WHISPER_MODEL").unwrap_or_else(|_| "large-v3-turbo-q5_0".into());

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

    // Parakeet baseline (current default engine)
    let parakeet = crate::parakeet_engine::ParakeetEngine::new_with_models_dir(Some(models_dir.clone())).unwrap();
    parakeet.discover_models().await.unwrap();
    parakeet.load_model("parakeet-tdt-0.6b-v3-int8").await.unwrap();
    let _ = parakeet.transcribe_audio(segments[0].2.clone()).await; // warm-up
    let start = Instant::now();
    for (_, _, audio) in &segments {
        parakeet.transcribe_audio(audio.clone()).await.unwrap();
    }
    let parakeet_secs = start.elapsed().as_secs_f64();
    parakeet.unload_model().await;

    let whisper = super::WhisperEngine::new_with_models_dir(Some(models_dir)).unwrap();
    whisper.discover_models().await.unwrap();
    whisper.load_model(&whisper_model).await.unwrap();
    let _ = whisper.transcribe_audio_with_confidence(segments[0].2.clone(), Some("en".into())).await; // warm-up
    let _ = whisper.transcribe_audio_with_confidence(segments[0].2.clone(), Some("auto:en,vi,zh".into())).await;

    let mut rows = vec![("parakeet (CPU)".to_string(), parakeet_secs, "-".to_string())];
    let mut outputs: Vec<Vec<String>> = Vec::new();
    for (label, preference, fit) in [
        ("fixed, full ctx", None, false),
        ("fixed, fitted ctx", None, true),
        ("auto:en,vi,zh, fitted ctx", Some("auto:en,vi,zh"), true),
    ] {
        whisper.set_fit_audio_ctx(fit);
        let start = Instant::now();
        let mut correct = 0;
        let mut texts = Vec::new();
        for (_, expected, audio) in &segments {
            let pref = preference.map(str::to_string).unwrap_or_else(|| expected.clone());
            let result = whisper.transcribe_audio_with_confidence(audio.clone(), Some(pref)).await.unwrap();
            if result.language.as_deref() == Some(expected.as_str()) {
                correct += 1;
            }
            texts.push(result.text);
        }
        rows.push((label.to_string(), start.elapsed().as_secs_f64(), format!("{correct}/{}", segments.len())));
        outputs.push(texts);
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
    for (name, secs, accuracy) in rows {
        println!("{:<28} {:>9.2} {:>8.3} {:>10}", name, secs, secs / audio_seconds, accuracy);
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
