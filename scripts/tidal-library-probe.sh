#!/usr/bin/env bash
# One-off probe for spec 0006 ("Facts vs. assumptions"). Run once with a real
# Tidal account, then paste library-probe-output.txt back. Delete this script
# once the spec's fixtures are recorded.
#
# Usage: scripts/tidal-library-probe.sh ARTIST_ID ALBUM_ID PLAYLIST_UUID
#   ARTIST_ID      an artist with albums AND singles/EPs: https://tidal.com/browse/artist/<id>
#   ALBUM_ID       any album: https://tidal.com/browse/album/<id>
#   PLAYLIST_UUID  any playlist: https://tidal.com/browse/playlist/<uuid>
# Works best on an account with more than 10 favorite tracks, albums and
# artists, and with both its own and favorited (followed) playlists.
#
# What it does, against the live API, using the same public client as the app,
# about 35 GET requests spaced 1 s apart (it stops at once on a 429). Nothing
# is written to the account:
#   1. logs in with the device flow (approve the code in your browser)
#   2. the library lists: /users/{you}/playlistsAndFavoritePlaylists, /playlists,
#      /favorites/playlists, /favorites/{tracks,albums,artists}: default page,
#      limit=3&offset=2, the largest page accepted (1000? 10000?), the order
#      parameters, and another user's favorites (refused?)
#   3. /albums/{id}, /playlists/{uuid}, unknown IDs
#   4. /artists/{id}, /toptracks, /albums (plain, EPSANDSINGLES, COMPILATIONS),
#      /similar, and an unknown artist
# Lists are cut to their first 2 items in the output; the keys of an item, the
# item types and the paging fields (limit, offset, totalNumberOfItems) are kept.
#
# Tokens, codes, your user ID, names and e-mail are redacted; catalogue data
# (track, album and artist titles) is kept, since fixtures are written from it
# with the IDs and names replaced by fakes. Titles and descriptions of
# playlists (yours and followed ones) and their creators are redacted.
# Requires: bash, curl, jq.
#
# Logging in creates a session on your account that the script cannot revoke
# (Tidal refuses revocation for this client, see spec 0002 "Facts"). It expires
# server-side like any other.

set -euo pipefail

[ $# -eq 3 ] || { sed -n '6,9p' "$0" >&2; exit 2; }
ARTIST_ID=$1
ALBUM_ID=$2
PLAYLIST=$3

CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
SCOPE="r_usr w_usr w_sub"
OUT="library-probe-output.txt"

for bin in curl jq; do
  command -v "$bin" >/dev/null || { echo "missing: $bin" >&2; exit 1; }
done
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
: >"$OUT"

redact_json='walk(if type == "object" then with_entries(
  if (.key | test("token|Token|deviceCode|userCode|sessionId|userId|user_id|email|username|firstName|lastName|^sub$|partnerId|^creator$|^promotedArtists$"; ""))
  then .value |= "<redacted>" else . end) else . end)'
# Playlist titles, descriptions and images (private for your own playlists).
pl_redact='walk(if type == "object" and has("uuid") then with_entries(
  if (.key | test("^(title|description|image|squareImage|url)$"; "")) and (.value | type) == "string"
  then .value |= "<redacted>" else . end) else . end)'
# Lists cut to 2 items, with the keys of the first item and the type counts.
trim_json='if type == "object" and has("items") then
  .itemKeys = ((.items[0] // {}) | keys)
  | .innerKeys = ((.items[0] // {}) | (.item // .playlist // {}) | if type == "object" then keys else [] end)
  | .itemCount = (.items | length)
  | .itemTypes = ([.items[] | (.type // .item.type // "none") | tostring] | group_by(.) | map({(.[0]): length}) | add)
  | .items |= .[:2]
  else . end'

log() { printf '%s\n' "$*" | tee -a "$OUT"; }

# req NAME URL [extra jq filter] → GET with the bearer, logs status and the
# redacted, trimmed body. Sets BODY (untrimmed) for follow-up filters.
req() {
  local name=$1 url=$2 extra=${3:-.}
  sleep 1
  local tmp; tmp=$(mktemp)
  STATUS=$(curl -sS -o "$tmp" -w '%{http_code}' -H "Authorization: Bearer $ACCESS" "$url" || echo "curl-error")
  BODY=$(cat "$tmp"); rm -f "$tmp"
  log "### $name"
  log "GET $(sed -E -e "s#/users/$USER_ID/#/users/<you>/#" -e 's/countryCode=[A-Z]+/countryCode=<cc>/' <<<"$url" | sed -E "s#^$API##")"
  log "status: $STATUS"
  if jq -e . >/dev/null 2>&1 <<<"$BODY"; then
    jq "$trim_json | $extra | $pl_redact | $redact_json" <<<"$BODY" | tee -a "$OUT"
  else
    log "non-JSON body (${#BODY} bytes): ${BODY:0:200}"
  fi
  log ""
  [ "$STATUS" = 429 ] && { log "stopped: rate limited"; exit 1; }
  return 0
}

# order NAME → logs the `created` dates and the first item's title of every
# entry of BODY, so the sort order can be read off.
order() {
  log "### $1: created dates in page order"
  jq -c '[.items[]? | .created // .dateAdded // .item.dateAdded // "none"]' <<<"$BODY" | tee -a "$OUT"
  log ""
}

log "# tidal-library-probe $(date -u +%FT%TZ)"
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
USER_ID=$(jq -r '.user.userId // .user_id' "$WORK/tok.json")
log "### logged in (client $(jq -r '.clientName // "?"' "$WORK/tok.json"))"
log ""
U="$API/users/$USER_ID"
Q="countryCode=$CC"
DESC="order=DATE&orderDirection=DESC"

# 2. Library lists
req "playlists and favorite playlists, default page" "$U/playlistsAndFavoritePlaylists?$Q"
req "playlists and favorite playlists, limit=3 offset=2" "$U/playlistsAndFavoritePlaylists?$Q&limit=3&offset=2"
req "playlists and favorite playlists, limit=1000" "$U/playlistsAndFavoritePlaylists?$Q&limit=1000"
req "playlists and favorite playlists, limit=50 $DESC" "$U/playlistsAndFavoritePlaylists?$Q&limit=50&$DESC"
order "playlists and favorite playlists, DATE DESC"
req "own playlists (tidalt's endpoint), limit=50" "$U/playlists?$Q&limit=50"
req "favorite playlists, limit=50" "$U/favorites/playlists?$Q&limit=50"

req "favorite tracks, default page" "$U/favorites/tracks?$Q"
order "favorite tracks, default order"
req "favorite tracks, limit=3 offset=2 $DESC" "$U/favorites/tracks?$Q&limit=3&offset=2&$DESC"
req "favorite tracks, limit=50 $DESC" "$U/favorites/tracks?$Q&limit=50&$DESC"
order "favorite tracks, DATE DESC"
req "favorite tracks, limit=50 order=DATE orderDirection=ASC" "$U/favorites/tracks?$Q&limit=50&order=DATE&orderDirection=ASC"
order "favorite tracks, DATE ASC"
req "favorite tracks, limit=1000" "$U/favorites/tracks?$Q&limit=1000"
req "favorite tracks, limit=10000" "$U/favorites/tracks?$Q&limit=10000"

req "favorite albums, default page" "$U/favorites/albums?$Q"
req "favorite albums, limit=50 $DESC" "$U/favorites/albums?$Q&limit=50&$DESC"
order "favorite albums, DATE DESC"
req "favorite albums, limit=1000" "$U/favorites/albums?$Q&limit=1000"

req "favorite artists, default page" "$U/favorites/artists?$Q"
req "favorite artists, limit=50 $DESC" "$U/favorites/artists?$Q&limit=50&$DESC"
order "favorite artists, DATE DESC"
req "favorite artists, limit=1000" "$U/favorites/artists?$Q&limit=1000"

req "another user's favorite tracks (user 1)" "$API/users/1/favorites/tracks?$Q&limit=3"

# 3. Album and playlist headers
req "album" "$API/albums/$ALBUM_ID?$Q"
req "unknown album" "$API/albums/1?$Q"
req "playlist" "$API/playlists/$PLAYLIST?$Q"
req "unknown playlist" "$API/playlists/00000000-0000-0000-0000-000000000000?$Q"

# 4. Artist
req "artist" "$API/artists/$ARTIST_ID?$Q"
req "unknown artist" "$API/artists/1?$Q"
req "artist top tracks, default page" "$API/artists/$ARTIST_ID/toptracks?$Q"
req "artist top tracks, limit=100" "$API/artists/$ARTIST_ID/toptracks?$Q&limit=100"
req "artist top tracks, limit=1000" "$API/artists/$ARTIST_ID/toptracks?$Q&limit=1000"
req "artist albums, default page" "$API/artists/$ARTIST_ID/albums?$Q"
req "artist albums, limit=1000" "$API/artists/$ARTIST_ID/albums?$Q&limit=1000"
req "artist albums, limit=100" "$API/artists/$ARTIST_ID/albums?$Q&limit=100"
req "artist EPs and singles, limit=100" "$API/artists/$ARTIST_ID/albums?$Q&limit=100&filter=EPSANDSINGLES"
req "artist compilations, limit=100" "$API/artists/$ARTIST_ID/albums?$Q&limit=100&filter=COMPILATIONS"
req "similar artists" "$API/artists/$ARTIST_ID/similar?$Q&limit=50"
req "unknown artist's albums" "$API/artists/1/albums?$Q"

echo
echo "Done. Check $OUT for anything private, then paste it back."
