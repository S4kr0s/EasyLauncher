#!/usr/bin/env bash
# Stores the macOS signing + notarization secrets for CI (see docs/SIGNING.md).
# Values go from your keychain export, prompts and the .p8 file straight into
# `gh secret set` over stdin; nothing is printed or written to shell history.
# Usage: scripts/set-apple-secrets.sh [path/to/devid.p12]
set -euo pipefail
repo=$(gh repo view --json nameWithOwner -q .nameWithOwner)

identity=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)"$/\1/p' | head -1)
[ -n "$identity" ] || { echo "No 'Developer ID Application' certificate in your keychain." >&2; exit 1; }
echo "Repo:     $repo"
echo "Identity: $identity"

p12=${1:-}
if [ -z "$p12" ]; then
  echo
  echo "Keychain Access opens now. Under 'My Certificates', right-click"
  echo "  $identity"
  echo "→ Export → save as devid.p12 on your Desktop, with a new password."
  open -a "Keychain Access"
  read -rp "Path to the .p12 [~/Desktop/devid.p12]: " p12
  p12=${p12:-~/Desktop/devid.p12}
fi
p12=${p12/#\~/$HOME}
[ -f "$p12" ] || { echo "Not found: $p12" >&2; exit 1; }
read -rsp "Password of the .p12: " p12pw; echo

echo
echo "Notarization login, either:"
echo "  1) App Store Connect API team key (.p8 + Issuer ID)"
echo "  2) Apple ID + app-specific password (appleid.apple.com → Sign-In and Security → App-Specific Passwords)"
read -rp "Choose [2]: " mode; mode=${mode:-2}
if [ "$mode" = 1 ]; then
  shopt -s nullglob
  keys=(~/.appstoreconnect/private_keys/AuthKey_*.p8 ~/Downloads/AuthKey_*.p8)
  shopt -u nullglob
  p8=${keys[0]:-}
  if [ ${#keys[@]} -gt 1 ]; then
    echo "Several keys found:"; printf '  %s\n' "${keys[@]}"
    read -rp "Which one [${p8}]: " pick; p8=${pick:-$p8}
  fi
  [ -n "$p8" ] && [ -f "$p8" ] || { echo "No AuthKey_*.p8 found." >&2; exit 1; }
  keyid=$(basename "$p8" .p8); keyid=${keyid#AuthKey_}
  echo "API key:  $keyid"
  read -rp "Issuer ID (Integrations → Team Keys, above the list): " issuer
  [ -n "$issuer" ] || { echo "Issuer ID is required." >&2; exit 1; }
else
  team=$(sed -n 's/.*(\([A-Z0-9]*\))$/\1/p' <<<"$identity")
  read -rp "Apple ID (email of your developer account): " appleid
  read -rsp "App-specific password: " apppw; echo
  [ -n "$appleid" ] && [ -n "$apppw" ] || { echo "Both are required." >&2; exit 1; }
fi

set_secret() { gh secret set "$1" -R "$repo" >/dev/null && echo "  set $1"; }
echo "Storing secrets…"
base64 -i "$p12" | tr -d '\n' | set_secret APPLE_CERTIFICATE
printf '%s' "$p12pw" | set_secret APPLE_CERTIFICATE_PASSWORD
printf '%s' "$identity" | set_secret APPLE_SIGNING_IDENTITY
if [ "$mode" = 1 ]; then
  printf '%s' "$issuer" | set_secret APPLE_API_ISSUER
  printf '%s' "$keyid" | set_secret APPLE_API_KEY
  set_secret APPLE_API_KEY_P8 < "$p8"
else
  printf '%s' "$appleid" | set_secret APPLE_ID
  printf '%s' "$apppw" | set_secret APPLE_PASSWORD
  printf '%s' "$team" | set_secret APPLE_TEAM_ID
fi

read -rp "Delete $p12 now? [Y/n] " del
[[ ${del:-y} =~ ^[Yy] ]] && rm -f "$p12" && echo "  deleted"
echo "Done. Re-run CI: gh workflow run build -R $repo  (or push to main)"
