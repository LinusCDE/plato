#!/bin/bash
# Build Plato for the reMarkable 2 the same way Vellum's VELBUILD does,
# inside ghcr.io/toltec-dev/rust:v4.0.
#
#   build.sh full   - apply VELBUILD seds, build thirdparty libs + plato, dist
#   build.sh plato  - sync crates/ from the source tree and rebuild only plato
#
# Source edits live in this checkout; the build happens in
# $PLATO_BUILD (default ~/plato-build) (worktree at 0.9.44-rm-release-14 with VELBUILD seds).
set -euo pipefail

SRC=${PLATO_SRC:-$(cd "$(dirname "$0")/../.." && pwd)}
BLD=${PLATO_BUILD:-$HOME/plato-build}
MODE=${1:-plato}
IMAGE=ghcr.io/toltec-dev/rust:v4.0

# Keep the build tree's Rust sources in sync with the edited tree.
rsync -a --delete "$SRC/crates/" "$BLD/crates/" \
    --exclude 'core/build.rs'
cp "$SRC/Cargo.toml" "$SRC/Cargo.lock" "$BLD/"
# The C side of the MuPDF bindings (two-core page rendering lives there).
if ! cmp -s "$SRC/mupdf_wrapper/mupdf_wrapper.c" "$BLD/mupdf_wrapper/mupdf_wrapper.c"; then
    cp "$SRC/mupdf_wrapper/mupdf_wrapper.c" "$BLD/mupdf_wrapper/"
    touch "$BLD/.wrapper-changed"
fi

read -r -d '' INNER <<'EOF' || true
set -e
. /opt/x-tools/switch-arm.sh
export CFLAGS="-mfpu=neon -mfloat-abi=hard -mcpu=cortex-a9 -D_TIME_BITS=64 -D_FILE_OFFSET_BITS=64 -O2 -pipe -g -feliminate-unused-debug-types -gdwarf-4"
export CARGO_HOME=/work/.cargo-home
# The image's cargo config sets the ARM linker; keep it with our CARGO_HOME.
mkdir -p "$CARGO_HOME"
cp /root/.cargo/config* "$CARGO_HOME/"
# Generate code for the rM2's Cortex-A7 (enables NEON and VFPv4) instead of
# generic ARMv7. The image only passes -mcpu to the linker.
sed -i '/^\[target.armv7-unknown-linux-gnueabihf\]/,/^\[/ s/^rustflags = \[/rustflags = [\n    "-C", "target-cpu=cortex-a7",/' "$CARGO_HOME"/config*
cd /work
if [ ! -f .velbuild-prepared ]; then
    apt-get update -qq
    apt-get install -y -qq autoconf jq libtool patchelf unzip wget >/dev/null
    sed -i "s/version=.*$/version=\'$(cargo pkgid -p plato | cut -d "#" -f 2 | cut -d ":" -f 2)\'/g" download.sh
    sed -i "s/cross build/cargo build/g" build.sh
    sed -i 's/armv7-unknown-linux-gnueabihf/$CARGO_BUILD_TARGET/g' build.sh dist.sh
    sed -i 's/zlib-1\.3\.1/zlib-1.3.2/g' thirdparty/download.sh
    sed -i 's/git ls-files/true #/g' thirdparty/download.sh
    sed -i "s/arm-linux-gnueabihf/${CHOST}/g" thirdparty/mupdf/kobo.patch thirdparty/bzip2/Makefile-kobo thirdparty/*/build-kobo.sh
    sed -i "s/-mcpu=cortex-a9 -mfpu=neon/${CFLAGS}/g" thirdparty/mupdf/kobo.patch thirdparty/bzip2/Makefile-kobo thirdparty/*/build-kobo.sh
    sed -i "s/'\${CFLAGS}'/\"\${CFLAGS}\"/g" thirdparty/*/build-kobo.sh
    sed -i "s/armv7-unknown-linux-gnueabihf/${CARGO_BUILD_TARGET}/" crates/core/build.rs
    rm mupdf_wrapper/build-kobo.sh
    ln -s build.sh mupdf_wrapper/build-kobo.sh
    sh -e ./clean.sh
    touch .velbuild-prepared
fi
export TARGET_OS=Kobo AR=${CROSS_COMPILE}ar CC=${CROSS_COMPILE}gcc CXX=${CROSS_COMPILE}g++ STRIP=${CROSS_COMPILE}strip
if [ "$MODE" = full ]; then
    env -u srcdir sh -e ./build.sh slow
    sh -ex ./dist.sh
else
    if [ -f .wrapper-changed ]; then
        (cd mupdf_wrapper && CFLAGS="-O2 -fPIC -mcpu=cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard" ./build-kobo.sh)
        # Cargo doesn't track the static library: make it relink.
        touch crates/core/src/lib.rs
        rm .wrapper-changed
    fi
    cargo build --release --target "$CARGO_BUILD_TARGET" -p plato
    # Only the binary changes; libs, fonts and icons are already on the tablet.
    ${STRIP} -o target/plato-stripped "target/$CARGO_BUILD_TARGET/release/plato"
fi
chown -R "$HOST_UID:$HOST_GID" /work
EOF

${DOCKER:-docker} run --rm \
    -e MODE="$MODE" -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    -v "$BLD:/work" "$IMAGE" bash -c "$INNER"

ls -la "$BLD/target/armv7-unknown-linux-gnueabihf/release/plato"
