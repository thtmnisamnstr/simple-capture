#!/usr/bin/env bash
# Usage: third_party/ffmpeg/build-linux.sh [linux|windows]
#   linux    native static helper for this machine (x86_64 or arm64), linking only glibc
#   windows  x86_64 Windows helper cross-built with mingw-w64, importing only system DLLs
# Env: JOBS (default: all cores), CLEAN=1 to rebuild every library.
#
# The Linux and Windows twins of build-macos.sh: the same pinned, checksum-verified sources and
# the same GPL (never nonfree) feature set with networking disabled. Windows adds Media
# Foundation encoders and D3D11 decoding; both add NVENC, which loads the NVIDIA driver at run
# time. None adds a link-time dependency. Linux has no VAAPI (the helper would need libva to start
# at all); the app's GStreamer path covers VAAPI there.
set -euo pipefail
. "$(dirname -- "$0")/../lib.sh"

OS=${1:-linux}
JOBS=${JOBS:-$(ncpu)}
case "$OS-$(uname -m)" in
    linux-x86_64)  PLAT=linux-x86_64; FF_ARCH=x86_64; VPX_TARGET=x86_64-linux-gcc; HOST=x86_64-linux-gnu ;;
    linux-aarch64) PLAT=linux-arm64;  FF_ARCH=aarch64; VPX_TARGET=arm64-linux-gcc; HOST=aarch64-linux-gnu ;;
    windows-*)     PLAT=windows-x86_64; FF_ARCH=x86_64; VPX_TARGET=x86_64-win64-gcc; HOST=x86_64-w64-mingw32 ;;
    *) die "unknown target $OS on $(uname -m)" ;;
esac
WIN=false
[ "$OS" = windows ] && WIN=true

WORK="$TP_BUILD/$PLAT"
SRC="$WORK/src"
PREFIX="$WORK/prefix"
STAMPS="$WORK/stamps"
OUT="$TP_OUT/$PLAT"
[ "${CLEAN:-0}" = 1 ] && rm -rf "$WORK"
mkdir -p "$SRC" "$PREFIX/lib/pkgconfig" "$STAMPS" "$OUT"

unset CPATH C_INCLUDE_PATH CPLUS_INCLUDE_PATH LIBRARY_PATH PKG_CONFIG_PATH CMAKE_PREFIX_PATH \
      CFLAGS CXXFLAGS LDFLAGS CPPFLAGS
export PKG_CONFIG=pkg-config
export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"
if $WIN; then
    export CC=$HOST-gcc CXX=$HOST-g++ AR=$HOST-ar RANLIB=$HOST-ranlib STRIP=$HOST-strip
    CROSS_HOST=(--host="$HOST")
    EXE=.exe
else
    export CC=gcc CXX=g++ STRIP=strip
    CROSS_HOST=()
    EXE=
fi
LIBFLAGS="-ffile-prefix-map=$TP_ROOT=third_party"
OPT="-O2"
CMAKE_COMMON=(
    -G Ninja
    -DCMAKE_BUILD_TYPE=Release
    -DCMAKE_INSTALL_PREFIX="$PREFIX"
    -DCMAKE_INSTALL_LIBDIR=lib
    -DCMAKE_FIND_USE_PACKAGE_REGISTRY=OFF
    -DBUILD_SHARED_LIBS=OFF
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON
    -DCMAKE_C_FLAGS="$LIBFLAGS"
    -DCMAKE_CXX_FLAGS="$LIBFLAGS"
)
if $WIN; then
    CMAKE_COMMON+=(
        -DCMAKE_SYSTEM_NAME=Windows -DCMAKE_SYSTEM_PROCESSOR=AMD64
        -DCMAKE_C_COMPILER="$CC" -DCMAKE_CXX_COMPILER="$CXX" -DCMAKE_RC_COMPILER="$HOST-windres"
        -DCMAKE_FIND_ROOT_PATH="$PREFIX" -DCMAKE_FIND_ROOT_PATH_MODE_PROGRAM=NEVER
        -DCMAKE_FIND_ROOT_PATH_MODE_LIBRARY=ONLY -DCMAKE_FIND_ROOT_PATH_MODE_INCLUDE=ONLY
    )
fi

stamp_ok() { [ -f "$STAMPS/$1" ] && [ "$(cat "$STAMPS/$1")" = "$(pin "$1" sha256)" ]; }
stamp_set() { pin "$1" sha256 > "$STAMPS/$1"; }
prepare() { local a; a=$(fetch "$1"); unpack "$a" "$SRC/$1"; cd "$SRC/$1"; }

autotools() {
    ./configure ${CROSS_HOST[@]+"${CROSS_HOST[@]}"} --prefix="$PREFIX" --disable-shared \
        --enable-static --with-pic "$@" CFLAGS="$LIBFLAGS $OPT"
    make -j"$JOBS" && make install
}

WEBP_OPTS=(
    -DWEBP_BUILD_ANIM_UTILS=OFF -DWEBP_BUILD_CWEBP=OFF -DWEBP_BUILD_DWEBP=OFF
    -DWEBP_BUILD_GIF2WEBP=OFF -DWEBP_BUILD_IMG2WEBP=OFF -DWEBP_BUILD_VWEBP=OFF
    -DWEBP_BUILD_WEBPINFO=OFF -DWEBP_BUILD_WEBPMUX=OFF -DWEBP_BUILD_EXTRAS=OFF
    -DWEBP_BUILD_LIBWEBPMUX=ON
)

build_zlib() {
    stamp_ok zlib && return 0
    prepare zlib
    cmake -S . -B build "${CMAKE_COMMON[@]}" -DZLIB_BUILD_SHARED=OFF -DZLIB_BUILD_TESTING=OFF
    cmake --build build -j"$JOBS"
    cmake --install build
    # zlib names its static library libzs.a on Windows; zlib.pc and FFmpeg ask for -lz.
    $WIN && cp "$PREFIX/lib/libzs.a" "$PREFIX/lib/libz.a"
    stamp_set zlib
}

build_libwebp() {
    stamp_ok libwebp && return 0
    prepare libwebp
    cmake -S . -B build "${CMAKE_COMMON[@]}" "${WEBP_OPTS[@]}"
    cmake --build build -j"$JOBS"
    cmake --install build
    stamp_set libwebp
}

build_libogg() { stamp_ok libogg && return 0; prepare libogg; autotools; stamp_set libogg; }
build_libvorbis() {
    stamp_ok libvorbis && return 0
    prepare libvorbis
    autotools --disable-oggtest --with-ogg="$PREFIX"
    stamp_set libvorbis
}
build_opus() {
    stamp_ok opus && return 0
    prepare opus
    autotools --disable-doc --disable-extra-programs
    stamp_set opus
}

build_libvpx() {
    stamp_ok libvpx && return 0
    prepare libvpx
    local env=()
    $WIN && env=(CROSS="$HOST-")
    env ${env[@]+"${env[@]}"} ./configure --target="$VPX_TARGET" --prefix="$PREFIX" \
        --enable-static --disable-shared --enable-pic --enable-runtime-cpu-detect \
        --disable-examples --disable-tools --disable-docs --disable-unit-tests \
        --enable-vp8 --enable-vp9 --disable-vp8-decoder --disable-vp9-decoder \
        --extra-cflags="$LIBFLAGS" --extra-cxxflags="$LIBFLAGS"
    sed -i "s|$TP_ROOT|third_party|g" vpx_config.c
    env ${env[@]+"${env[@]}"} make -j"$JOBS"
    env ${env[@]+"${env[@]}"} make install
    stamp_set libvpx
}

build_x264() {
    stamp_ok x264 && return 0
    prepare x264
    local cross=()
    $WIN && cross=(--host="$HOST" --cross-prefix="$HOST-")
    ./configure --prefix="$PREFIX" ${cross[@]+"${cross[@]}"} --enable-static --enable-pic --disable-cli \
        --disable-opencl --disable-lavf --disable-swscale --disable-ffms --disable-gpac --disable-lsmash \
        --extra-cflags="$LIBFLAGS"
    make -j"$JOBS" && make install-lib-static
    stamp_set x264
}

build_x265() {
    stamp_ok x265 && return 0
    prepare x265
    local asm=ON
    [ "$FF_ARCH" = aarch64 ] && asm=OFF
    cmake -S source -B build "${CMAKE_COMMON[@]}" \
        -DENABLE_SHARED=OFF -DENABLE_CLI=OFF -DENABLE_LIBNUMA=OFF -DENABLE_ASSEMBLY=$asm \
        -DENABLE_SVE=OFF -DENABLE_SVE2=OFF -DENABLE_SVE2_BITPERM=OFF
    cmake --build build -j"$JOBS"
    cmake --install build
    # x265.pc omits the C++ runtime that a static link needs.
    sed -i 's/^Libs.private:.*/& -lstdc++/' "$PREFIX/lib/pkgconfig/x265.pc"
    grep -q 'Libs.private' "$PREFIX/lib/pkgconfig/x265.pc" || echo 'Libs.private: -lstdc++' >> "$PREFIX/lib/pkgconfig/x265.pc"
    stamp_set x265
}

build_nvheaders() {
    stamp_ok nv-codec-headers && return 0
    prepare nv-codec-headers
    make install PREFIX="$PREFIX"
    stamp_set nv-codec-headers
}

build_svtav1() {
    stamp_ok svt-av1 && return 0
    prepare svt-av1
    cmake -S . -B build "${CMAKE_COMMON[@]}" \
        -DBUILD_APPS=OFF -DBUILD_TESTING=OFF -DENABLE_NASM=ON -DSVT_AV1_LTO=OFF
    cmake --build build -j"$JOBS"
    cmake --install build
    stamp_set svt-av1
}

# shellcheck disable=SC2054 # comma lists are single configure values
FFMPEG_CONFIGURE=(
    --prefix=/opt/simple-capture-helper
    --arch="$FF_ARCH"
    --pkg-config=pkg-config --pkg-config-flags=--static
    --extra-cflags="-I$PREFIX/include" --extra-ldflags="-L$PREFIX/lib"
    --extra-version=simple-capture
    --enable-gpl
    --disable-shared --enable-static
    --disable-autodetect --disable-network --disable-ffplay --disable-doc --disable-debug
    --disable-sdl2 --disable-xlib --disable-iconv --enable-zlib --disable-bzlib --disable-lzma
    --enable-pthreads
    --enable-libx264 --enable-libx265 --enable-libsvtav1 --enable-libvpx
    --enable-libopus --enable-libvorbis --enable-libwebp
    --enable-ffnvcodec --enable-nvenc
    --disable-everything
    --enable-protocol=file,pipe
    --enable-encoder=libx264,libx265,libsvtav1,libvpx_vp8,libvpx_vp9,prores_ks
    --enable-encoder=aac,libopus,libvorbis,flac,alac,pcm_s16le,pcm_s24le,pcm_f32le
    --enable-encoder=gif,apng,png,libwebp_anim,rawvideo,wrapped_avframe
    --enable-encoder=h264_nvenc,hevc_nvenc
    --enable-decoder=rawvideo,wrapped_avframe,h264,hevc,vp8,vp9,prores,gif,png,apng,webp
    --enable-decoder=aac,opus,vorbis,flac,alac,pcm_s16le,pcm_s24le,pcm_f32le
    --enable-muxer=mov,mp4,webm,matroska,gif,apng,webp,wav,yuv4mpegpipe,rawvideo,null,framemd5
    --enable-demuxer=mov,matroska,gif,apng,webp,wav,yuv4mpegpipe,rawvideo,pcm_f32le,pcm_s16le
    --enable-parser=h264,hevc,vp8,vp9,av1,aac,opus,vorbis,flac,gif,png
    --enable-bsfs
    --enable-indev=lavfi
    --enable-filter=palettegen,paletteuse,scale,format,fps,split,setpts,setsar,setparams,trim,atrim
    --enable-filter=null,anull,aformat,aresample,pad,crop,testsrc2,color,sine,anullsrc
)
if $WIN; then
    # shellcheck disable=SC2054
    FFMPEG_CONFIGURE+=(
        --target-os=mingw32 --cross-prefix="$HOST-" --enable-cross-compile
        --enable-w32threads --disable-pthreads
        --enable-mediafoundation --enable-d3d11va --enable-dxva2
        --enable-encoder=h264_mf,hevc_mf
        --enable-hwaccel=h264_d3d11va,hevc_d3d11va,vp9_d3d11va,h264_dxva2,hevc_dxva2
        --extra-ldflags="-static -static-libgcc -static-libstdc++"
    )
else
    FFMPEG_CONFIGURE+=(--target-os=linux --extra-ldflags="-static-libgcc -static-libstdc++")
fi

build_ffmpeg() {
    # mingw's libgcc_s.a imports libgcc_s_seh-1.dll; the static libgcc covers everything.
    $WIN && sed -i 's/ -lgcc_s//g' "$PREFIX"/lib/pkgconfig/*.pc
    prepare ffmpeg
    printf '%q ' ./configure "${FFMPEG_CONFIGURE[@]}" > "$OUT/ffmpeg.configure.txt"
    echo >> "$OUT/ffmpeg.configure.txt"
    ./configure "${FFMPEG_CONFIGURE[@]}" || { tail -60 ffbuild/config.log; die "ffmpeg configure failed"; }
    make -j"$JOBS"
    install -m 0755 "ffmpeg$EXE" "$OUT/ffmpeg$EXE"
    install -m 0755 "ffprobe$EXE" "$OUT/ffprobe$EXE"
    $STRIP "$OUT/ffmpeg$EXE" "$OUT/ffprobe$EXE"
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
    sed -n '1,/\*\//p' "$SRC/nv-codec-headers/include/ffnvcodec/nvEncodeAPI.h" > "$d/nv-codec-headers-LICENSE"
}

verify_linkage() {
    local bin bad=0
    : > "$OUT/ffmpeg.linkage.txt"
    for bin in "$OUT/ffmpeg$EXE" "$OUT/ffprobe$EXE"; do
        if $WIN; then
            $HOST-objdump -p "$bin" | awk '/DLL Name:/ {print $3}' | sort -u | tee -a "$OUT/ffmpeg.linkage.txt" |
            while read -r dll; do
                case "${dll,,}" in
                    kernel32.dll|user32.dll|advapi32.dll|ole32.dll|oleaut32.dll|shell32.dll|bcrypt.dll|\
                    msvcrt.dll|ws2_32.dll|mfplat.dll|mf.dll|mfuuid.dll|propsys.dll|shlwapi.dll|gdi32.dll|\
                    api-ms-win-*|ucrtbase.dll|secur32.dll|d3d11.dll|dxgi.dll|strmiids.dll) ;;
                    *) echo "NOT ALLOWED: $dll" >&2; exit 1 ;;
                esac
            done || bad=1
        else
            ldd "$bin" | tee -a "$OUT/ffmpeg.linkage.txt" | while read -r lib _; do
                case "$lib" in
                    linux-vdso.so.*|libc.so.*|libm.so.*|libmvec.so.*|libgcc_s.so.*|libpthread.so.*|\
                    libdl.so.*|librt.so.*|/lib*/ld-linux*|ld-linux*) ;;
                    *) echo "NOT ALLOWED: $lib" >&2; exit 1 ;;
                esac
            done || bad=1
        fi
    done
    [ "$bad" = 0 ] || die "the helper links something beyond the system libraries"
}

log "building $PLAT (jobs $JOBS)"
build_zlib
build_libwebp
build_libogg
build_libvorbis
build_opus
build_libvpx
build_x264
build_x265
build_svtav1
build_nvheaders
build_ffmpeg
collect_licenses
verify_linkage
if ! $WIN; then
    "$OUT/ffmpeg" -hide_banner -buildconf > "$OUT/ffmpeg.buildconf.txt"
    "$OUT/ffmpeg" -hide_banner -version | head -1
fi
for sec in ffmpeg x264 x265 svt-av1 libvpx opus libogg libvorbis zlib libwebp; do
    printf '%s %s %s\n' "$sec" "$(pin "$sec" version)" "$(pin "$sec" sha256)"
done > "$OUT/ffmpeg.sources.txt"
log "done: $OUT/ffmpeg$EXE $OUT/ffprobe$EXE"
