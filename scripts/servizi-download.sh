#!/usr/bin/env bash
# Builds the servizi.everbright.it download pair from the CI installer: the installer under a stable name and the JSON
# the products page reads. Prints the JSON; the upload is a separate workflow step.
set -euo pipefail
[ "$#" -eq 4 ] || { echo "usage: $0 <installer.exe> <version> <commit> <out-dir>" >&2; exit 2; }
installer=$1 version=$2 commit=$3 out=$4
file=AI-Plan-Usage-Windows-x64-Setup.exe

mkdir -p "$out"
cp "$installer" "$out/$file"
sha=$(sha256sum "$out/$file" | cut -d' ' -f1)
bytes=$(wc -c < "$out/$file" | tr -d ' ')
jq -n --arg version "$version" --arg sha256 "$sha" --argjson bytes "$bytes" --arg commit "$commit" --arg file "$file" \
  '{version: $version, sha256: $sha256, bytes: $bytes, commit: $commit, file: $file}' > "$out/AI-Plan-Usage-Windows-x64.json"
cat "$out/AI-Plan-Usage-Windows-x64.json"
