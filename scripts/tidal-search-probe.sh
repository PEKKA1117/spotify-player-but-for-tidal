#!/usr/bin/env bash
# One-off probe for spec 0007 (search), "Not verified": the /v1/search
# envelope (topHit included), the largest page per list, how deep offset goes,
# the per-type endpoints, and odd queries. Run once, paste back
# search-probe-output.txt, delete the script once the spec is updated.
#
# Usage: scripts/tidal-search-probe.sh
#
# Login: the device-flow link (with the code in it) is opened with xdg-open
# and the code is copied with wl-copy (or xclip/xsel), when those exist; just
# approve in the browser.
#
# About 25 GET requests, spaced 1 s apart (no bursts). Read-only: nothing in
# the account changes. It stops at once on a 429 or a 403 mentioning "abuse".
#
# Search results are public catalogue data and are logged as is, every array
# cut to its first 2 items; tokens, user IDs and names are redacted.
# Requires: bash, curl, jq; optional: xdg-open, wl-copy (or xclip, xsel).

set -euo pipefail

CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
SCOPE="r_usr w_usr w_sub"
OUT="search-probe-output.txt"

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

copy_code() {
  if command -v wl-copy >/dev/null; then printf '%s' "$1" | wl-copy
  elif command -v xclip >/dev/null; then printf '%s' "$1" | xclip -selection clipboard
  elif command -v xsel >/dev/null; then printf '%s' "$1" | xsel --clipboard --input
  else return 1; fi
}

log "# tidal-search-probe $(date -u +%FT%TZ)"
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

ALL="TRACKS,ALBUMS,ARTISTS,PLAYLISTS"

# 1. The combined envelope (topHit, videos?), default page, and tidalt's exact call
q "search, all types, no limit" search --data-urlencode "query=pierce the veil" --data-urlencode "types=$ALL"; summary "1"
q "search, tidalt's call (limit=20)" search --data-urlencode "query=pierce the veil" --data-urlencode "types=$ALL" --data-urlencode "limit=20"; summary "1b"
q "search, no types" search --data-urlencode "query=pierce the veil" --data-urlencode "limit=3"; summary "1c"
# 2. Does types limit the lists? Does topHit follow it?
q "search, types=TRACKS only" search --data-urlencode "query=pierce the veil" --data-urlencode "types=TRACKS"; summary "2"
# Top hit kinds: an album title, a track title, a playlist name
q "search, album title" search --data-urlencode "query=collide with the sky" --data-urlencode "types=$ALL" --data-urlencode "limit=3"; summary "2 album"
q "search, track title" search --data-urlencode "query=hell above" --data-urlencode "types=$ALL" --data-urlencode "limit=3"; summary "2 track"
q "search, playlist name" search --data-urlencode "query=this is pierce the veil" --data-urlencode "types=$ALL" --data-urlencode "limit=3"; summary "2 playlist"
# 3. Largest page on the combined endpoint
for n in 50 100 300 1000; do
  q "search, limit=$n" search --data-urlencode "query=love" --data-urlencode "types=$ALL" --data-urlencode "limit=$n"; summary "3 limit=$n"
done
# 4. Per-type endpoints, largest page
for t in tracks albums artists playlists; do
  q "search/$t, limit=100" "search/$t" --data-urlencode "query=love" --data-urlencode "limit=100"; summary "4 $t"
done
q "search/tracks, limit=1000" search/tracks --data-urlencode "query=love" --data-urlencode "limit=1000"; summary "4 tracks 1000"
# 5. Depth: does offset stop before the total?
for off in 250 300 1000 10000; do
  q "search/tracks, offset=$off" search/tracks --data-urlencode "query=love" --data-urlencode "limit=50" --data-urlencode "offset=$off"; summary "5 offset=$off"
done
q "search (combined), offset=300" search --data-urlencode "query=love" --data-urlencode "types=$ALL" --data-urlencode "limit=50" --data-urlencode "offset=300"; summary "5 combined"
# 6. Odd queries
q "search, empty query" search --data-urlencode "query=" --data-urlencode "types=$ALL"; summary "6 empty"
q "search, one letter" search --data-urlencode "query=a" --data-urlencode "types=$ALL" --data-urlencode "limit=3"; summary "6 one"
q "search, no match" search --data-urlencode "query=zzqxjvkwpq" --data-urlencode "types=$ALL"; summary "6 none"
q "search, AC/DC" search --data-urlencode "query=AC/DC" --data-urlencode "types=$ALL" --data-urlencode "limit=3"; summary "6 acdc"
q "search, Sigur Rós" search --data-urlencode "query=Sigur Rós" --data-urlencode "types=$ALL" --data-urlencode "limit=3"; summary "6 sigur"
q "search, 米津玄師" search --data-urlencode "query=米津玄師" --data-urlencode "types=$ALL" --data-urlencode "limit=3"; summary "6 cjk"
q "search, 200 chars" search --data-urlencode "query=$(printf 'love%.0s' {1..50})" --data-urlencode "types=$ALL" --data-urlencode "limit=3"; summary "6 long"

log "# done"
echo
echo "Wrote $OUT. Paste it back (it holds no token, user ID or name)."
