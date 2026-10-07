#!/usr/bin/env bash
# One-off READ-ONLY probe for spec 0006 decision 6: does the contributor
# ("Credits for <artist>") page work with our token, and how does it page? Paste
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
# A short, valid summary instead of the whole body: page modules and their
# paged lists, or a list's paging fields with the first item's keys and IDs.
summary='def list: {limit, offset, totalNumberOfItems, n: (.items|length),
    ids: [.items[]? | (.item.id // .id)][:5], entryKeys: ((.items[0] // {}) | keys),
    modes: ([.items[]? | (.item.audioModes // .audioModes) | tostring] | group_by(.) | map({(.[0]): length}) | add),
    cats: ([.items[]?.roles[]?.category] | group_by(.) | map({(.[0]): length}) | add)};
  if has("rows") then {title, modules: [.rows[].modules[] | {type, title, dataApiPath: .pagedList.dataApiPath,
    paged: (if .pagedList then (.pagedList | list) else null end)}]}
  elif has("items") then list else . end' 

req() {
  local name=$1 url=$2; shift 2
  sleep 1
  STATUS=$(curl -sS -o "$WORK/b" -w '%{http_code}' -H "Authorization: Bearer $ACCESS" "$@" "$url" || echo curl-error)
  log "### $name"; log "GET $(sed -E 's/countryCode=[A-Z]+/countryCode=<cc>/' <<<"$url")"; log "status: $STATUS"
  if jq -e . >/dev/null 2>&1 <"$WORK/b"; then jq -c "$summary | $redact" <"$WORK/b" | tee -a "$OUT"
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

# The contributor ("Credits for <artist>") page, as tidal.com's web player calls it,
# here with our device-flow token on api.tidal.com.
P="artistId=$ARTIST&$Q&deviceType=BROWSER&locale=en_US"
req "contributor page, no token" "$API/pages/contributor?$P" -H "Authorization:"
req "contributor page" "$API/pages/contributor?$P"
DATA=$(jq -r '[.rows[]?.modules[]? | .pagedList.dataApiPath // empty][0] // empty' <"$WORK/b")
log "### dataApiPath: ${DATA:-none}"; log ""
if [ -n "$DATA" ]; then
  SEP='&'; case "$DATA" in *\?*) ;; *) SEP='?';; esac
  req "credits page 2 (limit=50 offset=50)" "$API/$DATA$SEP$Q&deviceType=BROWSER&locale=en_US&limit=50&offset=50"
  req "credits, limit=1000" "$API/$DATA$SEP$Q&deviceType=BROWSER&locale=en_US&limit=1000&offset=0"
  log "### that page: items kept, totalNumberOfItems, audioModes and role categories seen"
  jq -c '{n: (.items|length), total: .totalNumberOfItems, modes: ([.items[]?.item.audioModes|tostring]|group_by(.)|map({(.[0]):length})|add), cats: ([.items[]?.roles[]?.category]|group_by(.)|map({(.[0]):length})|add)}' <"$WORK/b" | tee -a "$OUT"; log ""
  req "credits, limit=100 offset=500 (past the end?)" "$API/$DATA$SEP$Q&deviceType=BROWSER&locale=en_US&limit=100&offset=500"
  req "credits, Performer only (roleCategoryId=11?)" "$API/$DATA$SEP$Q&deviceType=BROWSER&locale=en_US&limit=10&roleCategoryId=11"
fi
req "contributor page, unknown artist" "$API/pages/contributor?artistId=1&$Q&deviceType=BROWSER&locale=en_US"
echo; echo "Done. Paste $OUT back."
