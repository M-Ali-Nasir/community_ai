# Community AI release builds

**Updated:** 2026-09-27  
**Release version:** `1.0.0`  
**Source branch:** `community_ai_v1`  
**Packaging branch:** `community-ai-app-bundles`  
**Source commit:** `2e2caa3d33e9292b1db48be7ae0454904b1db9ef` (merge of PR #6, universal responsive UI)  
**Identifier:** `ai.community.desktop`  
**Product name:** Community AI

All three packages use `apps/desktop/frontend/` through Tauri. There is no separate Android/Windows/Linux UI.

Do not promote BUILD success to INSTALL or RUNTIME success.

---

## Toolchain (this machine)

| Tool | Version |
|------|---------|
| rustc | 1.98.0 (88d9e12ae 2026-08-18) |
| cargo | 1.98.0 (797e8a9bc 2026-08-05) |
| Node | 22.15.0 |
| npm | 10.9.2 |
| Tauri CLI | 2.12.0 |
| Java | OpenJDK 17.0.20 |
| Android NDK | 27.0.12077973 |
| Android compileSdk | 37 (Gradle also resolved platform 36) |
| cargo-xwin | 0.23.1 |
| NSIS | 3.09-4 (`makensis`) |
| clang-cl / lld-link | Ubuntu LLVM 18.1.3 (user-local, Windows cross) |

---

## Artifacts

Binaries are **not** committed. Copy them from `release/` after a local build. Hashes below are SHA-256 of the files that were hashed after build (and, for Linux, after launch testing of that same AppImage).

### Linux x86_64 AppImage

| Field | Value |
|-------|--------|
| Actual name | `Community_AI-x86_64.AppImage` |
| Path (build) | `apps/desktop/src-tauri/target/release/bundle/appimage/Community_AI-x86_64.AppImage` |
| Architecture | x86_64 |
| SHA256 | `d1a219f3a779012cd8e5ee87543cf3c9dc9549a755510ec13ff3a4ceae7f0a99` |
| Size | 79M |
| Build | VERIFIED |
| Install | VERIFIED (copied out of the build tree to `/tmp` and executed) |
| Runtime | VERIFIED (process + WebKitGTK + SQLite + identity; see limitations) |

Launch:

```bash
chmod +x Community_AI-x86_64.AppImage
APPIMAGE_EXTRACT_AND_RUN=1 ./Community_AI-x86_64.AppImage
```

`APPIMAGE_EXTRACT_AND_RUN=1` is required when FUSE AppImage mounts are unavailable.

Local data is **not** inside the AppImage. This host used:

- identity: `~/.config/community-ai/identity.key` (existing key reused; not regenerated)
- SQLite: `~/.local/share/community-ai/storage/database/community.db`

### Windows x86_64 NSIS installer

| Field | Value |
|-------|--------|
| Actual name | `Community AI_1.0.0_x64-setup.exe` |
| Path (build) | `apps/desktop/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Community AI_1.0.0_x64-setup.exe` |
| Architecture | x86_64 (installer stub is i686 NSIS; payload is `community-desktop.exe` PE32+) |
| SHA256 | `844be9921222c7d8a057ad9c5d5db546a81eb127b73d56611f4c3bd1643267b9` |
| Size | 4.6M |
| Build | VERIFIED (PE32 Nullsoft installer; contains the cross-compiled GUI exe) |
| Install | NOT TESTED (no Windows host) |
| Runtime | NOT TESTED |
| Signing | skipped (Tauri Windows signing is host-Windows by default) |

The inner application binary is `community-desktop.exe` (PE32+, 18M). That file is **not** the installer.

### Android ARM64 APK (TEST signed)

| Field | Value |
|-------|--------|
| Gradle output | `app-universal-release-unsigned.apk` (unsigned) |
| Installable TEST APK | `Community-AI-1.0.0-Android-arm64-TEST.apk` |
| Path (unsigned) | `apps/desktop/src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk` |
| Architecture | ARM64 (`lib/arm64-v8a/libcommunity_desktop_lib.so` only) |
| SHA256 (unsigned) | `3493a00c493de59b254210ac3282a2f6db1d6c7115a0f877471d767c12dae2c3` |
| SHA256 (TEST signed) | `ad51a127cfe74dedc9792db76cd3c580bae83c493d5e2de2e1bff6442afc90a6` |
| Size | 22M |
| Build | VERIFIED |
| Install | NOT TESTED (`adb devices` empty) |
| Runtime | NOT TESTED |
| Signing | TEST (Android debug keystore). **Not** production release signing. |

---

## Build commands

From `apps/desktop/`, after `community_ai_v1` is checked out on `community-ai-app-bundles`.

### Linux AppImage

```bash
npx tauri build --bundles appimage
```

This environment lacked distro `-dev` packages (`pkg-config`, WebKitGTK headers). The binary was linked against a user-space sysroot; linuxdeploy was run with `APPIMAGE_EXTRACT_AND_RUN=1` and **without** the gtk plugin (the plugin failed on this AppDir). Runtime still loaded bundled `libwebkit2gtk-4.1`.

### Windows NSIS

```bash
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"   # clang-cl, lld-link, makensis wrapper
export NSISDIR="$HOME/.local/nsis-linux/usr/share/nsis" # required; Debian makensis PREFIX_DATA is /usr/share/nsis
export LD_LIBRARY_PATH="$HOME/.local/llvm/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
npx tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis
```

If `npx tauri` still invokes `makensis` without `NSISDIR`, run `makensis installer.nsi` in `src-tauri/target/x86_64-pc-windows-msvc/release/nsis/x64/` and copy `nsis-output.exe` to `bundle/nsis/Community AI_1.0.0_x64-setup.exe`.

### Android APK (aarch64)

```bash
export ANDROID_HOME="$HOME/.local/android-sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.0.12077973"
export NDK_HOME="$ANDROID_NDK_HOME"
export JAVA_HOME="/usr/lib/jvm/java-17-openjdk-amd64"
npx tauri android build --apk --target aarch64
```

Then debug-sign for sideload testing (do not commit the keystore):

```bash
apksigner sign --ks ~/.android/debug.keystore --ks-key-alias androiddebugkey \
  --ks-pass pass:android --key-pass pass:android \
  --out Community-AI-1.0.0-Android-arm64-TEST.apk aligned.apk
```

---

## Tests run before packaging

| Check | Result |
|-------|--------|
| `cargo fmt --all -- --check` | PASS |
| `cargo test --workspace` | PASS (before Android `mobile_entry_point` attr; that crate is workspace-excluded) |
| Frontend unit tests | NOT AVAILABLE (static `index.html`, no test script) |

---

## Platform limitations

- **Linux UI walkthrough:** session is Wayland. GNOME screenshot portal returned `AccessDenied`. Chat / Peers / Models / Tasks / Network / Settings were **not** clicked from this agent. Process stayed up with WebKit mapped; SQLite WAL was written.
- **Windows:** no Windows VM. Installer was not executed.
- **Android:** no emulator or device (`adb devices` empty). APK not installed.
- **Android local models:** `community-runtime` still starts `llama-server` via `std::process::Command` (desktop llama.cpp). That is **not** claimed to work on Android. No fake inference is substituted. Mesh/QUIC/storage still compile into the APK.
- **Android mDNS:** multicast permission is requested; LAN discovery on mobile is untested.
- **Production signing:** none of the three artifacts are store/production-signed.
- **PHYSICAL WAN:** still `PHYSICAL WAN VERIFIED — NOT TESTED`.
- **Compute receipts / wallet:** not in this release (remain on `community-ai-compute-receipts`).

---

## Architecture (unchanged)

No central server, central database, master node, or permanent coordinator was added for packaging. Chat still goes Tauri IPC → `CommunityApp` → `MeshSwarm` → QUIC.

Android permissions are INTERNET, ACCESS_NETWORK_STATE, ACCESS_WIFI_STATE, CHANGE_WIFI_MULTICAST_STATE only.
