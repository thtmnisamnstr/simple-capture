#!/usr/bin/env bash
# Usage: third_party/ffmpeg/build-macos.sh [arm64|x86_64]
# Env: JOBS (default: all cores), MACOS_MIN (default 14.0), CLEAN=1 to rebuild every library.
#
# Static GPL (never nonfree) FFmpeg export helper for Simple Capture. Every library is built
# from the pinned, checksum-verified sources in third_party/versions.toml into a private
# prefix; pkg-config and CMake are fenced off from Homebrew so nothing leaks in, and the
# finished binaries must link only Apple system frameworks and libSystem (checked at the end).
# Recipe ideas (x265/libvorbis/libvpx Darwin quirks) come from markus-perl/ffmpeg-build-script
# (MIT); see docs/THIRD_PARTY.md.
set -euo pipefail
. "$(dirname -- "$0")/../lib.sh"

ARCH=${1:-arm64}
MACOS_MIN=${MACOS_MIN:-14.0}
JOBS=${JOBS:-$(ncpu)}
case "$ARCH" in
    arm64)  HOST=aarch64-apple-darwin; FF_ARCH=aarch64; VPX_TARGET=arm64-darwin23-gcc ;;
    x86_64) HOST=x86_64-apple-darwin;  FF_ARCH=x86_64;  VPX_TARGET=x86_64-darwin23-gcc ;;
    *) die "unknown arch $ARCH (arm64|x86_64)" ;;
esac
HOST_ARCH=$(uname -m)
CROSS=false
[ "$ARCH" = "$HOST_ARCH" ] || CROSS=true

WORK="$TP_BUILD/macos-$ARCH"
SRC="$WORK/src"
PREFIX="$WORK/prefix"
STAMPS="$WORK/stamps"
OUT="$TP_OUT/macos-$ARCH"
[ "${CLEAN:-0}" = 1 ] && rm -rf "$WORK"
mkdir -p "$SRC" "$PREFIX/lib/pkgconfig" "$STAMPS" "$OUT"

unset CPATH C_INCLUDE_PATH CPLUS_INCLUDE_PATH OBJC_INCLUDE_PATH LIBRARY_PATH PKG_CONFIG_PATH \
      CMAKE_PREFIX_PATH CMAKE_FRAMEWORK_PATH CFLAGS CXXFLAGS LDFLAGS CPPFLAGS
export PKG_CONFIG=pkgconf
export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"
export MACOSX_DEPLOYMENT_TARGET="$MACOS_MIN"
SDK=$(xcrun --sdk macosx --show-sdk-path)
export CC=clang CXX=clang++
ARCHFLAGS="-arch $ARCH -mmacosx-version-min=$MACOS_MIN -isysroot $SDK"
# Keeps the build machine's paths (and user name) out of __FILE__ strings in the shipped binary.
LIBFLAGS="$ARCHFLAGS -ffile-prefix-map=$TP_ROOT=third_party"
OPT="-O2"
CMAKE_COMMON=(
    -G Ninja
    -DCMAKE_BUILD_TYPE=Release
    -DCMAKE_INSTALL_PREFIX="$PREFIX"
    -DCMAKE_INSTALL_LIBDIR=lib
    -DCMAKE_OSX_ARCHITECTURES="$ARCH"
    -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_MIN"
    -DCMAKE_OSX_SYSROOT="$SDK"
    -DCMAKE_IGNORE_PREFIX_PATH="/opt/homebrew;/usr/local"
    -DCMAKE_FIND_USE_PACKAGE_REGISTRY=OFF
    -DBUILD_SHARED_LIBS=OFF
    -DCMAKE_C_FLAGS="-ffile-prefix-map=$TP_ROOT=third_party"
    -DCMAKE_CXX_FLAGS="-ffile-prefix-map=$TP_ROOT=third_party"
)
if $CROSS; then
    CMAKE_COMMON+=(-DCMAKE_SYSTEM_NAME=Darwin -DCMAKE_SYSTEM_PROCESSOR="$ARCH")
fi

stamp_ok() { [ -f "$STAMPS/$1" ] && [ "$(cat "$STAMPS/$1")" = "$(pin "$1" sha256)" ]; }
stamp_set() { pin "$1" sha256 > "$STAMPS/$1"; }
prepare() { local a; a=$(fetch "$1"); unpack "$a" "$SRC/$1"; cd "$SRC/$1"; }

WEBP_OPTS=(
    -DWEBP_BUILD_ANIM_UTILS=OFF -DWEBP_BUILD_CWEBP=OFF -DWEBP_BUILD_DWEBP=OFF
    -DWEBP_BUILD_GIF2WEBP=OFF -DWEBP_BUILD_IMG2WEBP=OFF -DWEBP_BUILD_VWEBP=OFF
    -DWEBP_BUILD_WEBPINFO=OFF -DWEBP_BUILD_WEBPMUX=OFF -DWEBP_BUILD_EXTRAS=OFF
    -DWEBP_BUILD_LIBWEBPMUX=ON
)

build_zlib() {
    stamp_ok zlib && return 0
    prepare zlib
    cmake -S . -B build-$ARCH "${CMAKE_COMMON[@]}" \
        -DZLIB_BUILD_SHARED=OFF -DZLIB_BUILD_TESTING=OFF
    cmake --build build-$ARCH -j"$JOBS"
    cmake --install build-$ARCH
    stamp_set zlib
}

build_libwebp() {
    stamp_ok libwebp && return 0
    prepare libwebp
    cmake -S . -B build-$ARCH "${CMAKE_COMMON[@]}" "${WEBP_OPTS[@]}"
    cmake --build build-$ARCH -j"$JOBS"
    cmake --install build-$ARCH
    stamp_set libwebp
}

build_libogg() {
    stamp_ok libogg && return 0
    prepare libogg
    ./configure --host="$HOST" --prefix="$PREFIX" --disable-shared --enable-static \
        CFLAGS="$LIBFLAGS $OPT" LDFLAGS="$ARCHFLAGS"
    make -j"$JOBS" && make install
    stamp_set libogg
}

build_libvorbis() {
    stamp_ok libvorbis && return 0
    prepare libvorbis
    # The shipped configure adds -force_cpusubtype_ALL on Darwin, which current ld rejects.
    sed -i '' 's/ -force_cpusubtype_ALL//g' configure
    ./configure --host="$HOST" --prefix="$PREFIX" --disable-shared --enable-static \
        --disable-oggtest --with-ogg="$PREFIX" CFLAGS="$LIBFLAGS $OPT" LDFLAGS="$ARCHFLAGS"
    make -j"$JOBS" && make install
    stamp_set libvorbis
}

build_opus() {
    stamp_ok opus && return 0
    prepare opus
    ./configure --host="$HOST" --prefix="$PREFIX" --disable-shared --enable-static \
        --disable-doc --disable-extra-programs CFLAGS="$LIBFLAGS $OPT" LDFLAGS="$ARCHFLAGS"
    make -j"$JOBS" && make install
    stamp_set opus
}

build_libvpx() {
    stamp_ok libvpx && return 0
    prepare libvpx
    local as=()
    [ "$ARCH" = x86_64 ] && as=(--as=nasm)
    ./configure --target="$VPX_TARGET" --prefix="$PREFIX" ${as[@]+"${as[@]}"} \
        --enable-static --disable-shared --enable-pic --enable-runtime-cpu-detect \
        --disable-examples --disable-tools --disable-docs --disable-unit-tests \
        --enable-vp8 --enable-vp9 --disable-vp8-decoder --disable-vp9-decoder \
        --extra-cflags="-mmacosx-version-min=$MACOS_MIN -ffile-prefix-map=$TP_ROOT=third_party" --extra-cxxflags="-mmacosx-version-min=$MACOS_MIN -ffile-prefix-map=$TP_ROOT=third_party"
    # libvpx embeds its configure line (vpx_codec_build_config); drop the build machine's path.
    sed -i '' "s|$TP_ROOT|third_party|g" vpx_config.c
    make -j"$JOBS" && make install
    stamp_set libvpx
}

build_x264() {
    stamp_ok x264 && return 0
    prepare x264
    local cross=()
    $CROSS && cross=(--host="$HOST")
    ./configure --prefix="$PREFIX" ${cross[@]+"${cross[@]}"} --enable-static --enable-pic --disable-cli \
        --disable-opencl --disable-lavf --disable-swscale --disable-ffms --disable-gpac --disable-lsmash \
        --extra-cflags="$LIBFLAGS" --extra-ldflags="$ARCHFLAGS"
    make -j"$JOBS" && make install-lib-static
    stamp_set x264
}

build_x265() {
    stamp_ok x265 && return 0
    prepare x265
    # 8-bit only: exports are SDR 8-bit 4:2:0, so the main10/main12 multilib (3 builds) is skipped.
    cmake -S source -B build-$ARCH "${CMAKE_COMMON[@]}" \
        -DENABLE_SHARED=OFF -DENABLE_CLI=OFF -DENABLE_LIBNUMA=OFF -DENABLE_ASSEMBLY=ON \
        -DENABLE_SVE=OFF -DENABLE_SVE2=OFF -DENABLE_SVE2_BITPERM=OFF
    cmake --build build-$ARCH -j"$JOBS"
    cmake --install build-$ARCH
    stamp_set x265
}

build_svtav1() {
    stamp_ok svt-av1 && return 0
    prepare svt-av1
    cmake -S . -B build-$ARCH "${CMAKE_COMMON[@]}" \
        -DBUILD_APPS=OFF -DBUILD_TESTING=OFF -DENABLE_NASM=ON -DSVT_AV1_LTO=OFF
    cmake --build build-$ARCH -j"$JOBS"
    cmake --install build-$ARCH
    stamp_set svt-av1
}

# --prefix is never installed to (binaries are copied out); a neutral value keeps the build
# machine's path out of `ffmpeg -buildconf`.
FFMPEG_CONFIGURE=(
    --prefix=/opt/simple-capture-helper
    --arch="$FF_ARCH" --target-os=darwin --cc=clang --cxx=clang++
    --pkg-config=pkgconf --pkg-config-flags=--static
    --extra-cflags="$ARCHFLAGS -I$PREFIX/include" --extra-cxxflags="$ARCHFLAGS" --extra-ldflags="$ARCHFLAGS -L$PREFIX/lib"
    --extra-version=simple-capture
    --enable-gpl
    --disable-shared --enable-static
    --disable-autodetect --disable-network --disable-ffplay --disable-doc --disable-debug
    --disable-sdl2 --disable-xlib --disable-securetransport --disable-iconv
    --enable-zlib --disable-bzlib --disable-lzma
    --disable-appkit --disable-avfoundation --disable-coreimage --disable-metal
    --enable-videotoolbox --enable-audiotoolbox --enable-pthreads
    --enable-libx264 --enable-libx265 --enable-libsvtav1 --enable-libvpx
    --enable-libopus --enable-libvorbis --enable-libwebp
    --disable-everything
    --enable-protocol=file,pipe
    --enable-encoder=libx264,libx265,libsvtav1,libvpx_vp8,libvpx_vp9,h264_videotoolbox,hevc_videotoolbox,prores_videotoolbox,prores_ks
    --enable-encoder=aac,aac_at,libopus,libvorbis,flac,alac,pcm_s16le,pcm_s24le,pcm_f32le
    --enable-encoder=gif,apng,png,libwebp_anim,rawvideo,wrapped_avframe
    --enable-decoder=rawvideo,wrapped_avframe,h264,hevc,vp8,vp9,prores,gif,png,apng,webp
    --enable-decoder=aac,aac_at,opus,vorbis,flac,alac,pcm_s16le,pcm_s24le,pcm_f32le
    --enable-hwaccel=h264_videotoolbox,hevc_videotoolbox,prores_videotoolbox,vp9_videotoolbox
    --enable-muxer=mov,mp4,webm,matroska,gif,apng,webp,wav,yuv4mpegpipe,rawvideo,null,framemd5
    --enable-demuxer=mov,matroska,gif,apng,webp,wav,yuv4mpegpipe,rawvideo,pcm_f32le,pcm_s16le
    --enable-parser=h264,hevc,vp8,vp9,av1,aac,opus,vorbis,flac,gif,png
    --enable-bsfs
    --enable-indev=lavfi
    --enable-filter=palettegen,paletteuse,scale,format,fps,split,setpts,setsar,setparams,trim,atrim
    --enable-filter=null,anull,aformat,aresample,pad,crop,testsrc2,color,sine,anullsrc
)
$CROSS && FFMPEG_CONFIGURE+=(--enable-cross-compile)

build_ffmpeg() {
    prepare ffmpeg
    printf '%q ' ./configure "${FFMPEG_CONFIGURE[@]}" > "$OUT/ffmpeg.configure.txt"
    echo >> "$OUT/ffmpeg.configure.txt"
    ./configure "${FFMPEG_CONFIGURE[@]}" || { tail -60 ffbuild/config.log; die "ffmpeg configure failed"; }
    make -j"$JOBS"
    install -m 0755 ffmpeg "$OUT/ffmpeg"
    install -m 0755 ffprobe "$OUT/ffprobe"
    strip -x "$OUT/ffmpeg" "$OUT/ffprobe"
}

collect_licenses() {
    local d="$OUT/licenses/ffmpeg"
    rm -rf "$d"; mkdir -p "$d"
    cp "$SRC/ffmpeg/COPYING.GPLv2" "$d/ffmpeg-COPYING.GPLv2"
    cp "$SRC/ffmpeg/LICENSE.md" "$d/ffmpeg-LICENSE.md"
    cp "$SRC/x264/COPYING" "$d/x264-COPYING"
    cp "$SRC/x265/COPYING" "$d/x265-COPYING"
    cp "$SRC/svt-av1/LICENSE.md" "$d/svt-av1-LICENSE.md"
    cp "$SRC/svt-av1/PATENTS.md" "$d/svt-av1-PATENTS.md"
    cp "$SRC/libvpx/LICENSE" "$d/libvpx-LICENSE"
    cp "$SRC/libvpx/PATENTS" "$d/libvpx-PATENTS"
    cp "$SRC/opus/COPYING" "$d/opus-COPYING"
    cp "$SRC/libogg/COPYING" "$d/libogg-COPYING"
    cp "$SRC/libvorbis/COPYING" "$d/libvorbis-COPYING"
    cp "$SRC/zlib/LICENSE" "$d/zlib-LICENSE"
    cp "$SRC/libwebp/COPYING" "$d/libwebp-COPYING"
    cp "$SRC/libwebp/PATENTS" "$d/libwebp-PATENTS"
}

verify_linkage() {
    local bin bad=0
    : > "$OUT/ffmpeg.otool.txt"
    for bin in "$OUT/ffmpeg" "$OUT/ffprobe"; do
        otool -L "$bin" | tee -a "$OUT/ffmpeg.otool.txt"
        while read -r lib _; do
            case "$lib" in
                /System/Library/Frameworks/*|/usr/lib/libSystem.B.dylib|/usr/lib/libc++.1.dylib|/usr/lib/libobjc.A.dylib) ;;
                *) echo "NOT ALLOWED: $lib" >&2; bad=1 ;;
            esac
        done < <(otool -L "$bin" | tail -n +2)
    done
    [ "$bad" = 0 ] || die "binaries link something outside system frameworks/libSystem"
    vtool -show-build "$OUT/ffmpeg" | tee "$OUT/ffmpeg.vtool.txt"
}

log "building for macOS $ARCH (min $MACOS_MIN, jobs $JOBS, cross=$CROSS)"
build_zlib
build_libwebp
build_libogg
build_libvorbis
build_opus
build_libvpx
build_x264
build_x265
build_svtav1
build_ffmpeg
collect_licenses
verify_linkage
if ! $CROSS || [ "$ARCH" = x86_64 ]; then
    "$OUT/ffmpeg" -hide_banner -buildconf > "$OUT/ffmpeg.buildconf.txt"
    "$OUT/ffmpeg" -hide_banner -version | head -1
fi
for sec in ffmpeg x264 x265 svt-av1 libvpx opus libogg libvorbis zlib libwebp; do
    printf '%s %s %s\n' "$sec" "$(pin "$sec" version)" "$(pin "$sec" sha256)"
done > "$OUT/ffmpeg.sources.txt"
log "done: $OUT/ffmpeg $OUT/ffprobe"
