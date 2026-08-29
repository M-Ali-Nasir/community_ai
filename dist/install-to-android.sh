#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APK_PATH="$DIR/dist/CommunityAI.apk"
export PATH=$PATH:$HOME/.local/bin

echo "=========================================================="
echo " Installing Community AI APK to Connected Android Device..."
echo "=========================================================="

if [ ! -f "$APK_PATH" ]; then
  echo "Error: APK not found at $APK_PATH"
  exit 1
fi

# Check for adb device
DEVICE=$(adb devices | grep -w "device" | awk '{print $1}' | head -n 1)

if [ -z "$DEVICE" ]; then
  echo "No authorized device detected yet."
  echo "Please make sure:"
  echo "1. USB Debugging is ON in Settings > Developer Options."
  echo "2. For Xiaomi/Redmi: Enable 'Install via USB' in Developer Options."
  echo "3. Tap 'Allow' on the phone screen popup when prompted."
  echo ""
  echo "Waiting for device authorization..."
  adb wait-for-device
fi

echo "Installing CommunityAI.apk to device..."
adb install -r "$APK_PATH"

echo "Launching Community AI app on phone..."
adb shell monkey -p ai.community.worker -c android.intent.category.LAUNCHER 1

echo "Done! Community AI is now installed and running on your Android device."
