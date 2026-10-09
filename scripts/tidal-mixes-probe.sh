#!/usr/bin/env bash
# One-off probe for spec 0011 (mixes and radio), "Not verified": the
# my_collection_my_mixes page today (modules, totals, paging, mixType values,
# the mix ID form), the mix header (pages/mix, mixes/{id}), the largest page
# and depth of /mixes/{id}/items, and the artist radio. Run once, paste back
# mixes-probe-output.txt, delete the script once the spec is updated.
#
# Usage: scripts/tidal-mixes-probe.sh [TRACK_ID] [ARTIST_ID]
#   defaults: 33695188 (0004's autoplay seed) and 7367609 (0006's artist)
#
# Login: the device-flow link (with the code in it) is opened with xdg-open
# and the code is copied with wl-copy (or xclip/xsel), when those exist; just
# approve in the browser.
#
# About 25 GET requests, spaced 1 s apart (no bursts). Read-only: nothing in
# the account changes. It stops at once on a 429 or a 403 mentioning "abuse".
#
# Mix titles and subtitles name artists you listen to; they are logged, every
# array cut to its first 2 items. Tokens, user IDs and names are redacted.
# Requires: bash, curl, jq; optional: xdg-open, wl-copy (or xclip, xsel).

set -euo pipefail

CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
SCOPE="r_usr w_usr w_sub"
OUT="mixes-probe-output.txt"
TRACK=${1:-33695188}
ARTIST=${2:-7367609}

for bin in curl jq; do
  command -v "$bin" >/dev/null || { echo "missing: $bin" >&2; exit 1; }
done
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
: >"$OUT"

redact_json='walk(if type == "object" then with_entries(
  if (.key | test("token|Token|deviceCode|userCode|sessionId|userId|user_id|email|username|firstName|lastName|^sub$"; ""))
  then .value |= "<redacted len=\(tostring | length)>" else . end) else . end)'
# Keep the shape: every array cut to its first 2 elements, with its length noted.
shorten='walk(if type == "array" and length > 2 then (.[0:2] + ["… \(length) items in all"]) else . end)'

log() { printf '%s\n' "$*" | tee -a "$OUT"; }

# q NAME PATH [curl --data-urlencode ARGS...] → GET $API/PATH with countryCode
q() {
  local name=$1 path=$2; shift 2
  local tmp; tmp=$(mktemp)
  STATUS=$(curl -sS -G -o "$tmp" -w '%{http_code}' -H "Authorization: Bearer $ACCESS" \
    --data-urlencode "countryCode=$CC" "$@" "$API/$path" || echo curl-error)
  BODY=$(cat "$tmp"); rm -f "$tmp"
  log "### $name"
  log "GET /$path $*"
  log "status: $STATUS"
  if jq -e . >/dev/null 2>&1 <<<"$BODY"; then
    jq "$redact_json | $shorten" <<<"$BODY" | tee -a "$OUT"
  else
    log "non-JSON body (${#BODY} bytes): ${BODY:0:200}"
  fi
  log ""
  if [ "$STATUS" = 429 ] || { [ "$STATUS" = 403 ] && grep -qi abuse <<<"$BODY"; }; then
    log "STOPPING: Tidal answered $STATUS (rate limit / abuse detection)"; exit 1
  fi
  sleep 1
}

# summary LABEL: totals and item counts per list of the last response, and the top hit
summary() {
  jq -c '(select(has("items")) | {limit, offset, total: .totalNumberOfItems, items: (.items | length),
           first_ids: [.items[0:3][] | (.id // .uuid)]}),
         (to_entries[] | select(.value | type == "object" and has("items"))
          | {list: .key, limit: .value.limit, offset: .value.offset,
             total: .value.totalNumberOfItems, items: (.value.items | length),
             item_types: ([.value.items[]? | .type? // empty] | unique)}),
         (select(has("topHit")) | {topHit_type: .topHit.type?, topHit_keys: (.topHit.value | keys?)})' \
    <<<"$BODY" 2>/dev/null | sed "s/^/  $1: /" | tee -a "$OUT" || true
  log ""
}

# ids LABEL: item types, audio modes and the first 8 track IDs of the last list
ids() {
  jq -c '{types: ([.items[]? | .type? // "bare"] | unique),
          audioModes: ([.items[]? | (.item // .) | .audioModes] | unique),
          first_ids: [.items[0:8][]? | (.item // .) | .id]}' <<<"$BODY" 2>/dev/null \
    | sed "s/^/  $1: /" | tee -a "$OUT" || true
  log ""
}

copy_code() {
  if command -v wl-copy >/dev/null; then printf '%s' "$1" | wl-copy
  elif command -v xclip >/dev/null; then printf '%s' "$1" | xclip -selection clipboard
  elif command -v xsel >/dev/null; then printf '%s' "$1" | xsel --clipboard --input
  else return 1; fi
}

log "# tidal-mixes-probe $(date -u +%FT%TZ)"
log ""

# Device-flow login; the link with the code in it is opened for you
curl -sS -X POST "$AUTH/device_authorization" \
  --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "scope=$SCOPE" >"$WORK/dev.json"
DEVICE_CODE=$(jq -r .deviceCode "$WORK/dev.json")
USER_CODE=$(jq -r .userCode "$WORK/dev.json")
VERIFY=$(jq -r .verificationUri "$WORK/dev.json")
VERIFY_FULL=$(jq -r ".verificationUriComplete // \"$VERIFY/$USER_CODE\"" "$WORK/dev.json")
INTERVAL=$(jq -r '.interval // 5' "$WORK/dev.json")
EXPIRES=$(jq -r '.expiresIn // 300' "$WORK/dev.json")
case "$VERIFY_FULL" in http*) ;; *) VERIFY_FULL="https://$VERIFY_FULL" ;; esac
echo
echo "Approve the login at $VERIFY_FULL (code $USER_CODE; neither is written to $OUT)."
copy_code "$USER_CODE" && echo "The code is in your clipboard."
if command -v xdg-open >/dev/null; then xdg-open "$VERIFY_FULL" >/dev/null 2>&1 || true; fi
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
log "### logged in (country $CC)"
log ""

PAGE=(--data-urlencode "deviceType=BROWSER" --data-urlencode "locale=en_US")

# 1. The mixes list: modules, pagedList totals, dataApiPath, mixType values
q "my mixes page" pages/my_collection_my_mixes "${PAGE[@]}"
MIXES=$BODY
jq -c '.rows[]?.modules[]? | {type, title, total: .pagedList.totalNumberOfItems, items: (.pagedList.items | length),
        limit: .pagedList.limit, dataApiPath: .pagedList.dataApiPath,
        mixTypes: ([.pagedList.items[]?.mixType] | unique),
        ids: [.pagedList.items[0:3][]?.id], item_keys: (.pagedList.items[0] | keys?)}' <<<"$MIXES" \
  | sed 's/^/  1 module: /' | tee -a "$OUT" || true
log ""
MIX=$(jq -r '[.rows[]?.modules[]?.pagedList.items[]? | select((.mixType // "") | test("VIDEO") | not) | .id][0] // empty' <<<"$MIXES")
VMIX=$(jq -r '[.rows[]?.modules[]?.pagedList.items[]? | select((.mixType // "") | test("VIDEO")) | .id][0] // empty' <<<"$MIXES")
DPATH=$(jq -r '[.rows[]?.modules[]?.pagedList.dataApiPath // empty][0] // empty' <<<"$MIXES")
log "first audio mix: $MIX; first video mix: ${VMIX:-none}; dataApiPath: ${DPATH:-none}"
log ""
if [ -n "$DPATH" ]; then
  q "my mixes, dataApiPath page 2" "$DPATH" "${PAGE[@]}" --data-urlencode "limit=10" --data-urlencode "offset=10"; summary "1b"
fi

if [ -n "$MIX" ]; then
  # 2. The mix header
  q "pages/mix" pages/mix --data-urlencode "mixId=$MIX" "${PAGE[@]}"
  jq -c '.rows[]?.modules[]? | {type, title, subTitle, mix_keys: (.mix | keys?), total: .pagedList.totalNumberOfItems,
          dataApiPath: .pagedList.dataApiPath}' <<<"$BODY" | sed 's/^/  2 module: /' | tee -a "$OUT" || true
  log ""
  q "mixes/{id}" "mixes/$MIX"; summary "2b"
  q "pages/mix, unknown mix" pages/mix --data-urlencode "mixId=000000000000000000000000000000" "${PAGE[@]}"
  # 3. Items: default page, largest page, depth
  q "mix items, default" "mixes/$MIX/items"; summary "3"
  ids "3 default"
  for n in 50 100 200 1000; do
    q "mix items, limit=$n" "mixes/$MIX/items" --data-urlencode "limit=$n"; summary "3 limit=$n"
  done
  q "mix items, offset=50" "mixes/$MIX/items" --data-urlencode "limit=50" --data-urlencode "offset=50"; summary "3 offset=50"
  q "mix items, offset=100" "mixes/$MIX/items" --data-urlencode "limit=50" --data-urlencode "offset=100"; summary "3 offset=100"
fi
if [ -n "$VMIX" ]; then
  q "video mix items" "mixes/$VMIX/items" --data-urlencode "limit=3"; summary "3 video"
fi
q "mix items, unknown mix" "mixes/000000000000000000000000000000/items"

# 4. Radio: track (paging?) and artist; the artist's ARTIST_MIX for comparison
q "track radio, limit=100" "tracks/$TRACK/radio" --data-urlencode "limit=100"; summary "4 track"
q "track radio, offset=50" "tracks/$TRACK/radio" --data-urlencode "limit=50" --data-urlencode "offset=50"; summary "4 track offset"
q "artist radio, default" "artists/$ARTIST/radio"; summary "4 artist"
q "artist radio, limit=100" "artists/$ARTIST/radio" --data-urlencode "limit=100"; summary "4 artist 100"
ids "4 artist radio"
q "artist radio, limit=1000" "artists/$ARTIST/radio" --data-urlencode "limit=1000"; summary "4 artist 1000"
q "artist radio, unknown artist" "artists/1/radio" --data-urlencode "limit=10"
q "artist" "artists/$ARTIST"
AMIX=$(jq -r '.mixes.ARTIST_MIX // empty' <<<"$BODY")
log "artist mixes: $(jq -c '.mixes' <<<"$BODY" 2>/dev/null)"
if [ -n "$AMIX" ]; then
  q "artist mix items" "mixes/$AMIX/items" --data-urlencode "limit=100"; summary "4 artist mix"
  ids "4 artist mix"
fi

log "# done"
echo
echo "Wrote $OUT. Paste it back (it holds no token, user ID or name; mix titles name artists you listen to)."
