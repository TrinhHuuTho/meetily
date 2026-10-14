# Releasing TriNote

Releases are built by GitHub Actions. Pushing a version tag builds the Linux packages and creates a **draft** release; nothing is published until you publish the draft.

## Workflows

| Workflow | Runs on | Does |
| --- | --- | --- |
| [`ci.yml`](../.github/workflows/ci.yml) | Pushes to `main`, pull requests | Frontend type check, Rust unit tests (CPU build), test of the portability check |
| [`release-linux.yml`](../.github/workflows/release-linux.yml) | `v*` tags, or started by hand | Builds the `.deb` and AppImage with Vulkan on Ubuntu 22.04, checks portability, writes `SHA256SUMS`, and for tags creates a draft release |

Started by hand (Actions → Release (Linux) → Run workflow), the release workflow only uploads the packages as a build artifact. Use that for test builds.

The release build does not tune native code for the CI machine. `.github/force-portable-ggml.cmake` turns off `GGML_NATIVE` for whisper.cpp, and `.github/verify-portable-ggml.cjs` fails the job if whisper.cpp was built with `GGML_NATIVE` or AVX-512. The packages need an x86-64 CPU with AVX2.

## Steps

1. **Version.** Set the new version in all three places:
   - `frontend/src-tauri/tauri.conf.json` (`version`)
   - `frontend/src-tauri/Cargo.toml` (`version`)
   - `frontend/package.json` (`version`)
2. **Changelog.** In `CHANGELOG.md`, replace `Unreleased` with today's date under the new version and add the link at the bottom. The release notes are taken from this section.
3. **Docs.** Make sure the README, `docs/` and `PRIVACY_POLICY.md` match the release.
4. **Merge and tag.**

   ```bash
   git checkout main
   git merge --ff-only release/<version>
   git push origin main
   git tag v<version>
   git push origin v<version>
   ```

5. **Wait for the build.** Release (Linux) takes about an hour.
6. **Test the draft.** Download the `.deb` and AppImage from the draft release and check them against `SHA256SUMS`. At minimum:
   - install the `.deb` on a clean Ubuntu 22.04 or 24.04 system
   - record a short meeting with microphone and system audio
   - transcribe it in the English / Vietnamese / Chinese mode
   - generate a summary and export a PDF
7. **Publish** the draft on GitHub.

## Windows and macOS

Meetily's Windows and macOS workflows (code signing, Vulkan/Metal builds, portable Windows binaries) were removed in commit `8b1d131` because they depend on Meetily's signing certificates and services. They are the starting point for TriNote's Windows and macOS releases: `git show 8b1d131^:.github/workflows/build-windows.yml`.
