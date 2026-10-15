#!/usr/bin/env bash
# Installs a TriNote .deb in clean Ubuntu containers and checks that apt resolves its
# dependencies, that every binary finds its shared libraries, and that the sidecars run.
# It does not start the app window; test that on a real desktop.
#
# Usage: .github/scripts/smoke-test-deb.sh path/to/TriNote_x.y.z_amd64.deb [ubuntu:22.04 ubuntu:24.04 ...]
# Needs Docker (prefix with sudo if your user is not in the docker group).
set -euo pipefail

deb="$(realpath "$1")"
shift
if [ $# -gt 0 ]; then images=("$@"); else images=(ubuntu:22.04 ubuntu:24.04); fi

failed=0
for image in "${images[@]}"; do
  echo "=== $image ==="
  if docker run --rm -v "$deb:/tmp/trinote.deb:ro" "$image" bash -euo pipefail -c '
      export DEBIAN_FRONTEND=noninteractive
      apt-get update -qq
      apt-get install -y -qq /tmp/trinote.deb > /dev/null
      dpkg -s trinote | grep -E "^(Package|Version|Depends):"
      for bin in /usr/bin/trinote /usr/bin/trinote-ffmpeg /usr/bin/trinote-llama-helper; do
        test -x "$bin" || { echo "missing $bin"; exit 1; }
        if ldd "$bin" | grep "not found"; then echo "unresolved libraries in $bin"; exit 1; fi
      done
      trinote-ffmpeg -hide_banner -version > /tmp/ffmpeg-version && head -1 /tmp/ffmpeg-version
      # The helper reads requests from stdin; with none it must start (loading
      # llama.cpp and Vulkan without a GPU) and exit cleanly
      timeout 20 trinote-llama-helper < /dev/null
      echo "OK"
    '; then
    echo "=== $image: passed ==="
  else
    echo "=== $image: FAILED ==="
    failed=1
  fi
done
exit "$failed"
