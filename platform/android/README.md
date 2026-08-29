# Android APK Build & Integration Guide

The Android application embeds the **Shared Rust Core** via JNI (`community-ffi`) to provide hardware monitoring, cryptographic identity, and network coordination natively on mobile devices.

---

## 1. Prerequisites
- **Android NDK** (r25+ or latest via Android Studio SDK Manager).
- Set `ANDROID_NDK_HOME`:
  ```bash
  export ANDROID_NDK_HOME=$HOME/Android/Sdk/ndk/<version>
  ```
- **Rust Android Targets**:
  ```bash
  rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
  cargo install cargo-ndk
  ```

---

## 2. Compile Native `.so` Libraries
Run the automated build script:
```bash
./platform/android/build_android.sh
```
This generates:
- `platform/android/jniLibs/arm64-v8a/libcommunity_ffi.so`
- `platform/android/jniLibs/armeabi-v7a/libcommunity_ffi.so`
- `platform/android/jniLibs/x86_64/libcommunity_ffi.so`

---

## 3. Generate the Android APK

### Option A: Via Android Studio
1. Open or create an Android project in Android Studio.
2. Copy `platform/android/jniLibs/` into `app/src/main/jniLibs/`.
3. Copy `platform/android/CommunityAINative.kt` into `app/src/main/java/ai/community/worker/`.
4. Build $\rightarrow$ **Build Bundle(s) / APK(s) $\rightarrow$ Build APK(s)**.

### Option B: Via Gradle CLI
```bash
cd platform/android
./gradlew assembleDebug
```
The installable debug APK will be generated at:
```
app/build/outputs/apk/debug/app-debug.apk
```

---

## 4. Install & Test on Android Device
Connect your Android device via USB with USB Debugging enabled:
```bash
adb install app/build/outputs/apk/debug/app-debug.apk
```
