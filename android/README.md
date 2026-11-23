# wireant for Android (proof of concept)

Compiles the desktop sources in `../src` unchanged into an Android app (`src/lib.rs` is the
only Android-specific code). Built-in engines only (wiremom, MININEC): Android won't run the
bundled nec2c executable, and there are no external NEC programs.

Stand-ins and shortcuts:
- File → Open, Save as and the PDF reports use Android's file picker. `rfd-stub/` queues the
  requests, `src/files.rs` hands them to `java/radio/wireant/MainActivity.java` (a NativeActivity
  subclass that receives the picker's answer) and copies files back and forth: an opened file
  is copied into the app's storage and each later Save is copied back to it; a saved file is
  written to the app's `exports` folder, then copied where the user picks.
- Tools → Preferences' NEC program picker says it isn't available (no external NEC on Android).
- The UI is shared with the desktop (`src/theme.rs`, `src/num.rs`, `src/app.rs`): below 720
  points across the tabs move to a bottom bar, Geometry's sections become foldable cards with
  one line per wire (tap it for the wire editor), and Calculate puts the button and the answer
  first. `theme::touch` gives finger-sized targets and turns number fields into tap-to-type
  boxes, so swiping across them scrolls. Light or dark follows the phone.
- Fonts: egui's bundled emoji fonts (`monochrome_emoji_fonts`), then Hack and Android's
  `NotoSansSymbols-Regular-Subsetted.ttf` as fallbacks for ● ↶ → ∥ ⊥ ✘. 🧲 still shows as a box
  (only in the colour emoji font).
- Signed with the SDK debug key.

Build and run on the emulator or a phone:

    export ANDROID_HOME=~/Android/Sdk ANDROID_NDK_ROOT=~/Android/Sdk/ndk/30.0.16248370
    export JAVA_HOME=~/Android/jdk17 PATH=~/Android/jdk17/bin:$PATH
    ./build-apk.sh                        # phones; ./build-apk.sh x86_64-linux-android for the emulator
    adb install -r target/release/apk/wireant.apk

`build-apk.sh` runs `cargo apk build`, then compiles the Java side with javac and d8, adds
`classes.dex` to the APK and signs it again (plain `cargo apk build` gives an APK without it,
which fails to start).
