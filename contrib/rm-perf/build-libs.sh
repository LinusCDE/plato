#!/bin/bash
# Rebuild libmupdf.so for the rM2 with:
#  - Cortex-A7 tuning, NEON/VFPv4 and -O3 (Vellum builds -O2 for cortex-a9)
#  - libjpeg-turbo (NEON SIMD) linked statically with hidden symbols, so
#    DjVu keeps using the bundled libjpeg.so.9 without symbol clashes.
# Requires a prior `build.sh full` (it reuses the configured thirdparty tree).
set -euo pipefail

BLD=${PLATO_BUILD:-$HOME/plato-build}
IMAGE=ghcr.io/toltec-dev/rust:v4.0

# Reduced-resolution and multi-threaded JPEG 2000 decoding.
python3 "$(dirname "$0")/patch-mupdf.py" "$BLD/thirdparty/mupdf"

read -r -d '' INNER <<'EOF' || true
set -e
. /opt/x-tools/switch-arm.sh
OPT="-O3 -mcpu=cortex-a7 -mfpu=neon-vfpv4 -mfloat-abi=hard"

# libjpeg-turbo, static and position independent.
T=/work/thirdparty/libjpeg-turbo-src/libjpeg-turbo-3.0.4
rm -rf "$T/build" && mkdir -p "$T/build" && cd "$T/build"
cmake -G "Unix Makefiles" \
    -DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=arm \
    -DCMAKE_C_COMPILER=${CROSS_COMPILE}gcc \
    -DCMAKE_C_FLAGS="$OPT" -DCMAKE_BUILD_TYPE=Release \
    -DENABLE_SHARED=OFF -DENABLE_STATIC=ON -DWITH_TURBOJPEG=OFF \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DWITH_SIMD=ON \
    -DCMAKE_INSTALL_PREFIX="$T/install" .. > /dev/null
make -j"$(nproc)" install > /dev/null
grep -q "WITH_SIMD 1" "$T/install/include/jconfig.h" && echo "libjpeg-turbo: SIMD enabled"

# MuPDF with A7 tuning, compiled against libjpeg-turbo's headers.
cd /work/thirdparty/mupdf
sed -i -E '/^ifeq "\$\(OS\)" "kobo"/,/^endif/ s/^(  CFLAGS \+= )-O2 -fPIC .*-mcpu=cortex-a9 /\1-fPIC '"$OPT"' /' Makerules
sed -i -E '/^ifeq "\$\(OS\)" "kobo"/,/^endif/ s/ -O2 -pipe / -pipe /' Makerules
grep -n '^  CFLAGS += ' Makerules | sed -n '/fPIC/p' | head -1
rm -rf build/release
make -j"$(nproc)" tesseract=no USE_SYSTEM_LIBS=yes OS=kobo build=release \
     SYS_LIBJPEG_CFLAGS="-I$T/install/include" libs > /dev/null
${CROSS_COMPILE}gcc -Wl,--gc-sections -o build/release/libmupdf.so \
    $(find build/release -name '*.o' | grep -Ev '(SourceHanSerif-Regular|DroidSansFallbackFull|NotoSerifTangut|color-lcms)') \
    -lm -L../freetype2/objs/.libs -lfreetype -L../harfbuzz/src/.libs -lgumbo -L../gumbo/.libs -lharfbuzz \
    -L../jbig2dec/.libs -ljbig2dec "$T/install/lib/libjpeg.a" -Wl,--exclude-libs,libjpeg.a \
    -L../openjpeg/build/bin -lopenjp2 -L../zlib -lz \
    -shared -Wl,-soname -Wl,libmupdf.so -Wl,--no-undefined
cp build/release/libmupdf.so /work/libs/libmupdf.so
${STRIP:-${CROSS_COMPILE}strip} -o /work/target/libmupdf-stripped.so build/release/libmupdf.so
chown -R "$HOST_UID:$HOST_GID" /work
EOF

${DOCKER:-docker} run --rm -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    -v "$BLD:/work" "$IMAGE" bash -c "$INNER"

readelf -d "$BLD/target/libmupdf-stripped.so" | grep -E "NEEDED"
ls -la "$BLD/target/libmupdf-stripped.so"
