#!/usr/bin/env bash
# Builds wireant.apk: cargo-apk for the Rust library, then adds the Java side
# (java/radio/wireant/MainActivity.java, the file picker) and signs it again.
#   ./build-apk.sh [aarch64-linux-android | x86_64-linux-android]
#
# Signing key: KEYSTORE (a .jks or .keystore file) and KEYSTORE_PASSWORD; KEY_ALIAS picks
# the key if the store holds several, KEY_PASSWORD if it differs from the store's. Without
# them, the SDK's debug key (~/.android/debug.keystore, password "android"), made here if it
# doesn't exist yet.
# Android only installs an update over an APK signed with the same key.
set -euo pipefail
cd "$(dirname "$0")"
target=${1:-aarch64-linux-android}

export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export ANDROID_NDK_ROOT=${ANDROID_NDK_ROOT:-$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}
if [ -z "${JAVA_HOME:-}" ] && [ -d "$HOME/Android/jdk17" ]; then
    export JAVA_HOME=$HOME/Android/jdk17
fi
[ -n "${JAVA_HOME:-}" ] && export PATH=$JAVA_HOME/bin:$PATH
bt=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)
jar=$(ls -d "$ANDROID_HOME"/platforms/android-* | sort -V | tail -1)/android.jar

debug=$HOME/.android/debug.keystore
if [ ! -f "$debug" ]; then
    mkdir -p "$(dirname "$debug")"
    keytool -genkey -keystore "$debug" -storepass android -keypass android \
        -alias androiddebugkey -dname "CN=Android Debug,O=Android,C=US" -keyalg RSA -keysize 2048 -validity 10000
fi
export KEYSTORE=${KEYSTORE:-$debug}
export KEYSTORE_PASSWORD=${KEYSTORE_PASSWORD:-android}
# cargo-apk's signature is replaced below, so it always gets the debug key (it can't take a
# key password that differs from the store's)
export CARGO_APK_RELEASE_KEYSTORE=$debug
export CARGO_APK_RELEASE_KEYSTORE_PASSWORD=android

cargo apk build --release --target "$target"

out=target/release/apk
work=target/java
rm -rf "$work" && mkdir -p "$work/classes" "$work/dex"
javac -source 8 -target 8 -bootclasspath "$jar" -Xlint:-options -d "$work/classes" $(find java -name '*.java')
"$bt/d8" --release --min-api 26 --lib "$jar" --output "$work/dex" $(find "$work/classes" -name '*.class')

cp "$out/wireant.apk" "$work/unsigned.apk"
(cd "$work/dex" && "$bt/aapt" add ../unsigned.apk classes.dex >/dev/null)
"$bt/zipalign" -f -p 4 "$work/unsigned.apk" "$work/aligned.apk"
sign=(--ks "$KEYSTORE" --ks-pass env:KEYSTORE_PASSWORD)
[ -n "${KEY_ALIAS:-}" ] && sign+=(--ks-key-alias "$KEY_ALIAS")
[ -n "${KEY_PASSWORD:-}" ] && sign+=(--key-pass env:KEY_PASSWORD)
"$bt/apksigner" sign "${sign[@]}" --out "$out/wireant.apk" "$work/aligned.apk"
"$bt/apksigner" verify "$out/wireant.apk"
echo "built $out/wireant.apk"
