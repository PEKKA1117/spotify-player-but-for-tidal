#!/usr/bin/env bash
# One-off READ-ONLY probe for spec 0006 decision 6: is there an endpoint for
# all of an artist's tracks (the Tidal apps' "Credits for <artist>")? Paste
# back artist-tracks-probe-output.txt; delete the script once recorded.
#
# Usage: scripts/tidal-artist-tracks-probe.sh ARTIST_ID
# About 12 GET requests, 1 s apart. Nothing is changed. Requires: bash, curl, jq.

set -euo pipefail
[ $# -eq 1 ] || { sed -n '6p' "$0" >&2; exit 2; }
ARTIST=$1
CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
V2="https://openapi.tidal.com/v2"
SCOPE="r_usr w_usr w_sub"
OUT="artist-tracks-probe-output.txt"
for bin in curl jq; do command -v "$bin" >/dev/null || { echo "missing: $bin" >&2; exit 1; }; done
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
: >"$OUT"
log() { printf '%s\n' "$*" | tee -a "$OUT"; }
redact='walk(if type == "object" then with_entries(if (.key | test("token|userId|user_id|email|username"; "")) then .value |= "<redacted>" else . end) else . end)'
# Cut every array to 3 entries, but report its length.
trim='walk(if type == "array" and length > 3 then .[:3] + ["… \(length) in total"] else . end)'

req() {
  local name=$1 url=$2; shift 2
  sleep 1
  STATUS=$(curl -sS -o "$WORK/b" -w '%{http_code}' -H "Authorization: Bearer $ACCESS" "$@" "$url" || echo curl-error)
  log "### $name"; log "GET $(sed -E 's/countryCode=[A-Z]+/countryCode=<cc>/' <<<"$url")"; log "status: $STATUS"
  if jq -e . >/dev/null 2>&1 <"$WORK/b"; then jq "$trim | $redact" <"$WORK/b" | head -c 6000 | tee -a "$OUT"; echo | tee -a "$OUT"
  else log "non-JSON body: $(head -c 200 "$WORK/b")"; fi
  log ""
  [ "$STATUS" = 429 ] && { log "stopped: rate limited"; exit 1; }
  return 0
}

curl -sS -X POST "$AUTH/device_authorization" --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "scope=$SCOPE" >"$WORK/dev.json"
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
Q="countryCode=$CC"
log "# tidal-artist-tracks-probe $(date -u +%FT%TZ)"; log ""

# v1 candidates for the apps' "Credits" view
req "v1 artist credits" "$API/artists/$ARTIST/credits?$Q&limit=50"
req "v1 artist contributions" "$API/artists/$ARTIST/contributions?$Q&limit=50"
req "v1 artist tracks" "$API/artists/$ARTIST/tracks?$Q&limit=50"
req "v1 contributor page" "$API/pages/contributor?artistId=$ARTIST&$Q&deviceType=BROWSER&locale=en_US"
req "v1 artist page (module titles)" "$API/pages/artist?artistId=$ARTIST&$Q&deviceType=BROWSER&locale=en_US"
log "### artist page: module titles and types"
jq -c '[.rows[]?.modules[]? | {type, title, more: (.showMore.apiPath // null)}]' <"$WORK/b" | tee -a "$OUT"; log ""
# v2 (JSON:API)
H=(-H "Accept: application/vnd.api+json")
req "v2 artist tracks relationship" "$V2/artists/$ARTIST/relationships/tracks?$Q" "${H[@]}"
req "v2 artist tracks, collapsed by fingerprint" "$V2/artists/$ARTIST/relationships/tracks?$Q&collapseBy=FINGERPRINT" "${H[@]}"
req "v2 artist tracks, collapseBy=NONE, include=tracks" "$V2/artists/$ARTIST/relationships/tracks?$Q&collapseBy=NONE&include=tracks" "${H[@]}"
req "v2 artist with relationships" "$V2/artists/$ARTIST?$Q" "${H[@]}"
echo; echo "Done. Paste $OUT back."
