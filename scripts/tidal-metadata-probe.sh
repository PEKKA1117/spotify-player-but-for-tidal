#!/usr/bin/env bash
# One-off probe for spec 0004 ("Facts vs. assumptions"). Run once with a real
# Tidal account, then paste metadata-probe-output.txt back. Delete this script
# once the spec's fixtures are recorded.
#
# Usage: scripts/tidal-metadata-probe.sh TRACK_ID ALBUM_ID PLAYLIST_UUID
#   TRACK_ID       any track: https://tidal.com/browse/track/<id>
#   ALBUM_ID       an album with MORE than 10 tracks: https://tidal.com/browse/album/<id>
#   PLAYLIST_UUID  a playlist with more than 10 tracks, ideally one with a video
#                  in it: https://tidal.com/browse/playlist/<uuid>
#
# What it does, against the live API, using the same public client as the app,
# about 15 requests spaced 1 s apart (it stops at once on a 429):
#   1. logs in with the device flow (approve the code in your browser)
#   2. GET /v1/tracks/{id}, and an unknown track ID
#   3. GET /v1/albums/{id}/tracks with no paging params, with limit=3&offset=2,
#      with limit=1000 (largest accepted page?), and an unknown album ID
#   4. GET /v1/playlists/{uuid}/items and /tracks, the same way, and an unknown UUID
# Lists are cut to their first 2 items in the output; the keys of an item and
# the paging fields (limit, offset, totalNumberOfItems) are kept.
#
# Tokens, codes, user IDs, names and e-mail are redacted; catalogue data (track
# and album titles, artists) is kept, since fixtures are written from it with
# the IDs and names replaced by fakes. Your playlist's title, description and
# creator are redacted. Requires: bash, curl, jq.
#
# Logging in creates a session on your account that the script cannot revoke
# (Tidal refuses revocation for this client, see spec 0002 "Facts"). It expires
# server-side like any other.

set -euo pipefail

[ $# -eq 3 ] || { sed -n '6,10p' "$0" >&2; exit 2; }
TRACK_ID=$1
ALBUM_ID=$2
PLAYLIST=$3

CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
SCOPE="r_usr w_usr w_sub"
OUT="metadata-probe-output.txt"

for bin in curl jq; do
  command -v "$bin" >/dev/null || { echo "missing: $bin" >&2; exit 1; }
done
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
: >"$OUT"

redact_json='walk(if type == "object" then with_entries(
  if (.key | test("token|Token|deviceCode|userCode|sessionId|userId|user_id|email|username|firstName|lastName|^sub$|partnerId|^creator$|^promotedArtists$"; ""))
  then .value |= "<redacted>" else . end) else . end)'
# Lists cut to 2 items, with the keys of the first item listed.
trim_json='if type == "object" and has("items") then
  .itemKeys = ((.items[0] // {}) | keys) | .itemCount = (.items | length)
  | .itemTypes = ([.items[] | .type // "none"] | group_by(.) | map({(.[0]): length}) | add)
  | .items |= .[:2]
  else . end'

log() { printf '%s\n' "$*" | tee -a "$OUT"; }

# req NAME URL [extra jq filter] → GET with the bearer, logs status and the redacted, trimmed body.
req() {
  local name=$1 url=$2 extra=${3:-.}
  sleep 1
  local tmp; tmp=$(mktemp)
  STATUS=$(curl -sS -o "$tmp" -w '%{http_code}' -H "Authorization: Bearer $ACCESS" "$url" || echo "curl-error")
  BODY=$(cat "$tmp"); rm -f "$tmp"
  log "### $name"
  log "GET ${url%%\?*}?$(sed -nE 's#^[^?]*\?(.*)$#\1#p' <<<"$url" | sed -E 's/countryCode=[A-Z]+/countryCode=<cc>/')"
  log "status: $STATUS"
  if jq -e . >/dev/null 2>&1 <<<"$BODY"; then
    jq "$trim_json | $extra | $redact_json" <<<"$BODY" | tee -a "$OUT"
  else
    log "non-JSON body (${#BODY} bytes): ${BODY:0:200}"
  fi
  log ""
  [ "$STATUS" = 429 ] && { log "stopped: rate limited"; exit 1; }
  return 0
}

log "# tidal-metadata-probe $(date -u +%FT%TZ)"
log ""

# 1. Device-flow login
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
log "### logged in (client $(jq -r '.clientName // "?"' "$WORK/tok.json"))"
log ""

# 2. Track
req "track" "$API/tracks/$TRACK_ID?countryCode=$CC"
req "unknown track" "$API/tracks/1?countryCode=$CC"

# 3. Album tracks
req "album" "$API/albums/$ALBUM_ID?countryCode=$CC"
req "album tracks, no paging params" "$API/albums/$ALBUM_ID/tracks?countryCode=$CC"
req "album tracks, limit=3 offset=2" "$API/albums/$ALBUM_ID/tracks?countryCode=$CC&limit=3&offset=2"
req "album tracks, limit=1000" "$API/albums/$ALBUM_ID/tracks?countryCode=$CC&limit=1000"
req "album items, limit=3" "$API/albums/$ALBUM_ID/items?countryCode=$CC&limit=3"
req "unknown album" "$API/albums/1/tracks?countryCode=$CC"

# 4. Playlist
PL_REDACT='walk(if type == "object" then with_entries(
  if (.key | test("^(title|description|image|squareImage|url)$"; "")) and (.value | type) == "string"
  then .value |= "<redacted>" else . end) else . end)'
req "playlist (title and description redacted)" "$API/playlists/$PLAYLIST?countryCode=$CC" "$PL_REDACT"
req "playlist items, no paging params" "$API/playlists/$PLAYLIST/items?countryCode=$CC"
req "playlist items, limit=3 offset=2" "$API/playlists/$PLAYLIST/items?countryCode=$CC&limit=3&offset=2"
req "playlist items, limit=1000" "$API/playlists/$PLAYLIST/items?countryCode=$CC&limit=1000"
req "playlist items, limit=50" "$API/playlists/$PLAYLIST/items?countryCode=$CC&limit=50"
log "### non-track entries of that page (untrimmed)"
jq "[.items[] | select(.type != \"track\")] | $redact_json" <<<"$BODY" | tee -a "$OUT"
log ""
req "playlist tracks (tidalt's endpoint), limit=50: itemCount 39 = videos left out, 41 = included" \
  "$API/playlists/$PLAYLIST/tracks?countryCode=$CC&limit=50"
req "unknown playlist" "$API/playlists/00000000-0000-0000-0000-000000000000/items?countryCode=$CC"

echo
echo "Done. Check $OUT for anything private, then paste it back."
