# Contributing to TriNote

Thanks for helping improve TriNote. Bug reports, fixes, translations of the docs and new features are all welcome.

## Reporting bugs and requesting features

Open an issue at <https://github.com/TrinhHuuTho/trinote/issues>. For bugs, please include:

- TriNote version and how you installed it (`.deb`, AppImage or source build)
- Your Linux distribution and GPU
- What you did, what you expected and what happened
- The log, if you can: start TriNote from a terminal with `RUST_LOG=info trinote` and copy the output around the problem

Please remove anything private from logs and screenshots first; transcripts can end up in them.

Problems that also exist in [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) can be reported there as well.

## Making changes

1. Fork the repository and clone your fork.
2. Set up the build as described in [docs/building_in_linux.md](docs/building_in_linux.md).
3. Create a branch from `main`:
   - `fix/<short-name>` for bug fixes
   - `enhance/<short-name>` for features and improvements
4. Make your change, with tests where it makes sense.
5. Check it:

   ```bash
   cd frontend && ./node_modules/.bin/tsc --noEmit
   cd src-tauri && cargo test --features vulkan --lib
   ```

6. Update the docs your change affects: the README, `docs/`, and [CHANGELOG.md](CHANGELOG.md) under the next version.
7. Open a pull request against `main` and describe what changed and how you tested it.

## Code guidelines

- Follow the style of the surrounding code.
- New app behavior belongs in the Rust core (`frontend/src-tauri/src`), exposed through Tauri commands and events. [docs/architecture.md](docs/architecture.md) describes the structure.
- Keep TriNote local-first. Do not add analytics, tracking, or network calls the user did not ask for. A feature that sends data off the computer must be opt-in and documented in [PRIVACY_POLICY.md](PRIVACY_POLICY.md).
- For speech recognition or GPU changes, run the benchmarks described in [docs/GPU_ACCELERATION.md](docs/GPU_ACCELERATION.md) and put the numbers in the pull request.

## Commit messages

TriNote uses [Conventional Commits](https://www.conventionalcommits.org/):

```text
<type>(<scope>): <summary>

<why the change is needed and what it does>
```

Common types are `feat`, `fix`, `perf`, `docs`, `test` and `chore`, for example `fix(audio): capture Linux system audio through PipeWire`.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE.md).
