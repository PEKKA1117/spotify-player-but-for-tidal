#!/usr/bin/env bash
# One-off probe for spec 0002 ("Facts vs. assumptions"). Run once with a real
# Tidal account, then paste auth-probe-output.txt back. Delete this script
# once the spec's fixtures are recorded.
#
# What it does, against the live API, using tidalt's public client credentials:
#   1. device_authorization, and an immediate poll (the "pending" body)
#   2. polls until you approve the code in the browser
#   3. GET /v1/sessions
#   4. refresh with the real refresh token (does Tidal rotate it?)
#   5. refresh with a bogus token, and with the pre-rotation token
#   6. revokes the session it created, then refreshes with it ("session lost")
# It ends by revoking, so it leaves no extra login behind on your account.
#
# Every token, code and ID is replaced by "<redacted len=N>" before anything is
# printed or written. Requires: bash, curl, jq, sha256sum.

set -euo pipefail

CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
SCOPE="r_usr w_usr w_sub"
OUT="auth-probe-output.txt"

for bin in curl jq sha256sum; do
  command -v "$bin" >/dev/null || { echo "missing: $bin" >&2; exit 1; }
done
: >"$OUT"

# Replace the value of every sensitive key with its length only.
redact='walk(if type == "object" then with_entries(
  if (.key | test("token|Token|deviceCode|userCode|device_code|user_code|sessionId|userId|^id$|email|username|firstName|lastName|^sub$|partnerId"; ""))
  then .value |= "<redacted len=\(tostring | length)>" else . end) else . end)'

log() { printf '%s\n' "$*" | tee -a "$OUT"; }
fp() { printf '%s' "$1" | sha256sum | cut -c1-8; }   # local fingerprint, never the token

# req NAME CURL_ARGS... → sets STATUS and BODY, logs the redacted result.
req() {
  local name=$1; shift
  local tmp; tmp=$(mktemp)
  STATUS=$(curl -sS -o "$tmp" -w '%{http_code}' "$@" || echo "curl-error")
  BODY=$(cat "$tmp"); rm -f "$tmp"
  log "### $name"
  log "status: $STATUS"
  if jq -e . >/dev/null 2>&1 <<<"$BODY"; then
    jq "$redact" <<<"$BODY" | tee -a "$OUT"
  else
    log "non-JSON body (${#BODY} bytes): ${BODY:0:200}"
  fi
  log ""
}

token_req() { req "$1" -X POST "$AUTH/token" -u "$CLIENT_ID:$CLIENT_SECRET" "${@:2}"; }

log "# tidal-auth-probe $(date -u +%FT%TZ)"
log ""

# 1. Device authorization
req "device_authorization" -X POST "$AUTH/device_authorization" \
  --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "scope=$SCOPE"
DEVICE_CODE=$(jq -r .deviceCode <<<"$BODY")
USER_CODE=$(jq -r .userCode <<<"$BODY")
VERIFY=$(jq -r .verificationUri <<<"$BODY")
INTERVAL=$(jq -r '.interval // 5' <<<"$BODY")
EXPIRES=$(jq -r '.expiresIn // 300' <<<"$BODY")

token_req "token: immediate poll (expect pending)" \
  --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "device_code=$DEVICE_CODE" \
  --data-urlencode "grant_type=urn:ietf:params:oauth:grant-type:device_code" \
  --data-urlencode "scope=$SCOPE"

echo
echo "Open this link (it is NOT written to $OUT):"
echo "  https://$VERIFY/$USER_CODE"
echo "If that page does not already show the code, use https://$VERIFY and type: $USER_CODE"
echo "Waiting for approval (code expires in ${EXPIRES}s)..."
echo

# 2. Poll until granted
deadline=$(( $(date +%s) + EXPIRES ))
seen=""
while :; do
  sleep "$INTERVAL"
  tmp=$(mktemp)
  STATUS=$(curl -sS -o "$tmp" -w '%{http_code}' -X POST "$AUTH/token" -u "$CLIENT_ID:$CLIENT_SECRET" \
    --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "device_code=$DEVICE_CODE" \
    --data-urlencode "grant_type=urn:ietf:params:oauth:grant-type:device_code" \
    --data-urlencode "scope=$SCOPE")
  BODY=$(cat "$tmp"); rm -f "$tmp"
  [ "$STATUS" = 200 ] && break
  err=$(jq -r '.error // empty' <<<"$BODY" 2>/dev/null || true)
  if [[ " $seen " != *" $STATUS:$err "* ]]; then
    seen="$seen $STATUS:$err"
    log "### token: poll response $STATUS ($err)"
    jq "$redact" <<<"$BODY" 2>/dev/null | tee -a "$OUT" || log "${BODY:0:200}"
    log ""
  fi
  [ "$err" = slow_down ] && INTERVAL=$((INTERVAL + 5))
  [ "$(date +%s)" -ge "$deadline" ] && { log "gave up: code expired"; exit 1; }
done
log "### token: granted"
log "status: 200"
jq "$redact" <<<"$BODY" | tee -a "$OUT"
log ""
ACCESS=$(jq -r .access_token <<<"$BODY")
REFRESH1=$(jq -r .refresh_token <<<"$BODY")

# 3. Session info
req "GET /v1/sessions" -H "Authorization: Bearer $ACCESS" "$API/sessions"

# 4. Refresh with the real token
token_req "refresh: valid token" \
  --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "grant_type=refresh_token" \
  --data-urlencode "refresh_token=$REFRESH1" --data-urlencode "scope=$SCOPE"
REFRESH2=$(jq -r '.refresh_token // empty' <<<"$BODY")
if [ -z "$REFRESH2" ]; then
  log "rotation: response has NO refresh_token (old one stays)"; REFRESH2=$REFRESH1
elif [ "$(fp "$REFRESH2")" = "$(fp "$REFRESH1")" ]; then
  log "rotation: response repeats the SAME refresh_token"
else
  log "rotation: response has a NEW refresh_token"
fi
log ""

# 5. Failure shapes
token_req "refresh: bogus token" \
  --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "grant_type=refresh_token" \
  --data-urlencode "refresh_token=bogus-refresh-token" --data-urlencode "scope=$SCOPE"
if [ "$REFRESH2" != "$REFRESH1" ]; then
  token_req "refresh: pre-rotation token" \
    --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "grant_type=refresh_token" \
    --data-urlencode "refresh_token=$REFRESH1" --data-urlencode "scope=$SCOPE"
fi
req "GET /v1/sessions with bogus bearer" -H "Authorization: Bearer bogus" "$API/sessions"

# 6. Revoke, then use the revoked token
req "revoke" -X POST "$AUTH/revoke" -u "$CLIENT_ID:$CLIENT_SECRET" --data-urlencode "token=$REFRESH2"
token_req "refresh: after revoke" \
  --data-urlencode "client_id=$CLIENT_ID" --data-urlencode "grant_type=refresh_token" \
  --data-urlencode "refresh_token=$REFRESH2" --data-urlencode "scope=$SCOPE"

echo
read -rp "Did the link already show the code on the Tidal page? [y/n] " prefill
log "### link pre-fill: $prefill"
echo
echo "Done. Check $OUT for anything private, then paste it back."
