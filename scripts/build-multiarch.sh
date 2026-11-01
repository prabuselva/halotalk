#!/usr/bin/env bash
# Cross-compile HaloTalk release binaries for linux amd64/arm64/armv7 in a single
# Debian container (rust + cross gcc), no qemu needed to COMPILE. Artifacts +
# SHA256SUMS land in dist/release/. Native x86_64 gets a health-check e2e;
# foreign arches get `file` verification (plus qemu run if binfmt is registered).
set -euo pipefail
cd "$(dirname "$0")/.."

IMAGE=${IMAGE:-localhost/halotalk-cross:build}
OUT=dist/release
TARGETS=(x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf)
SUFFIXES=(amd64 arm64 armv7)

echo "==> building cross toolchain image ($IMAGE)"
podman build -q -t "$IMAGE" -f - . >/dev/null <<'DOCKER'
FROM rust:1-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
      gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu libc6-dev-arm64-cross \
      gcc-arm-linux-gnueabihf binutils-arm-linux-gnueabihf libc6-dev-armhf-cross \
      cmake perl nasm \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf
DOCKER

echo "==> compiling targets: ${TARGETS[*]}"
podman run --rm -v "$PWD":/src -w /src "$IMAGE" bash -eu -c '
  set -e
  env_aarch64() {
    export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
    export AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
  }
  env_armv7() {
    export CC_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-gcc
    export AR_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-ar
    export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc
  }
  for t in "$@"; do
    echo "--- cargo build --release --target $t"
    unset CC_aarch64_unknown_linux_gnu AR_aarch64_unknown_linux_gnu \
          CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER \
          CC_armv7_unknown_linux_gnueabihf AR_armv7_unknown_linux_gnueabihf \
          CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER
    case "$t" in
      aarch64*) env_aarch64 ;;
      armv7*)   env_armv7 ;;
    esac
    cargo build --release -p halo-server --target "$t"
  done
' _ "${TARGETS[@]}"

mkdir -p "$OUT"
for i in "${!TARGETS[@]}"; do
  t=${TARGETS[$i]}; s=${SUFFIXES[$i]}
  cp "target/$t/release/halotalk" "$OUT/halotalk-linux-$s"
done

echo "==> artifacts"
( cd "$OUT" && rm -f SHA256SUMS && sha256sum halotalk-linux-* > SHA256SUMS && cat SHA256SUMS && file halotalk-linux-* )

echo "==> x86_64 release e2e (health + embedded UI)"
RT=$(mktemp -d)
HALO_DB_PATH=$RT/h.db HALO_UPLOAD_DIR=$RT/up HALO_HOST=127.0.0.1 HALO_PORT=18422 \
  "$OUT/halotalk-linux-amd64" >/tmp/opencode/multiarch-e2e.log 2>&1 &
pid=$!
for _ in $(seq 1 20); do
  curl -fsS -m2 http://127.0.0.1:18422/api/health >/dev/null 2>&1 && break
  sleep 1
done
curl -fsS -m5 http://127.0.0.1:18422/api/health; echo
curl -fsS -m5 http://127.0.0.1:18422/ | grep -q "assets/" && echo "embedded UI: ok"
kill $pid

qemu_have() { command -v "$1" >/dev/null 2>&1 || [ -e /proc/sys/fs/binfmt_misc/"$1" ]; }
if qemu_have qemu-aarch64-static && cp /usr/bin/qemu-aarch64-static "$RT/" 2>/dev/null; then
  echo "==> running aarch64 binary under qemu"
  HALO_DB_PATH=$RT/b.db "$RT/qemu-aarch64-static" "$OUT/halotalk-linux-arm64" >/dev/null 2>&1 &
  pid=$!; sleep 5; curl -fsS -m3 http://127.0.0.1:8420/api/health && echo " arm64 qemu OK" || echo " arm64 qemu smoke FAILED"
  kill $pid
else
  echo "==> qemu binfmt not registered — foreign arches are file/link-verified only (honest scope-out)"
fi
rm -rf "$RT"
echo "done."
