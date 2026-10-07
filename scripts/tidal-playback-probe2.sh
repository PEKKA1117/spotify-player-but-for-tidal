#!/usr/bin/env bash
# One-off probe for spec 0003, decision 6 (option A): can a refresh token from
# our device-flow login (client fX2JxdmntZWK0ixT) be refreshed under tidalapi's
# PKCE client (6BDSRdpK9hqEBTgU), and does that token get 16/44.1 FLAC at
# LOSSLESS and still 24-bit FLAC at HI_RES_LOSSLESS? This is the workaround
# sleezer (PR #174) and hifi-api use since Tidal capped our client ID at AAC
# for LOSSLESS (python-tidal issue #404, 2026-04). Run once, paste back
# playback-probe2-output.txt, delete the script once the spec is updated.
#
# Usage: scripts/tidal-playback-probe2.sh CD_TRACK_ID HIRES_TRACK_ID
#   e.g. 473593668 (16/44.1) and 35986245 (24/96) from the first probe
#
# Requests, about 15 in total, spaced 1 s apart (no bursts):
#   1. device-flow login with our client (approve the code in the browser)
#   2. our token: LOSSLESS on the CD track (baseline, expect AAC HIGH), and the
#      same with an x-tidal-client-version header (rules the header in or out)
#   3. our token: openapi.tidal.com v2 trackManifests for the CD track (option D)
#   4. refresh our refresh token with the PKCE client ID + secret (option A)
#   5. that token: LOSSLESS on the CD track, HI_RES_LOSSLESS on the hi-res
#      track, plus the first bytes of each stream through ffprobe
#   6. that token: v2 trackManifests for the CD track
#   7. our original client: refresh again, to see whether the cross-client
#      refresh broke the original refresh token
# It stops at once on a 429 or a 403 mentioning "abuse".
#
# Tokens, codes, IDs and URL query values are redacted as in the first probe.
# Requires: bash, curl, jq, base64, sha256sum; optional: ffprobe.

set -euo pipefail

[ $# -eq 2 ] || { sed -n '10,11p' "$0" >&2; exit 2; }
CD_ID=$1
HIRES_ID=$2

CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
# tidalapi's PKCE client, stored double-base64 exactly as in
# EbbLabs/python-tidal tidalapi/session.py (client_id_pkce, client_secret_pkce).
PKCE_ID=$( { printf '%s' TmtKRVUxSmtjRXM= | base64 -d; printf '%s' NWFIRkZRbFJuVlE9PQ== | base64 -d; } | base64 -d)
PKCE_SECRET=$( { printf '%s' ZUdWMVVHMVpOMjVpY0ZvNVNVbGlURUZqVVQ= | base64 -d; printf '%s' a3pjMmhyWVRGV1RtaGxWVUZ4VGpaSlkzTjZhbFJIT0QwPQ== | base64 -d; } | base64 -d)
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
OPENAPI="https://openapi.tidal.com/v2"
SCOPE="r_usr w_usr w_sub"
CLIENT_VERSION="2026.9.15"
OUT="playback-probe2-output.txt"

for bin in curl jq base64 sha256sum; do
  command -v "$bin" >/dev/null || { echo "missing: $bin" >&2; exit 1; }
done
HAVE_FFPROBE=0; command -v ffprobe >/dev/null && HAVE_FFPROBE=1
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
: >"$OUT"

# Replace the value of every sensitive key with its length only.
redact_json='walk(if type == "object" then with_entries(
  if (.key | test("token|Token|deviceCode|userCode|device_code|user_code|sessionId|userId|user_id|email|username|firstName|lastName|^sub$|partnerId|manifestHash|^manifest$"; ""))
  then .value |= "<redacted len=\(tostring | length)>" else . end) else . end)'

# Redact URL query values (keep the names) and long ID-like path segments.
redact_urls() {
  sed -E -e 's/([?&;][A-Za-z0-9_.-]+=)[^&"'"'"' <]*/\1<redacted>/g' \
    -e 's#(https?://[^/"'"'"' <]+)?/[A-Za-z0-9_=-]{16,}#\1/<id>#g'
}

log() { printf '%s\n' "$*" | tee -a "$OUT"; }

# req NAME CURL_ARGS... → sets STATUS, BODY and HEADERS; logs the redacted result.
req() {
  local name=$1; shift
  local tmp hdr; tmp=$(mktemp); hdr=$(mktemp)
  STATUS=$(curl -sS -D "$hdr" -o "$tmp" -w '%{http_code}' "$@" || echo "curl-error")
  BODY=$(cat "$tmp"); HEADERS=$(cat "$hdr"); rm -f "$tmp" "$hdr"
  log "### $name"
  log "status: $STATUS"
  if jq -e . >/dev/null 2>&1 <<<"$BODY"; then
    jq "$redact_json" <<<"$BODY" | redact_urls | tee -a "$OUT"
  else
    log "non-JSON body (${#BODY} bytes): $(printf '%s' "${BODY:0:200}" | redact_urls)"
  fi
  log ""
}

# url_shape URL → host, path and the names of its query parameters only.
url_shape() {
  local u=$1 host path q
  host=$(sed -E 's#^https?://([^/]+).*#\1#' <<<"$u")
  path=$(sed -E 's#^https?://[^/]+([^?]*).*#\1#' <<<"$u")
  q=$(sed -nE 's#^[^?]*\?(.*)$#\1#p' <<<"$u" | tr '&' '\n' | cut -d= -f1 | paste -sd, -)
  # Path segments that look like IDs or hashes are shortened to their length.
  path=$(sed -E 's#/[A-Za-z0-9_-]{16,}#/<id>#g' <<<"$path")
  printf 'host=%s path=%s query-params=[%s]' "$host" "$path" "$q"
}

# expiry_hint URL → any query value that looks like a unix timestamp, as seconds from now.
expiry_hint() {
  local now; now=$(date +%s)
  sed -nE 's#^[^?]*\?(.*)$#\1#p' <<<"$1" | tr '&' '\n' | while IFS='=' read -r k v; do
    if [[ "$v" =~ ^[0-9]{10}$ ]]; then printf '%s=now%+ds ' "$k" $((v - now)); fi
  done
}

# fetch_head NAME URL [RANGE] → downloads up to RANGE (default the first 256 KiB) into $WORK/NAME
fetch_head() {
  local name=$1 url=$2 range=${3:-0-262143} hdr
  hdr=$(mktemp)
  local st; st=$(curl -sS -D "$hdr" -o "$WORK/$name" -w '%{http_code}' -H "Range: bytes=$range" "$url" || echo curl-error)
  log "  GET ($(url_shape "$url")) Range: bytes=$range -> $st, $(wc -c <"$WORK/$name" 2>/dev/null || echo 0) bytes"
  grep -i -E '^(content-type|content-length|content-range|accept-ranges|cache-control|expires):' "$hdr" | tr -d '\r' | sed 's/^/    /' | tee -a "$OUT" || true
  rm -f "$hdr"
}

ffprobe_file() {
  [ "$HAVE_FFPROBE" = 1 ] || { log "  (ffprobe not installed)"; return; }
  log "  ffprobe $1:"
  ffprobe -v error -show_entries format=format_name:stream=codec_name,profile,sample_rate,channels,bits_per_raw_sample,sample_fmt \
    -of compact "$WORK/$1" 2>&1 | sed 's/^/    /' | tee -a "$OUT" || true
}

log "# tidal-playback-probe2 $(date -u +%FT%TZ)"
log ""

# 1. Device-flow login (same as the auth probe; details are not logged again)
curl -sS -X POST "$AUTH/device_authorization" \
  --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "scope=$SCOPE" >"$WORK/dev.json"
DEVICE_CODE=$(jq -r .deviceCode "$WORK/dev.json")
USER_CODE=$(jq -r .userCode "$WORK/dev.json")
VERIFY=$(jq -r .verificationUri "$WORK/dev.json")
INTERVAL=$(jq -r '.interval // 5' "$WORK/dev.json")
EXPIRES=$(jq -r '.expiresIn // 300' "$WORK/dev.json")
echo
echo "Open https://$VERIFY and enter the code $USER_CODE (not written to $OUT)."
echo "Waiting for approval (expires in ${EXPIRES}s)..."
deadline=$(( $(date +%s) + EXPIRES ))
while :; do
  sleep "$INTERVAL"
  STATUS=$(curl -sS -o "$WORK/tok.json" -w '%{http_code}' -X POST "$AUTH/token" -u "$CLIENT_ID:$CLIENT_SECRET" \
    --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "device_code=$DEVICE_CODE" \
    --data-urlencode "grant_type=urn:ietf:params:oauth:grant-type:device_code" \
    --data-urlencode "scope=$SCOPE" || echo curl-error)
  [ "$STATUS" = 200 ] && break
  [ "$(jq -r '.error // empty' "$WORK/tok.json" 2>/dev/null || true)" = slow_down ] && INTERVAL=$((INTERVAL + 5))
  [ "$(date +%s)" -ge "$deadline" ] && { log "gave up: code expired"; exit 1; }
done
ACCESS=$(jq -r .access_token "$WORK/tok.json")
CC=$(jq -r .user.countryCode "$WORK/tok.json")
log "### logged in (country $CC, client $(jq -r '.clientName // "?"' "$WORK/tok.json"))"
log ""

# Stop at the first sign of Tidal's anti-abuse layer.
guard() {
  if [ "$STATUS" = 429 ] || { [ "$STATUS" = 403 ] && grep -qi abuse <<<"$BODY"; }; then
    log "STOPPING: Tidal answered $STATUS (rate limit / abuse detection)"; exit 1
  fi
  sleep 1
}

playbackinfo() { # LABEL TOKEN TRACK QUALITY [extra curl args]
  local label=$1 tok=$2 id=$3 q=$4; shift 4
  req "$label" -H "Authorization: Bearer $tok" "$@" \
    "$API/tracks/$id/playbackinfopostpaywall?audioquality=$q&playbackmode=STREAM&assetpresentation=FULL&countryCode=$CC"
  guard
  [ "$STATUS" = 200 ] || return 0
  local mime; mime=$(jq -r .manifestMimeType <<<"$BODY")
  jq -r .manifest <<<"$BODY" | base64 -d >"$WORK/manifest" 2>/dev/null || true
  log "  manifest ($mime), redacted:"; redact_urls <"$WORK/manifest" | sed 's/^/    /' | tee -a "$OUT"; log ""
  local f; f=$(tr -c 'A-Za-z0-9' _ <<<"$label")
  case "$mime" in
    application/vnd.tidal.bts)
      fetch_head "$f.bin" "$(jq -r '.urls[0]' "$WORK/manifest")"; ffprobe_file "$f.bin" ;;
    application/dash+xml)
      local init media start
      init=$(grep -oE 'initialization="[^"]*"' "$WORK/manifest" | head -1 | cut -d'"' -f2 | sed 's/&amp;/\&/g' || true)
      media=$(grep -oE 'media="[^"]*"' "$WORK/manifest" | head -1 | cut -d'"' -f2 | sed 's/&amp;/\&/g' || true)
      start=$(grep -oE 'startNumber="[0-9]+"' "$WORK/manifest" | head -1 | grep -oE '[0-9]+' || echo 1)
      fetch_head "$f-init.mp4" "$init" "0-"; fetch_head "$f-seg.mp4" "${media//\$Number\$/$start}" "0-"
      cat "$WORK/$f-init.mp4" "$WORK/$f-seg.mp4" >"$WORK/$f-joined.mp4"; ffprobe_file "$f-joined.mp4" ;;
  esac
  log ""
}

trackmanifests() { # LABEL TOKEN TRACK
  req "$1" -H "Authorization: Bearer $2" -H "x-tidal-client-version: $CLIENT_VERSION" \
    -H "Accept: application/vnd.api+json" \
    "$OPENAPI/trackManifests/$3?adaptive=true&manifestType=MPEG_DASH&uriScheme=HTTPS&usage=PLAYBACK&formats=FLAC&formats=FLAC_HIRES&formats=AACLC"
  guard
}

refresh() { # LABEL ID SECRET REFRESH_TOKEN → sets NEW_ACCESS
  req "$1" -X POST "$AUTH/token" \
    --data-urlencode "client_id=$2" --data-urlencode "client_secret=$3" \
    --data-urlencode "grant_type=refresh_token" --data-urlencode "refresh_token=$4" \
    --data-urlencode "scope=$SCOPE"
  guard
  NEW_ACCESS=""
  if [ "$STATUS" = 200 ]; then
    NEW_ACCESS=$(jq -r .access_token <<<"$BODY")
    log "  clientName: $(jq -r '.clientName // "?"' <<<"$BODY"); new refresh_token in response: $(jq -r 'if .refresh_token then "yes" else "no" end' <<<"$BODY")"
    log ""
  fi
}

ACCESS=$(jq -r .access_token "$WORK/tok.json")
REFRESH=$(jq -r .refresh_token "$WORK/tok.json")

# 2. Baseline with our token
playbackinfo "ours: CD track LOSSLESS (baseline)" "$ACCESS" "$CD_ID" LOSSLESS
playbackinfo "ours: CD track LOSSLESS + x-tidal-client-version" "$ACCESS" "$CD_ID" LOSSLESS -H "x-tidal-client-version: $CLIENT_VERSION"

# 3. Option D with our token
trackmanifests "ours: v2 trackManifests CD track" "$ACCESS" "$CD_ID"

# 4. Option A: refresh under the PKCE client
refresh "option A: refresh our refresh token with the PKCE client" "$PKCE_ID" "$PKCE_SECRET" "$REFRESH"
if [ -n "$NEW_ACCESS" ]; then
  PKCE_ACCESS=$NEW_ACCESS
  # 5. Playback with that token
  playbackinfo "PKCE token: CD track LOSSLESS" "$PKCE_ACCESS" "$CD_ID" LOSSLESS
  playbackinfo "PKCE token: hi-res track HI_RES_LOSSLESS" "$PKCE_ACCESS" "$HIRES_ID" HI_RES_LOSSLESS
  playbackinfo "PKCE token: CD track HI_RES_LOSSLESS (does Tidal downgrade to LOSSLESS?)" "$PKCE_ACCESS" "$CD_ID" HI_RES_LOSSLESS
  # 6. Option D with that token
  trackmanifests "PKCE token: v2 trackManifests CD track" "$PKCE_ACCESS" "$CD_ID"
else
  log "option A refresh failed: skipping steps 5 and 6"; log ""
fi

# 7. Does our original client still refresh the same refresh token?
refresh "our client: refresh the same refresh token again" "$CLIENT_ID" "$CLIENT_SECRET" "$REFRESH"

echo
echo "Done. Check $OUT for anything private, then paste it back."
