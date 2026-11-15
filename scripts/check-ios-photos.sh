#!/usr/bin/env bash
# Compile the real plugin against Tauri/UIKit/PhotoKit after Tauri iOS init/build.
set -euo pipefail
project_root=${1:?Pass the absolute Phi-TKC checkout path}
build_root=${2:?Pass a separate derived-data directory}
plugin_root="$project_root/src-tauri/plugins/chart-picker/ios"
test -f "$plugin_root/../.tauri/tauri-api/Package.swift"
bridge_headers="$plugin_root/Bridge/include"
mkdir -p "$build_root"
xcrun swiftc -frontend -parse -I "$bridge_headers" "$plugin_root"/Sources/*.swift

# Compile the actual C declarations and a Swift caller against the simulator
# SDK. Checking the emitted calls also catches a return to Swift-only symbol
# declarations that may appear to work while using the wrong calling convention.
sdk_root=$(xcrun --sdk iphonesimulator --show-sdk-path)
xcrun --sdk iphonesimulator clang -target arm64-apple-ios14.0-simulator \
  -isysroot "$sdk_root" -std=c11 -fsyntax-only \
  -I "$bridge_headers" "$plugin_root/Bridge/PhiTKCNativeBridge.c"
cat > "$build_root/native-bridge.swift" <<'SWIFT'
import PhiTKCNativeBridge
public func checkNativeBridge(_ bytes: UnsafeMutablePointer<UInt8>?, _ capacity: Int) -> Int32 {
    let session: UInt64 = 1
    phi_tkc_render_lifecycle(1)
    phi_tkc_preview_active(session, 1)
    phi_tkc_preview_touch(session, 2, 0, 0.5, 0.5, 1.0)
    let result = phi_tkc_preview_read_frame(session, bytes, numericCast(capacity))
    phi_tkc_preview_close(session)
    return result
}
SWIFT
xcrun swiftc -sdk "$sdk_root" -target arm64-apple-ios14.0-simulator \
  -I "$bridge_headers" -emit-ir "$build_root/native-bridge.swift" \
  -o "$build_root/native-bridge.ll"
python3 - "$build_root/native-bridge.ll" "$plugin_root/Sources" <<'PY'
import re
import sys
from pathlib import Path

ir = Path(sys.argv[1]).read_text()
symbols = ["render_lifecycle", "preview_read_frame", "preview_touch", "preview_active", "preview_close"]
sources = "\n".join(path.read_text() for path in Path(sys.argv[2]).glob("*.swift"))
for suffix in symbols:
    symbol = "phi_tkc_" + suffix
    assert not re.search(r'@_silgen_name\s*\(\s*"' + symbol + r'"', sources), f"Swift-only native declaration: {symbol}"
    declarations = [line for line in ir.splitlines() if re.search(r"^declare\b.*@" + symbol + r"\(", line)]
    calls = [line for line in ir.splitlines() if re.search(r"\bcall\b.*@" + symbol + r"\(", line)]
    assert declarations and calls, f"Missing native declaration or call: {symbol}"
    assert all(not re.search(r"\b(?:swiftcc|swifttailcc)\b", line) for line in declarations + calls), f"Swift calling convention: {symbol}"
print("All five Swift-to-Rust calls use the platform C calling convention")
PY
cd "$plugin_root"
xcrun xcodebuild \
  -scheme tauri-plugin-chart-picker \
  -configuration Debug \
  -sdk iphonesimulator \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath "$build_root" \
  CODE_SIGNING_ALLOWED=NO build
