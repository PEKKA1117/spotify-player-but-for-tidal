#!/usr/bin/env bash
# One-off WRITING probe for spec 0006 (favorites and playlist editing). Run
# once, paste library-write-probe-output.txt back, delete the script once the
# spec's fixtures are recorded.
#
# Usage: scripts/tidal-library-write-probe.sh TRACK_ID TRACK_ID2 ALBUM_ID ARTIST_ID PLAYLIST_UUID
#   all five must NOT be in your favorites now (the script adds, then removes them);
#   PLAYLIST_UUID is someone else's public playlist (e.g. an editorial one)
#
# What it changes on your account, and undoes before it ends (about 30
# requests, 1 s apart; stops at once on a 429):
#   1. favorites: POST /users/{you}/favorites/{tracks,albums,artists,playlists},
#      the same again (a duplicate add), read back, DELETE each, DELETE again
#      (removing what is not there), read back
#   2. playlists: creates a private playlist "tidal-player probe (delete me)",
#      adds TRACK_ID and TRACK_ID2 (with and without the ETag), adds TRACK_ID
#      again (duplicates), removes index 0, tries a stale ETag, then deletes the
#      playlist
# If it stops midway, remove the probe playlist and the favorites by hand.
# Tokens, your user ID and names are redacted. Requires: bash, curl, jq.

set -euo pipefail

[ $# -eq 5 ] || { sed -n '6,8p' "$0" >&2; exit 2; }
T1=$1; T2=$2; ALBUM=$3; ARTIST=$4; PL=$5

CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
SCOPE="r_usr w_usr w_sub"
OUT="library-write-probe-output.txt"

for bin in curl jq; do command -v "$bin" >/dev/null || { echo "missing: $bin" >&2; exit 1; }; done
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
: >"$OUT"

redact_json='walk(if type == "object" then with_entries(
  if (.key | test("token|Token|deviceCode|userCode|sessionId|userId|user_id|email|username|firstName|lastName|^sub$|^creator$|^promotedArtists$|image|url"; ""))
  then .value |= "<redacted>" else . end) else . end)'
trim_json='if type == "object" and has("items") then .itemCount = (.items | length)
  | .ids = [.items[] | (.item // .) | (.id // .uuid)] | del(.items) else . end'
log() { printf '%s\n' "$*" | tee -a "$OUT"; }

# req NAME METHOD URL [curl args...] → logs status, ETag and the redacted body.
req() {
  local name=$1 method=$2 url=$3; shift 3
  sleep 1
  STATUS=$(curl -sS -X "$method" -D "$WORK/h" -o "$WORK/b" -w '%{http_code}' \
    -H "Authorization: Bearer $ACCESS" "$url" "$@" || echo curl-error)
  BODY=$(cat "$WORK/b"); ETAG=$(tr -d '\r' <"$WORK/h" | sed -n 's/^[Ee][Tt][Aa][Gg]: //p' | tail -1)
  log "### $name"
  log "$method $(sed -E -e "s#/users/$USER_ID/#/users/<you>/#" -e 's/countryCode=[A-Z]+/countryCode=<cc>/' -e "s#^$API##" <<<"$url")"
  log "status: $STATUS  etag: ${ETAG:-none}"
  if [ -n "$BODY" ] && jq -e . >/dev/null 2>&1 <<<"$BODY"; then
    jq "$trim_json | $redact_json" <<<"$BODY" | tee -a "$OUT"
  elif [ -n "$BODY" ]; then log "non-JSON body: ${BODY:0:200}"; fi
  log ""
  [ "$STATUS" = 429 ] && { log "stopped: rate limited"; exit 1; }
  return 0
}

log "# tidal-library-write-probe $(date -u +%FT%TZ)"
curl -sS -X POST "$AUTH/device_authorization" \
  --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "scope=$SCOPE" >"$WORK/dev.json"
DEVICE_CODE=$(jq -r .deviceCode "$WORK/dev.json"); INTERVAL=$(jq -r '.interval // 5' "$WORK/dev.json")
echo; echo "Open https://$(jq -r .verificationUri "$WORK/dev.json") and enter the code $(jq -r .userCode "$WORK/dev.json")."
deadline=$(( $(date +%s) + $(jq -r '.expiresIn // 300' "$WORK/dev.json") ))
while :; do
  sleep "$INTERVAL"
  S=$(curl -sS -o "$WORK/tok.json" -w '%{http_code}' -X POST "$AUTH/token" -u "$CLIENT_ID:$CLIENT_SECRET" \
    --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "device_code=$DEVICE_CODE" \
    --data-urlencode "grant_type=urn:ietf:params:oauth:grant-type:device_code" --data-urlencode "scope=$SCOPE" || echo x)
  [ "$S" = 200 ] && break
  [ "$(date +%s)" -ge "$deadline" ] && { log "gave up: code expired"; exit 1; }
done
ACCESS=$(jq -r .access_token "$WORK/tok.json"); CC=$(jq -r .user.countryCode "$WORK/tok.json")
USER_ID=$(jq -r '.user.userId // .user_id' "$WORK/tok.json")
U="$API/users/$USER_ID"; Q="countryCode=$CC"
log "### logged in"; log ""

# 1. Favorites: kind, form field, id
for spec in "tracks trackIds $T1" "albums albumIds $ALBUM" "artists artistIds $ARTIST" "playlists uuids $PL"; do
  set -- $spec; kind=$1; field=$2; id=$3
  req "add favorite $kind" POST "$U/favorites/$kind?$Q" --data-urlencode "$field=$id"
  req "add favorite $kind again (duplicate)" POST "$U/favorites/$kind?$Q" --data-urlencode "$field=$id"
  req "favorite $kind, newest first (is it first?)" GET "$U/favorites/$kind?$Q&limit=3&order=DATE&orderDirection=DESC"
  req "remove favorite $kind" DELETE "$U/favorites/$kind/$id?$Q"
  req "remove favorite $kind again (not there)" DELETE "$U/favorites/$kind/$id?$Q"
done
req "add two favorite tracks in one request (comma list)" POST "$U/favorites/tracks?$Q" --data-urlencode "trackIds=$T1,$T2"
req "favorite tracks, newest first" GET "$U/favorites/tracks?$Q&limit=3&order=DATE&orderDirection=DESC"
req "remove favorite track 1" DELETE "$U/favorites/tracks/$T1?$Q"
req "remove favorite track 2" DELETE "$U/favorites/tracks/$T2?$Q"
req "add an unknown track to favorites" POST "$U/favorites/tracks?$Q" --data-urlencode "trackIds=1"

# 2. Playlists
req "create playlist" POST "$U/playlists?$Q" --data-urlencode "title=tidal-player probe (delete me)" \
  --data-urlencode "description=temporary, made by scripts/tidal-library-write-probe.sh"
NEW=$(jq -r '.uuid // empty' <<<"$BODY")
[ -n "$NEW" ] || { log "no playlist created; stopping"; exit 1; }
P="$API/playlists/$NEW"
req "playlist header (ETag?)" GET "$P?$Q"; E0=$ETAG
req "add track without ETag" POST "$P/items?$Q" --data-urlencode "trackIds=$T1" --data-urlencode "onDupes=FAIL"
req "add track with a stale ETag" POST "$P/items?$Q" -H "If-None-Match: $E0" --data-urlencode "trackIds=$T2"
req "playlist header (new ETag)" GET "$P?$Q"; E1=$ETAG
req "add track with the current ETag" POST "$P/items?$Q" -H "If-None-Match: $E1" --data-urlencode "trackIds=$T2"
req "playlist header" GET "$P?$Q"; E2=$ETAG
req "add a duplicate, onDupes=FAIL" POST "$P/items?$Q" -H "If-None-Match: $E2" --data-urlencode "trackIds=$T1" --data-urlencode "onDupes=FAIL"
req "playlist header" GET "$P?$Q"; E3=$ETAG
req "add a duplicate, onDupes=ADD, toIndex=0" POST "$P/items?$Q" -H "If-None-Match: $E3" --data-urlencode "trackIds=$T1" --data-urlencode "onDupes=ADD" --data-urlencode "toIndex=0"
req "items" GET "$P/items?$Q&limit=50"
req "playlist header" GET "$P?$Q"; E4=$ETAG
req "remove index 0" DELETE "$P/items/0?$Q" -H "If-None-Match: $E4"
req "remove index 0 again with the old ETag" DELETE "$P/items/0?$Q" -H "If-None-Match: $E4"
req "items" GET "$P/items?$Q&limit=50"
req "playlist header" GET "$P?$Q"; E5=$ETAG
req "delete the probe playlist" DELETE "$P?$Q" -H "If-None-Match: $E5"
req "probe playlist gone?" GET "$P?$Q"
req "edit someone else's playlist (expect refusal)" POST "$API/playlists/$PL/items?$Q" --data-urlencode "trackIds=$T1"

echo; echo "Done. Check $OUT, then paste it back."
