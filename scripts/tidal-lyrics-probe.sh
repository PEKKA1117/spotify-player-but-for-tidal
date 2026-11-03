#!/usr/bin/env bash
# One-off probe for issue #54 (lyrics page), before its spec: does
# GET /v1/tracks/{id}/lyrics exist, what does it answer (lyrics, subtitles in
# LRC, provider, isRightToLeft), with which token (the device-flow client's or
# the PKCE client's the player refreshes with), on api.tidal.com and
# listen.tidal.com, for a track with lyrics, an instrumental, a right-to-left
# track, a CJK track and an unknown track; and whether openapi v2 has them.
# Run once, keep lyrics-probe-output.txt, delete the script once the spec is
# written.
#
# Usage: scripts/tidal-lyrics-probe.sh
#
# Login: the device-flow link (with the code in it) is opened with xdg-open
# and the code is copied with wl-copy (or xclip/xsel), when those exist; just
# approve in the browser.
#
# About 30 requests, spaced 1 s apart (no bursts). Read-only: nothing in the
# account changes. It stops at once on a 429 or a 403 mentioning "abuse".
#
# Lyrics are copyrighted: `lyrics` and `subtitles` are cut to their first 4
# lines, with their line and byte counts noted. Tokens, user IDs and names are
# redacted. Requires: bash, curl, jq; optional: xdg-open, wl-copy (or xclip,
# xsel).

set -euo pipefail

CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
PKCE_CLIENT_ID="6BDSRdpK9hqEBTgU"
PKCE_CLIENT_SECRET="xeuPmY7nbpZ9IIbLAcQ93shka1VNheUAqN6IcszjTG8="
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
LISTEN="https://listen.tidal.com/v1"
OPENAPI="https://openapi.tidal.com/v2"
SCOPE="r_usr w_usr w_sub"
OUT="lyrics-probe-output.txt"

for bin in curl jq; do
  command -v "$bin" >/dev/null || { echo "missing: $bin" >&2; exit 1; }
done
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
: >"$OUT"

redact_json='walk(if type == "object" then with_entries(
  if (.key | test("token|Token|deviceCode|userCode|sessionId|userId|user_id|email|username|firstName|lastName|^sub$"; ""))
  then .value |= "<redacted len=\(tostring | length)>" else . end) else . end)'
# Lyrics text: first 4 lines kept, the rest counted.
cut_lyrics='walk(if type == "object" then with_entries(
  if (.key | test("^(lyrics|subtitles|text)$")) and (.value | type == "string")
  then .value |= ((split("\n")) as $l | ($l[0:4] | join("\n")) + "\n… \($l | length) lines, \(length) bytes in all")
  else . end) else . end)'
# Keep the shape: every array cut to its first 2 elements, with its length noted.
shorten='walk(if type == "array" and length > 2 then (.[0:2] + ["… \(length) items in all"]) else . end)'

log() { printf '%s\n' "$*" | tee -a "$OUT"; }

# q NAME URL TOKEN [curl args...] → GET URL with countryCode, Bearer TOKEN
q() {
  local name=$1 url=$2 token=$3; shift 3
  local tmp; tmp=$(mktemp)
  STATUS=$(curl -sS -G -o "$tmp" -D "$WORK/hdr" -w '%{http_code}' -H "Authorization: Bearer $token" \
    --data-urlencode "countryCode=$CC" "$@" "$url" || echo curl-error)
  BODY=$(cat "$tmp"); rm -f "$tmp"
  log "### $name"
  local shown=${url#https://}; [ -n "${UID_:-}" ] && shown=${shown//$UID_/<uid>}
  log "GET $shown $*"
  log "status: $STATUS  content-type: $(grep -i '^content-type:' "$WORK/hdr" | cut -d' ' -f2- | tr -d '\r')"
  if jq -e . >/dev/null 2>&1 <<<"$BODY"; then
    jq "$redact_json | $cut_lyrics | $shorten" <<<"$BODY" | tee -a "$OUT"
  else
    log "non-JSON body (${#BODY} bytes): ${BODY:0:200}"
  fi
  log ""
  if [ "$STATUS" = 429 ] || { [ "$STATUS" = 403 ] && grep -qi abuse <<<"$BODY"; }; then
    log "STOPPING: Tidal answered $STATUS (rate limit / abuse detection)"; exit 1
  fi
  sleep 1
}

# shape LABEL: the facts the spec needs from a lyrics answer
shape() {
  jq -c '{keys: keys, provider: .lyricsProvider, rtl: .isRightToLeft,
          lyrics_lines: ((.lyrics // "") | split("\n") | length),
          subtitles_lines: ((.subtitles // "") | split("\n") | length),
          stamp_forms: ([(.subtitles // "") | split("\n")[] | capture("^(?<s>\\[[^\\]]*\\])").s
                         | gsub("[0-9]"; "9")] | unique),
          blank_subtitle_lines: ([(.subtitles // "") | split("\n")[] | select(test("^\\[[^\\]]*\\]\\s*$"))] | length),
          last_stamp: ([(.subtitles // "") | split("\n")[] | capture("^(?<s>\\[[^\\]]*\\])").s] | last),
          crlf: ((.lyrics // "") + (.subtitles // "") | test("\r"))}' <<<"$BODY" 2>/dev/null \
    | sed "s/^/  $1: /" | tee -a "$OUT" || true
  log ""
}

# find QUERY → first track ID of a track search, logged with its title
find_track() {
  q "search: $1" "$API/search/tracks" "$ACCESS" --data-urlencode "query=$1" --data-urlencode "limit=1"
  FOUND=$(jq -r '.items[0].id // empty' <<<"$BODY")
  log "  → $(jq -c '.items[0] | {id, title, artist: .artist.name, duration, mediaMetadata, keys: (keys | map(select(test("yric|ubtit|explicit"; "i"))))}' <<<"$BODY" 2>/dev/null)"
  log ""
}

copy_code() {
  if command -v wl-copy >/dev/null; then printf '%s' "$1" | wl-copy
  elif command -v xclip >/dev/null; then printf '%s' "$1" | xclip -selection clipboard
  elif command -v xsel >/dev/null; then printf '%s' "$1" | xsel --clipboard --input
  else return 1; fi
}

log "# tidal-lyrics-probe $(date -u +%FT%TZ)"
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
REFRESH=$(jq -r .refresh_token "$WORK/tok.json")
CC=$(jq -r .user.countryCode "$WORK/tok.json")
UID_=$(jq -r .user.userId "$WORK/tok.json")
log "### logged in (country $CC)"
log ""

# The token the player actually uses: refreshed with the PKCE client (0002 AC19)
STATUS=$(curl -sS -o "$WORK/pk.json" -w '%{http_code}' -X POST "$AUTH/token" -u "$PKCE_CLIENT_ID:$PKCE_CLIENT_SECRET" \
  --data-urlencode "client_id=$PKCE_CLIENT_ID" --data-urlencode "refresh_token=$REFRESH" \
  --data-urlencode "grant_type=refresh_token" --data-urlencode "scope=$SCOPE" || echo curl-error)
PKCE=$(jq -r '.access_token // empty' "$WORK/pk.json")
log "### PKCE refresh: status $STATUS, token ${PKCE:+obtained}"
log ""

q "subscription" "$API/users/$UID_/subscription" "$ACCESS"
log "  → $(jq -c '{type: .subscription.type, highestSoundQuality, status}' <<<"$BODY" 2>/dev/null)"
log ""

# 1. Tracks to try, found by search (no guessed IDs)
find_track "Adele Hello";              T_POP=${FOUND:-}
find_track "Fairuz Kifak Inta";        T_RTL=${FOUND:-}
find_track "Jay Chou Qing Tian";       T_CJK=${FOUND:-}
find_track "Brian Eno 1/1";            T_INST=${FOUND:-}
find_track "Bohemian Rhapsody Queen";  T_LONG=${FOUND:-}
T_UNKNOWN=1

# 2. The endpoint, device-flow token, api.tidal.com
for t in "$T_POP:popular" "$T_RTL:right-to-left" "$T_CJK:CJK" "$T_INST:instrumental" "$T_LONG:long" "$T_UNKNOWN:unknown"; do
  id=${t%%:*}; label=${t#*:}
  [ -n "$id" ] || { log "### lyrics, $label: no track found"; log ""; continue; }
  q "lyrics, $label ($id)" "$API/tracks/$id/lyrics" "$ACCESS"; shape "2 $label"
done

# 3. Token, host and parameters
if [ -n "$T_POP" ]; then
  if [ -n "$PKCE" ]; then
    q "lyrics, popular, PKCE token" "$API/tracks/$T_POP/lyrics" "$PKCE"; shape "3 pkce"
  fi
  q "lyrics, popular, listen.tidal.com" "$LISTEN/tracks/$T_POP/lyrics" "$ACCESS"; shape "3 listen"
  q "lyrics, popular, deviceType+locale" "$API/tracks/$T_POP/lyrics" "$ACCESS" \
    --data-urlencode "deviceType=BROWSER" --data-urlencode "locale=en_US"; shape "3 params"
  q "lyrics, popular, no token" "$API/tracks/$T_POP/lyrics" "" ; shape "3 anon"
  # Does a track carry anything that says it has lyrics?
  q "track, popular" "$API/tracks/$T_POP" "$ACCESS"
  log "  track keys: $(jq -c 'keys' <<<"$BODY" 2>/dev/null)"
  log ""
  # 4. openapi v2
  q "v2 track include=lyrics" "$OPENAPI/tracks/$T_POP" "${PKCE:-$ACCESS}" \
    -H "Accept: application/vnd.api+json" --data-urlencode "include=lyrics"
  q "v2 track relationships/lyrics" "$OPENAPI/tracks/$T_POP/relationships/lyrics" "${PKCE:-$ACCESS}" \
    -H "Accept: application/vnd.api+json"
  q "v2 lyrics/{id}" "$OPENAPI/lyrics/$T_POP" "${PKCE:-$ACCESS}" -H "Accept: application/vnd.api+json"
fi

log "# done"
echo
echo "Wrote $OUT (no token, user ID or name; lyrics cut to 4 lines each)."
