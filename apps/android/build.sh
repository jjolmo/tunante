#!/bin/sh
# Build the Android app: Rust first, Gradle second.
#
# Gradle does not know how to build Rust, and AGP's externalNativeBuild only
# drives CMake or ndk-build. So the cdylib and the decoder are produced here by
# cargo-ndk and staged into app/src/main/jniLibs, which AGP then packages like
# any other prebuilt .so.
#
# Two things about that staging are load-bearing:
#
#   * The decoder is renamed to libtunante_decoder.so. It is an executable, not
#     a library, but only files matching lib*.so are extracted into
#     nativeLibraryDir — and nativeLibraryDir is the one place an app is allowed
#     to execve from. Called anything else it would not be extracted at all.
#
#   * libc++_shared.so is copied out of the NDK by hand. Nothing links it into
#     the APK on our behalf, because the crates that need it are built outside
#     Gradle, and without it both binaries fail to load with a missing DT_NEEDED.
set -eu

cd "$(dirname "$0")"
# ../.. because this script sits at apps/android/ and the Cargo workspace root
# is the repository root.
ROOT=$(cd ../.. && pwd)

: "${ANDROID_NDK_HOME:=$HOME/Android/Sdk/ndk/27.3.13750724}"
export ANDROID_NDK_HOME

API=26

# arm64-v8a is the phone. x86_64 is the emulator, and it is worth carrying:
# without it there is no way to look at the interface except on a device that
# might be locked, and UI work should not need someone's thumb.
#
# It is not a substitute for the phone where the decoders are concerned — on
# x86_64 lazyusf2 turns on ARCH_MIN_SSE2 and DeSmuME takes its SSE branch, so
# the emulator runs different C. Fine for pixels, useless for "do the cores
# still work".
#
# ABIS="arm64-v8a" skips the second build when only the phone matters.
: "${ABIS:=arm64-v8a x86_64}"

# Cleared rather than overwritten: staging only adds, so an ABI dropped from
# $ABIS would keep shipping whatever was left there by the last build that did
# include it. `ABIS="arm64-v8a"` has to mean an APK with one ABI in it, not an
# APK with a stale second one.
rm -rf app/src/main/jniLibs

for ABI in $ABIS; do
    case "$ABI" in
        arm64-v8a) TRIPLE=aarch64-linux-android ;;
        x86_64)    TRIPLE=x86_64-linux-android ;;
        *) echo "unknown ABI: $ABI"; exit 1 ;;
    esac
    JNI="app/src/main/jniLibs/$ABI"
    OUT="$ROOT/target/$TRIPLE/release"

    echo "== rust ($TRIPLE, api $API)"
    # From the workspace directory, not with --manifest-path: cargo-ndk runs its
    # own `cargo metadata` in the current directory before handing anything to
    # cargo, so from here it would look for a Cargo.toml that is not there.
    (cd "$ROOT" && cargo ndk -t "$ABI" --platform "$API" \
        build --release -p tunante-android -p tunante-decoder)

    echo "== staging into $JNI"
    mkdir -p "$JNI"
    cp "$OUT/libtunante_android.so" "$JNI/"
    cp "$OUT/tunante-decoder"       "$JNI/libtunante_decoder.so"
    cp "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/$TRIPLE/libc++_shared.so" \
       "$JNI/"
    chmod 755 "$JNI"/*.so
    ls -l "$JNI"
done

echo "== fixtures into assets"
mkdir -p app/src/main/assets
for f in sample.psf sample.nsf sine.flac sine.mp3; do
    cp "$ROOT/crates/tunante-codec/tests/fixtures/$f" app/src/main/assets/
done

echo "== gradle"
# The wrapper, not whatever `gradle` happens to be on PATH: it pins the version
# the project was built against, and CI has no gradle installed at all.
exec "${GRADLE:-./gradlew}" "${@:-assembleDebug}"
