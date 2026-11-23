#!/usr/bin/env bash
# Builds wireant.apk: cargo-apk for the Rust library, then adds the Java side
# (java/radio/wireant/MainActivity.java, the file picker) and signs it again.
#   ./build-apk.sh [aarch64-linux-android | x86_64-linux-android]
set -euo pipefail
cd "$(dirname "$0")"
target=${1:-aarch64-linux-android}

export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export ANDROID_NDK_ROOT=${ANDROID_NDK_ROOT:-$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}
export JAVA_HOME=${JAVA_HOME:-$HOME/Android/jdk17}
export PATH=$JAVA_HOME/bin:$PATH
bt=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)
jar=$(ls -d "$ANDROID_HOME"/platforms/android-* | sort -V | tail -1)/android.jar
keystore=${KEYSTORE:-$HOME/.android/debug.keystore}

cargo apk build --release --target "$target"

out=target/release/apk
work=target/java
rm -rf "$work" && mkdir -p "$work/classes" "$work/dex"
javac -source 8 -target 8 -bootclasspath "$jar" -Xlint:-options -d "$work/classes" $(find java -name '*.java')
"$bt/d8" --release --min-api 26 --lib "$jar" --output "$work/dex" $(find "$work/classes" -name '*.class')

cp "$out/wireant.apk" "$work/unsigned.apk"
(cd "$work/dex" && "$bt/aapt" add ../unsigned.apk classes.dex >/dev/null)
"$bt/zipalign" -f -p 4 "$work/unsigned.apk" "$work/aligned.apk"
"$bt/apksigner" sign --ks "$keystore" --ks-pass pass:android --out "$out/wireant.apk" "$work/aligned.apk"
echo "built $out/wireant.apk"
