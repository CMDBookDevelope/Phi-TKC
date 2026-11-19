#!/usr/bin/env bash
# Every Android release uses the repository's persistent signing identity.
# Missing or replaced secrets must fail instead of generating another identity.
set +x
set -euo pipefail
umask 077

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
for name in ANDROID_RELEASE_KEYSTORE_BASE64 ANDROID_RELEASE_STORE_PASSWORD ANDROID_RELEASE_KEY_PASSWORD ANDROID_RELEASE_KEY_ALIAS; do
  if [[ -z "${!name:-}" ]]; then
    echo "::error::Missing repository signing secret ${name}"
    exit 1
  fi
done

SIGNING_DIR="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/phi-tkc-signing.XXXXXX")"
trap 'rm -rf -- "${SIGNING_DIR}"' EXIT
export PHI_SIGNING_KEYSTORE="${SIGNING_DIR}/release.p12"
python3 - <<'PY'
import base64, os
from pathlib import Path
try:
    data = base64.b64decode(os.environ['ANDROID_RELEASE_KEYSTORE_BASE64'], validate=True)
except ValueError:
    raise SystemExit('::error::Invalid Android release keystore encoding')
if not data:
    raise SystemExit('::error::Empty Android release keystore')
Path(os.environ['PHI_SIGNING_KEYSTORE']).write_bytes(data)
PY

if ! keytool -exportcert -keystore "${PHI_SIGNING_KEYSTORE}" \
  -storepass:env ANDROID_RELEASE_STORE_PASSWORD \
  -alias "${ANDROID_RELEASE_KEY_ALIAS}" -file "${SIGNING_DIR}/certificate.der" >/dev/null 2>&1; then
  echo '::error::Could not read the persistent Android release certificate'
  exit 1
fi
EXPECTED_CERT="$(tr -d '[:space:]' < "${SCRIPT_DIR}/android-release-certificate.sha256")"
ACTUAL_CERT="$(sha256sum "${SIGNING_DIR}/certificate.der" | cut -d ' ' -f 1)"
if [[ ! "${EXPECTED_CERT}" =~ ^[a-f0-9]{64}$ || "${ACTUAL_CERT}" != "${EXPECTED_CERT}" ]]; then
  echo '::error::Android signing certificate does not match the pinned release identity'
  exit 1
fi
if [[ "${1:-}" == '--check-config' ]]; then
  echo "Android release signing identity verified: ${ACTUAL_CERT}"
  exit 0
fi

INPUT_APK="${1:-}"
if [[ -z "${INPUT_APK}" ]]; then
  INPUT_APK="$(find "${REPO_DIR}/src-tauri/gen/android/app/build/outputs/apk" -type f -name '*release-unsigned.apk' -print -quit)"
fi
if [[ ! -f "${INPUT_APK}" ]]; then
  echo '::error::No unsigned Android release APK was produced'
  exit 1
fi
OUTPUT_APK="${2:-${REPO_DIR}/Phi-TKC-android-arm64.apk}"
SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
if [[ -z "${SDK_ROOT}" ]]; then
  echo '::error::Android SDK path is missing'
  exit 1
fi
BUILD_TOOLS_DIR="$(find "${SDK_ROOT}/build-tools" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -1)"
if [[ ! -x "${BUILD_TOOLS_DIR}/apksigner" || ! -x "${BUILD_TOOLS_DIR}/zipalign" ]]; then
  echo '::error::Android SDK build-tools are unavailable'
  exit 1
fi
"${BUILD_TOOLS_DIR}/zipalign" -f -p 4 "${INPUT_APK}" "${SIGNING_DIR}/aligned.apk"
"${BUILD_TOOLS_DIR}/apksigner" sign --ks "${PHI_SIGNING_KEYSTORE}" \
  --ks-pass env:ANDROID_RELEASE_STORE_PASSWORD --key-pass env:ANDROID_RELEASE_KEY_PASSWORD \
  --ks-key-alias "${ANDROID_RELEASE_KEY_ALIAS}" --out "${OUTPUT_APK}" "${SIGNING_DIR}/aligned.apk"
"${BUILD_TOOLS_DIR}/apksigner" verify --verbose --print-certs "${OUTPUT_APK}" > "${SIGNING_DIR}/verification.txt"
APK_CERT="$(sed -n 's/^Signer #1 certificate SHA-256 digest: //p' "${SIGNING_DIR}/verification.txt")"
if [[ "${APK_CERT}" != "${EXPECTED_CERT}" ]]; then
  echo '::error::The APK signer does not match the pinned Android release identity'
  exit 1
fi
cat "${SIGNING_DIR}/verification.txt"
echo "Signed Android release APK: ${OUTPUT_APK}"
