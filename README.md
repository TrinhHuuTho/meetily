<p align="center">
  <img src="frontend/src-tauri/icons/128x128@2x.png" width="96" alt="TriNote icon">
</p>

<h1 align="center">TriNote</h1>

<p align="center">
  Private meeting notes for English, Vietnamese and Chinese.<br>
  Record, transcribe and summarize meetings on your own computer.
</p>

<p align="center">
  <a href="https://github.com/TrinhHuuTho/trinote/releases/latest">Download</a> ·
  <a href="docs/GPU_ACCELERATION.md">Hardware guide</a> ·
  <a href="CHANGELOG.md">Changelog</a> ·
  <a href="PRIVACY_POLICY.md">Privacy</a>
</p>

TriNote is a fork of [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) by Zackriya Solutions, built for meetings that switch between English, Vietnamese and Chinese. [CONTRIBUTIONS.md](CONTRIBUTIONS.md) explains what the fork changed and why.

## Features

- **Live transcription on your computer** with Parakeet or Whisper. Audio is never uploaded.
- **Mixed-language meetings.** Each speech segment is detected as English, Vietnamese or Chinese and transcribed in that language, with EN/VI/ZH labels in the transcript.
- **Summaries** with a built-in local model (Qwen 3.5), a local Ollama server, or a cloud provider you choose (OpenAI, Anthropic, Groq, OpenRouter or any OpenAI-compatible server).
- **Records both sides of a call:** your microphone and your computer's audio (PipeWire on Linux).
- **PDF export** of the summary and the timestamped transcript, with Vietnamese and Chinese text.
- **Uses your GPU automatically** (NVIDIA, AMD or Intel through Vulkan) and falls back to the CPU when there is none.
- **No account, no analytics, no update checks.** See the [privacy policy](PRIVACY_POLICY.md).

## Install

TriNote 1.0 is available for Linux on x86_64 and is built for Ubuntu 22.04 and newer. It needs a CPU with AVX2, which most Intel Core CPUs since 2013 and AMD CPUs since 2015 have; some Pentium and Celeron models do not. Windows and macOS builds are planned.

Download the `.deb` or the AppImage from the [latest release](https://github.com/TrinhHuuTho/trinote/releases/latest).

```bash
# Debian / Ubuntu package
sudo apt install ./TriNote_1.0.0_amd64.deb

# or the AppImage, which runs without installing
chmod +x TriNote_1.0.0_amd64.AppImage
./TriNote_1.0.0_amd64.AppImage
```

The AppImage needs FUSE 2 (`sudo apt install libfuse2t64`, or `libfuse2` on Ubuntu 22.04). Without it, start it with `./TriNote_1.0.0_amd64.AppImage --appimage-extract-and-run`. To uninstall the package, run `sudo apt remove trinote`; your meetings stay in the folders listed in the [privacy policy](PRIVACY_POLICY.md).

To record your computer's audio, TriNote needs PipeWire's `pw-record` (package `pipewire-bin`), which Ubuntu 22.10 and newer use by default. For GPU acceleration you need a Vulkan driver: NVIDIA's proprietary driver, or `mesa-vulkan-drivers` for AMD and Intel.

## Getting started

1. **First launch.** The setup downloads Parakeet for speech recognition and a built-in summary model.
2. **Vietnamese or Chinese meetings.** Parakeet only knows English and other European languages. Open **Settings → Transcription**, switch to **Local Whisper**, and download a model:
   - with a GPU: `large-v3-turbo-q5_0`
   - on the CPU only: `small-q5_1`

   On the main screen, click the globe button above the transcript and choose **Auto Detect (English / Vietnamese / Chinese)**. Download the small `base` model when TriNote offers it; it keeps language detection fast.
3. **Record.** Pick your microphone and system audio, then start recording. The transcript appears as people speak.
4. **After the meeting.** Generate a summary, edit it, and export it as PDF or copy it as Markdown.

The [hardware guide](docs/GPU_ACCELERATION.md) has measured speeds and model recommendations for machines with and without a GPU.

## Known limitations

- Linux only for now.
- System audio needs PipeWire. Ubuntu 22.04 uses PulseAudio by default, so only the microphone is recorded there unless you switch to PipeWire.
- Very short or silent segments sometimes come out as wrong or invented text, a known Whisper weakness.
- Language detection can pick the wrong language when two languages sound similar in a segment, for example with strong accents.
- Speakers are not identified.

## Build from source

See [docs/building_in_linux.md](docs/building_in_linux.md). [docs/architecture.md](docs/architecture.md) gives an overview of the code.

The whole app, including the Rust core in `frontend/src-tauri`, lives in `frontend/`. The folder keeps its Meetily name, from when Meetily also had a separate Python backend, so that changes from upstream still apply cleanly.

## Contributing

Bug reports and pull requests are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) first.

## Credits

TriNote is built on [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) by Zackriya Solutions and its contributors. It also relies on:

- [whisper.cpp](https://github.com/ggml-org/whisper.cpp) and [llama.cpp](https://github.com/ggml-org/llama.cpp) for local inference
- [Parakeet](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) by NVIDIA, with the ONNX conversion by [istupakov](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx)
- [Qwen](https://huggingface.co/Qwen) models for built-in summaries
- [Tauri](https://tauri.app), [Screenpipe](https://github.com/mediar-ai/screenpipe) and [transcribe-rs](https://crates.io/crates/transcribe-rs), which Meetily borrowed code from
- [Noto Sans](https://fonts.google.com/noto) fonts (SIL Open Font License) for PDF export
- [FFmpeg](https://ffmpeg.org) to encode recordings and decode imported audio. Linux packages include a static FFmpeg build from [johnvansickle.com](https://johnvansickle.com/ffmpeg/), licensed under the GPLv3; its license and source code are linked from that page.

## License

MIT. See [LICENSE.md](LICENSE.md). Copyright © 2024 Zackriya Solutions (Meetily) and © 2026 Trinh Huu Tho (TriNote).
