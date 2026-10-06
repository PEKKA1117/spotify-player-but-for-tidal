#!/usr/bin/env bash
# One-off probe for spec 0003 ("Facts vs. assumptions"). Run once with a real
# Tidal account, then paste playback-probe-output.txt back. Delete this script
# once the spec's fixtures are recorded.
#
# Usage: scripts/tidal-playback-probe.sh HIRES_TRACK_ID LOSSLESS_TRACK_ID
#   HIRES_TRACK_ID     a track with the "Max" / hi-res badge in the Tidal app
#   LOSSLESS_TRACK_ID  a track WITHOUT it (CD quality only)
# A track ID is the number in its share link: https://tidal.com/browse/track/<id>
#
# What it does, against the live API, using tidalt's public client credentials:
#   1. logs in with the device flow (approve the code in your browser)
#   2. for each track and each tier of the ladder (HI_RES_LOSSLESS, LOSSLESS,
#      HIGH, LOW): GET /v1/tracks/{id}/playbackinfopostpaywall and decodes the
#      manifest (BTS JSON or DASH MPD)
#   3. the same for tidalt's endpoint, GET /v1/tracks/{id}/urlpostpaywall
#   4. downloads the first bytes of each stream (or the DASH init segment and
#      the first media segment), reports the HTTP headers that matter for
#      seeking (Accept-Ranges, Content-Length) and, if ffprobe is installed,
#      the codec, sample rate and bit depth
#   5. error shapes: an unknown track ID, a bogus bearer, and a stream URL
#      re-requested with a byte range far past its end
#   6. asks Tidal for the track twice, 5 s apart, to see whether stream URLs
#      change per request and whether they carry an expiry
#
# Every token, code, ID and URL query value is replaced by "<redacted len=N>"
# before anything is printed or written. Stream bytes are written to a temp
# directory that is deleted on exit. Requires: bash, curl, jq, base64,
# sha256sum; optional: ffprobe (from ffmpeg).
#
# Logging in creates a session on your account that the script cannot revoke
# (Tidal refuses revocation for this client, see spec 0002 "Facts"). It expires
# server-side like any other.

set -euo pipefail

[ $# -eq 2 ] || { sed -n '6,10p' "$0" >&2; exit 2; }
HIRES_ID=$1
LOSSLESS_ID=$2

CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
SCOPE="r_usr w_usr w_sub"
OUT="playback-probe-output.txt"
LADDER=(HI_RES_LOSSLESS LOSSLESS HIGH LOW)

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

log "# tidal-playback-probe $(date -u +%FT%TZ)"
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
AUTHZ=(-H "Authorization: Bearer $ACCESS")

# Subscription tier, if the API exposes it to this client.
req "GET /v1/users/<me>/subscription" "${AUTHZ[@]}" \
  "$API/users/$(jq -r .user_id "$WORK/tok.json")/subscription?countryCode=$CC"

# 2.–4. Each track × each tier
probe_track() {
  local label=$1 id=$2 q
  for q in "${LADDER[@]}"; do
    req "$label: playbackinfopostpaywall audioquality=$q" "${AUTHZ[@]}" \
      "$API/tracks/$id/playbackinfopostpaywall?audioquality=$q&playbackmode=STREAM&assetpresentation=FULL&countryCode=$CC"
    if [ "$STATUS" = 200 ]; then
      local mime manifest
      mime=$(jq -r .manifestMimeType <<<"$BODY")
      jq -r .manifest <<<"$BODY" | base64 -d >"$WORK/manifest" 2>/dev/null || true
      log "  manifest ($mime), redacted:"
      redact_urls <"$WORK/manifest" | sed 's/^/    /' | tee -a "$OUT"
      log ""
      case "$mime" in
        application/vnd.tidal.bts)
          local url; url=$(jq -r '.urls[0]' "$WORK/manifest")
          log "  url: $(url_shape "$url") $(expiry_hint "$url")"
          log "  urls in manifest: $(jq '.urls | length' "$WORK/manifest")"
          fetch_head "$label-$q.bin" "$url"
          ffprobe_file "$label-$q.bin"
          if [ "$q" = "${LADDER[0]}" ] || [ "$q" = LOSSLESS ]; then
            fetch_head "$label-$q-past-end.bin" "$url" "999999999-"
          fi
          ;;
        application/dash+xml)
          # SegmentTemplate URLs, with $Number$ replaced by startNumber and startNumber+1.
          local init media start
          init=$(grep -oE 'initialization="[^"]*"' "$WORK/manifest" | head -1 | cut -d'"' -f2 | sed 's/&amp;/\&/g' || true)
          media=$(grep -oE 'media="[^"]*"' "$WORK/manifest" | head -1 | cut -d'"' -f2 | sed 's/&amp;/\&/g' || true)
          start=$(grep -oE 'startNumber="[0-9]+"' "$WORK/manifest" | head -1 | grep -oE '[0-9]+' || echo 1)
          log "  init:  $(url_shape "$init") $(expiry_hint "$init")"
          log "  media: $(url_shape "$media") startNumber=$start"
          fetch_head "$label-$q-init.mp4" "$init" "0-"
          fetch_head "$label-$q-seg1.mp4" "${media//\$Number\$/$start}" "0-"
          cat "$WORK/$label-$q-init.mp4" "$WORK/$label-$q-seg1.mp4" >"$WORK/$label-$q-joined.mp4"
          ffprobe_file "$label-$q-joined.mp4"
          ;;
      esac
      log ""
    fi
    req "$label: urlpostpaywall audioquality=$q (tidalt)" "${AUTHZ[@]}" \
      "$API/tracks/$id/urlpostpaywall?audioquality=$q&urlusagemode=STREAM&assetpresentation=FULL&countryCode=$CC"
    if [ "$STATUS" = 200 ] && [ "$(jq -r '.urls[0] // empty' <<<"$BODY")" != "" ]; then
      local u; u=$(jq -r '.urls[0]' <<<"$BODY")
      log "  url: $(url_shape "$u") $(expiry_hint "$u")"
      fetch_head "$label-$q-upp.bin" "$u"
      ffprobe_file "$label-$q-upp.bin"
      log ""
    fi
  done
}
probe_track hires "$HIRES_ID"
probe_track lossless "$LOSSLESS_ID"

# 5. Error shapes
req "unknown track ID" "${AUTHZ[@]}" \
  "$API/tracks/1/playbackinfopostpaywall?audioquality=LOSSLESS&playbackmode=STREAM&assetpresentation=FULL&countryCode=$CC"
req "bogus bearer" -H "Authorization: Bearer bogus" \
  "$API/tracks/$LOSSLESS_ID/playbackinfopostpaywall?audioquality=LOSSLESS&playbackmode=STREAM&assetpresentation=FULL&countryCode=$CC"
req "PREVIEW presentation" "${AUTHZ[@]}" \
  "$API/tracks/$LOSSLESS_ID/playbackinfopostpaywall?audioquality=LOSSLESS&playbackmode=STREAM&assetpresentation=PREVIEW&countryCode=$CC"

# 6. Are stream URLs stable between requests?
url_of() {
  curl -sS "${AUTHZ[@]}" \
    "$API/tracks/$LOSSLESS_ID/playbackinfopostpaywall?audioquality=LOSSLESS&playbackmode=STREAM&assetpresentation=FULL&countryCode=$CC" \
    | jq -r .manifest | base64 -d | sha256sum | cut -c1-8
}
a=$(url_of); sleep 5; b=$(url_of)
log "### manifest fingerprint, two requests 5 s apart: $a / $b ($([ "$a" = "$b" ] && echo same || echo different))"

echo
echo "Done. Check $OUT for anything private, then paste it back."
