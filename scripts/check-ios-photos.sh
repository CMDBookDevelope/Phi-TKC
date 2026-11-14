#!/usr/bin/env bash
# Compile the real plugin against Tauri/UIKit/PhotoKit after Tauri iOS init/build.
set -euo pipefail
project_root=${1:?Pass the absolute Phi-TKC checkout path}
build_root=${2:?Pass a separate derived-data directory}
plugin_root="$project_root/src-tauri/plugins/chart-picker/ios"
test -f "$plugin_root/../.tauri/tauri-api/Package.swift"
xcrun swiftc -frontend -parse "$plugin_root/Sources/ChartPickerPlugin.swift"
cd "$plugin_root"
xcrun xcodebuild \
  -scheme tauri-plugin-chart-picker \
  -configuration Debug \
  -sdk iphonesimulator \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath "$build_root" \
  CODE_SIGNING_ALLOWED=NO build
