#!/usr/bin/env bash
# 規格 0024(瀏覽頁,issue #69)的一次性探測:Tidal 的 home、explore、類型與
# 心情頁回傳哪些 module、哪些能當清單顯示、怎麼分頁。規格寫完後刪除本腳本。
#
# 用法:scripts/tidal-browse-probe.sh [輸出目錄]   (預設為目前目錄)
#   browse-probe-output.txt  每頁的 module 表,每種 module 第一次出現時的樣本
#   browse-probe-raw/        每頁完整回應(已遮蔽),寫規格與 fixture 時查詢用
#
# 登入:裝置流程的連結(含代碼)以 xdg-open 開啟,代碼以 wl-copy 複製;
# 在瀏覽器按同意即可。
#
# 只有 GET,帳號不會有任何變動。請求間隔 1 秒,最多走 MAX_PAGES 頁,
# 遇到 429 或含 "abuse" 的 403 立即停止。
#
# 首頁可能含最近播放與推薦內容,原始回應只留在本機;token、使用者 ID 與姓名
# 一律遮蔽。需要:bash、curl、jq;選用:xdg-open、wl-copy(或 xclip、xsel)。

set -euo pipefail

CLIENT_ID="fX2JxdmntZWK0ixT"
CLIENT_SECRET="1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg="
AUTH="https://auth.tidal.com/v1/oauth2"
API="https://api.tidal.com/v1"
API2="https://api.tidal.com/v2"
SCOPE="r_usr w_usr w_sub"
DIR=${1:-.}
OUT="$DIR/browse-probe-output.txt"
RAW="$DIR/browse-probe-raw"
MAX_PAGES=26
LINKS_PER_MODULE=3

for bin in curl jq; do
  command -v "$bin" >/dev/null || { echo "missing: $bin" >&2; exit 1; }
done
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
mkdir -p "$RAW"
: >"$OUT"

redact_json='walk(if type == "object" then with_entries(
  if (.key | test("token|Token|deviceCode|userCode|sessionId|userId|user_id|email|username|firstName|lastName|^sub$"; ""))
  then .value |= "<redacted len=\(tostring | length)>" else . end) else . end)'
# 保留結構:每個陣列只留前 2 項並註明總長
shorten='walk(if type == "array" and length > 2 then (.[0:2] + ["… \(length) items in all"]) else . end)'

log() { printf '%s\n' "$*" | tee -a "$OUT"; }

# fetch BASE PATH [curl 參數...]:結果放在 STATUS 與 BODY
fetch() {
  local base=$1 path=$2; shift 2
  local tmp; tmp=$(mktemp)
  STATUS=$(curl -sS -G -o "$tmp" -w '%{http_code}' -H "Authorization: Bearer $ACCESS" \
    --data-urlencode "countryCode=$CC" "$@" "$base/$path" || echo curl-error)
  BODY=$(cat "$tmp"); rm -f "$tmp"
  log "### GET ${base#https://api.tidal.com}/$path $*"
  log "status: $STATUS"
  if ! jq -e . >/dev/null 2>&1 <<<"$BODY"; then
    log "non-JSON body (${#BODY} bytes): ${BODY:0:200}"
  elif [ "$STATUS" != 200 ]; then
    jq -c "$redact_json" <<<"$BODY" | cut -c1-400 | tee -a "$OUT"
  fi
  if [ "$STATUS" = 429 ] || { [ "$STATUS" = 403 ] && grep -qi abuse <<<"$BODY"; }; then
    log "STOPPING: Tidal answered $STATUS (rate limit / abuse detection)"; exit 1
  fi
  sleep 1
}

# save NAME:把目前的回應(已遮蔽、不截斷)存進 RAW
save() {
  jq "$redact_json" <<<"$BODY" >"$RAW/$(tr -c 'A-Za-z0-9_-' '_' <<<"$1").json" 2>/dev/null || true
}

declare -A SEEN_PAGE=() SEEN_MODULE=()

# modules:一行一個 module 的摘要,新類型附上截斷後的樣本與分頁測試
modules() {
  jq -c '.rows[]?.modules[]? | {type, title, preTitle, scroll, listFormat,
      total: .pagedList.totalNumberOfItems, paged: (.pagedList.items | length?),
      limit: .pagedList.limit, data: .pagedList.dataApiPath,
      items: (.items | length?), highlights: (.highlights | length?),
      showMore: .showMore.apiPath,
      item_types: ([.pagedList.items[]?, .items[]? | .type? // empty] | unique),
      item_keys: ((.pagedList.items[0]? // .items[0]? // .highlights[0]?) | keys?)}' \
    <<<"$BODY" 2>/dev/null | sed 's/^/  module: /' | tee -a "$OUT" || true

  local page=$BODY type sample data
  while IFS= read -r type; do
    [ -n "$type" ] || continue
    [ -z "${SEEN_MODULE[$type]:-}" ] || continue
    SEEN_MODULE[$type]=1
    sample=$(jq -c --arg t "$type" '[.rows[]?.modules[]? | select(.type == $t)][0]' <<<"$page")
    log "  first $type module:"
    jq "$redact_json | $shorten" <<<"$sample" | tee -a "$OUT"
    data=$(jq -r '.pagedList.dataApiPath // empty' <<<"$sample")
    if [ -n "$data" ]; then
      fetch "$API" "$data" "${PAGE[@]}" --data-urlencode "limit=5" --data-urlencode "offset=5"
      jq -c '{limit, offset, total: .totalNumberOfItems, n: (.items | length?),
              first: [.items[0:3][]? | (.item // .) | (.id // .uuid // .title // .apiPath)]}' \
        <<<"$BODY" 2>/dev/null | sed "s/^/  $type page 2: /" | tee -a "$OUT" || true
    fi
  done < <(jq -r '[.rows[]?.modules[]?.type] | unique[]' <<<"$page" 2>/dev/null)
  BODY=$page
}

# links:每個 module 最多 LINKS_PER_MODULE 個頁面連結,加上它的 showMore
links() {
  jq -r --argjson n "$LINKS_PER_MODULE" '.rows[]?.modules[]?
      | ([.pagedList.items[]?, .items[]? | .apiPath? // empty][0:$n][]), (.showMore.apiPath? // empty)' \
    <<<"$BODY" 2>/dev/null | grep '^pages/' || true
}

QUEUE=()
walk() {
  local path=$1
  [ -z "${SEEN_PAGE[$path]:-}" ] || return 0
  [ "${#SEEN_PAGE[@]}" -lt "$MAX_PAGES" ] || { log "page budget reached, skipped $path"; return 0; }
  SEEN_PAGE[$path]=1
  fetch "$API" "$path" "${PAGE[@]}"
  log "  page title: $(jq -c '.title' <<<"$BODY" 2>/dev/null || true)"
  save "$path"
  modules
  while IFS= read -r link; do QUEUE+=("$link"); done < <(links)
  log ""
}

copy_code() {
  if command -v wl-copy >/dev/null; then printf '%s' "$1" | wl-copy
  elif command -v xclip >/dev/null; then printf '%s' "$1" | xclip -selection clipboard
  elif command -v xsel >/dev/null; then printf '%s' "$1" | xsel --clipboard --input
  else return 1; fi
}

log "# tidal-browse-probe $(date -u +%FT%TZ)"
log ""

# 裝置流程登入;含代碼的連結會自動開啟
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

# 1. v1 的種子頁,再沿著它們的連結往下走
for seed in pages/home pages/explore pages/for_you pages/genre_page pages/genre_page_local \
            pages/moods_page pages/hires pages/explore_new_music pages/explore_top_music; do
  walk "$seed"
done
i=0
while [ "$i" -lt "${#QUEUE[@]}" ]; do walk "${QUEUE[$i]}"; i=$((i + 1)); done

# 2. 不存在的頁面:錯誤的形狀
fetch "$API" pages/this_page_does_not_exist "${PAGE[@]}"
log ""

# 3. v2 的首頁動態:v1 的 pages/home 可能已不再使用
V2=(--data-urlencode "deviceType=BROWSER" --data-urlencode "locale=en_US" --data-urlencode "platform=WEB")
feed() {
  jq -c '{uuid: (.uuid != null), page, header: (.header | keys?), top_keys: keys}' <<<"$BODY" 2>/dev/null \
    | sed 's/^/  feed: /' | tee -a "$OUT" || true
  jq -c '.items[]? | {type, moduleId, title, subtitle, viewAll, n: (.items | length?),
          item_types: ([.items[]?.type?] | unique), item_keys: (.items[0]? | keys?),
          data_keys: (.items[0]?.data? | keys?)}' <<<"$BODY" 2>/dev/null \
    | sed 's/^/  feed module: /' | tee -a "$OUT" || true
}
fetch "$API2" home/feed/static "${V2[@]}"
if [ "$STATUS" != 200 ]; then
  fetch "$API2" home/feed/static "${V2[@]}" -H "x-tidal-client-version: 2026.10.1"
fi
save "v2_home_feed_static"
feed
FEED=$BODY
log "  first feed module of each type:"
jq "$redact_json | $shorten" <<<"$(jq -c '[.items[]?] | group_by(.type) | map(.[0])' <<<"$FEED" 2>/dev/null)" | tee -a "$OUT" || true
log ""
CURSOR=$(jq -r '.page.cursor // empty' <<<"$FEED" 2>/dev/null || true)
if [ -n "$CURSOR" ]; then
  fetch "$API2" home/feed/static "${V2[@]}" --data-urlencode "cursor=$CURSOR"
  save "v2_home_feed_cursor"
  feed
  log ""
fi
n=0
while IFS= read -r path; do
  [ "$n" -lt 3 ] || break
  n=$((n + 1))
  fetch "$API2" "${path#/}" "${V2[@]}" --data-urlencode "limit=10"
  save "v2_view_all_$n"
  jq -c '{top_keys: keys, cursor: .page.cursor?, n: (.items | length?), item_types: ([.items[]?.type?] | unique),
          item_keys: (.items[0]? | keys?)}' <<<"$BODY" 2>/dev/null | sed 's/^/  view-all: /' | tee -a "$OUT" || true
  log ""
done < <(jq -r '[.items[]?.viewAll // empty] | unique[]' <<<"$FEED" 2>/dev/null || true)

log "# done: ${#SEEN_PAGE[@]} pages walked, module types: ${!SEEN_MODULE[*]}"
echo
echo "Wrote $OUT and $RAW/."
