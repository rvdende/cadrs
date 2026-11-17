#!/usr/bin/env bash
# Signs a Windows executable in place with Authenticode, through osslsigncode, when a code-signing
# certificate is given; without one it does nothing (and says so), so unsigned builds still work:
#   WINDOWS_SIGN_PFX=<cert.pfx> WINDOWS_SIGN_PASSWORD=<its password> tools/windows/sign.sh <file.exe>
# WINDOWS_SIGN_TIMESTAMP picks the RFC 3161 timestamp server (DigiCert's by default), which keeps
# the signature valid after the certificate expires; `none` skips it (offline tests only). Used by tools/release/package.sh for
# cadrs.exe, and by the installer (tools/windows/cadrs.nsi) for itself and its uninstaller.
set -euo pipefail

file="$1"
if [[ -z "${WINDOWS_SIGN_PFX:-}" ]]; then
    echo "sign.sh: no WINDOWS_SIGN_PFX, $file stays unsigned" >&2
    exit 0
fi
out="$file.signed"
ts="${WINDOWS_SIGN_TIMESTAMP:-http://timestamp.digicert.com}"
ts_args=()
[[ "$ts" != none ]] && ts_args=(-ts "$ts")
osslsigncode sign \
    -pkcs12 "$WINDOWS_SIGN_PFX" \
    ${WINDOWS_SIGN_PASSWORD:+-pass "$WINDOWS_SIGN_PASSWORD"} \
    -n cadrs -i https://cadrs.net \
    -h sha256 \
    "${ts_args[@]}" \
    -in "$file" -out "$out"
mv "$out" "$file"
echo "sign.sh: signed $file" >&2
