#!/usr/bin/env bash
# One-off probe for spec 0004, decision 4 (autoplay). Run once with a real Tidal
# account, then paste autoplay-probe-output.txt back. Delete this script once
# the spec's fixtures are recorded.
#
# Usage: scripts/tidal-autoplay-probe.sh TRACK_ID
#   TRACK_ID  any track you would want autoplay to continue from
#
# Which endpoint gives "more like this" tracks after a queue ends? About 9
# requests, 1 s apart (stops at once on a 429), after a device-flow login:
#   1. GET /v1/tracks/{id}                      (for its mixes.TRACK_MIX ID)
#   2. GET /v1/tracks/{id}/radio                no paging params, limit=10, limit=100
#   3. GET /v1/mixes/{TRACK_MIX}/items          no paging params, limit=100
#   4. both for an unknown track (ID 1)
# For each list: paging fields, item count and types, the first 2 items, all
# track IDs, and whether the seed track is in it. Tokens and personal data are
# redacted as in tidal-metadata-probe.sh. Requires: bash, curl, jq.

set -euo pipefail

[ $# -eq 1 ] || { sed -n '6,7p' "$0" >&2; exit 2; }
TRACK_ID=$1

CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
SCOPE="r_usr w_usr w_sub"
OUT="autoplay-probe-output.txt"

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

log "# tidal-autoplay-probe $(date -u +%FT%TZ)"
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


# The lists are trimmed by req, so the IDs are computed on the raw body here.
list() {
  req "$1" "$2"
  if jq -e .items >/dev/null 2>&1 <<<"$BODY"; then
    jq -c "{allIds: [.items[] | (.item // .) | .id], seedIncluded: ([.items[] | (.item // .) | .id] | index($TRACK_ID) != null)}" <<<"$BODY" | tee -a "$OUT"
    log ""
  fi
}

req "track" "$API/tracks/$TRACK_ID?countryCode=$CC"
MIX=$(jq -r '.mixes.TRACK_MIX // empty' <<<"$BODY")
log "### TRACK_MIX: ${MIX:-<none>}"
log ""

list "track radio, no paging params" "$API/tracks/$TRACK_ID/radio?countryCode=$CC"
list "track radio, limit=10" "$API/tracks/$TRACK_ID/radio?countryCode=$CC&limit=10"
list "track radio, limit=100" "$API/tracks/$TRACK_ID/radio?countryCode=$CC&limit=100"
if [ -n "$MIX" ]; then
  list "track mix items, no paging params" "$API/mixes/$MIX/items?countryCode=$CC"
  list "track mix items, limit=100" "$API/mixes/$MIX/items?countryCode=$CC&limit=100"
fi
req "track radio, unknown track" "$API/tracks/1/radio?countryCode=$CC"
req "mix items, unknown mix" "$API/mixes/0000000000000000000000000000ff/items?countryCode=$CC"

echo
echo "Done. Check $OUT for anything private, then paste it back."
