[English](../0013-key-hints.md) | 正體中文

<a id="0013--key-sequence-hints"></a>
# 0013 — 按鍵序列提示

- **狀態**：implemented（2026-10-09；「測試計畫」下的手動檢查在使用者的機器上執行）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#17](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/17)
- **相依於**：0004（implemented：畫面）、0008（implemented：按鍵對應、按鍵序列、按鍵說明與它的文字、`app.toml`）
- **使用者文件**：[`docs/tui.md`](../../zh-TW/tui.md) 新增「Key hints」；[`docs/config.md`](../../zh-TW/config.md) 新增 `key_hints` 與 `key_hints_delay_ms`（AC7）

<a id="context"></a>
## 背景

六個預設的綁定是以 `g` 開頭的兩鍵序列（`g g`、`g a`、`g l`、`g y`、`g s`、`g m`），而 `keymap.toml` 可以加入任何長度的序列（0008，「Key sequences and where keys act」）。按下第一個鍵之後，畫面上沒有任何東西說明有序列正在等待，或後面可以接什麼：使用者得記住第二個鍵，或開啟按鍵說明（`?`），而那也會清除等待中的按鍵。which-key（Neovim）與 Helix 會顯示一個小的彈出視窗，列出可以接在前綴之後的按鍵。本規格加入這樣一個提示。

tidalt 沒有多鍵綁定，所以沒有東西需要沿用。

<a id="behaviour"></a>
## 行為

<a id="when-the-hint-shows"></a>
### 提示何時顯示

當一個按鍵序列已經等待了**延遲**時間（`key_hints_delay_ms`，預設 1000 ms）而沒有再按下一個鍵（目前為止按下的鍵開頭是一個更長的綁定，但它們本身不是綁定，0008 的規則），且它所開頭的綁定中至少有一個**在使用者所在之處有作用**時，就會繪製一個**提示**方塊。比延遲更快輸入完的序列絕不會顯示提示。每個讓序列繼續等待的後續按鍵（`s l a` 中的 `s` 再按 `l`）都會重新開始延遲。只要沒有任何東西在等待，提示就會消失：

- 下一個鍵完成了一個綁定（該綁定執行，與現在相同）
- 下一個鍵不開頭任何東西（收集從那個鍵單獨重新開始，與現在相同：`g x` 做 `x` 做的事，`g esc` 做 `esc` 做的事，所以 `esc` 會取消）
- 其他東西清除了等待中的按鍵（文字輸入框或彈出視窗的固定按鍵、開啟按鍵說明）

按鍵說明開啟時不繪製（說明是強制回應式的，且已列出每個按鍵）、整份清單載入期間（那時只有 `esc`/`q` 有作用）、頁面區域沒有繪製時（少於 8 列的終端機，0004），或 `key_hints = false` 時，也都不繪製。

提示不會改變按鍵的作用：不論有沒有繪製提示，分派讀取的都是相同的等待中按鍵，而延遲只決定何時繪製方塊（0008：序列仍然沒有逾時）。延遲由 TUI 衡量（它至少每 100 ms 重繪一次），因此方塊會在延遲之後的一個畫面內出現。

<a id="what-it-lists"></a>
### 它列出什麼

每個不同的下一個鍵有一個**項目**，從即時的按鍵對應（包括 `keymap.toml`）建立，並以與按鍵說明相同的規則篩選到目前的情境（0008「The keys help」）：

- 這些項目是目前檢視的按鍵說明的列（它的視窗或彈出視窗區段、`Lists`、`Pages`、`Playback`、`Actions`、`App`，不含篩選）中，按鍵序列以等待中的按鍵開頭且比它們更長的那些，依按鍵說明的順序。說明在此處沒有列出的綁定（它在這個檢視中不做任何事）沒有項目
- 項目顯示檔案語法中的**下一個鍵**（`l`、`C-x`、`enter`）與按鍵說明中該綁定的**說明文字**（`the library`、`move to the top`）；在前綴之下有數個序列的綁定，每個下一個鍵給出一個項目
- 當下一個鍵本身開頭更長的序列（**巢狀前綴**，例如 `s` 之後的 `s l a` 與 `s l b`），項目顯示下一個鍵與 `+N`，N 是它通往多少個列出的綁定（`l  +2`）。再下一層在按下那個鍵之後才列出
- 綁定現在不會做任何事的項目（按鍵說明的淡色規則：空佇列上的播放按鍵、沒有列時的列按鍵、中斷連線時的播放器指令）以淡色繪製。巢狀前綴的項目只有在它通往的一切都是淡色時才是淡色

預設按鍵對應在 `g` 之後，位於選取了一列的音樂庫視窗上，80 × 24（欄寬上限為 32，因此 76 個內部欄中容得下兩欄，最長的文字會被截斷）：

```
│┌g …─────────────────────────────────────────────────────────────────────────┐│
││a  actions on the selected row     y  favorite tracks                       ││
││g  move to the top                 s  the search page (on one: its…         ││
││l  the library                     m  your mixes                            ││
│└────────────────────────────────────────────────────────────────────────────┘│
└──────────────────────────────────────────────────────────────────────────────┘
```

<a id="where-and-how-it-is-drawn"></a>
### 在哪裡以及如何繪製

- **停靠**在頁面區域的底部，蓋在頁面的最後幾列之上，寬度為整個頁面區域，像彈出視窗一樣（頁面不會被調整大小或重新排版，它的清單高度，也就是 `Resize` 的列數，維持不變）
- 一個**有邊框的方塊**，標題為等待中的按鍵加上 `…`（`g …`、`s l …`）
- **欄**：項目逐欄填入（由上到下，再到下一欄），依上述順序。每個項目是 `key  text`（兩個空格）。每一欄的寬度等於最寬的項目，最多 32 格（且最多為方塊的內部寬度），欄與欄之間三個空格；放得下多少欄就放多少欄，列數則盡量少
- **高度**：列數加上兩個邊框列，最多為頁面區域的高度減一列，因此頁面的標題列保持可見。當項目需要的列數超過這個限制時，最後一格顯示 `… +N more`（N 個項目未顯示）
- **窄**：放不進它那一欄的文字會以 `…` 截斷。頁面區域少於 3 列或少於 12 欄時不繪製提示

<a id="config"></a>
### 設定

`app.toml` 新增下列項目，只由 TUI 讀取（如同 `volume_step`：常駐程式接受並忽略它們，而每個連上的 TUI 使用自己的，0008 AC16 對按鍵對應的規則）：

| 鍵 | 接受的值 | 預設值 | 可被覆寫於 |
|---|---|---|---|
| `key_hints` | `true`、`false` | `true` | `TIDAL_PLAYER_KEY_HINTS`（`on`、`off`） |
| `key_hints_delay_ms` | 整數 0–10 000（`0`：立即） | `1000` | `TIDAL_PLAYER_KEY_HINTS_DELAY_MS` |

延遲的單位是毫秒，而不是 0008 的 `_secs`，因為有用的值都小於一秒。

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — 有序列等待時，模型的提示（`help::hints(&State) -> Option<Hints>`）給出等待中按鍵的標籤與每個下一個鍵一個項目：下一個鍵的標籤、按鍵說明的文字、與按鍵說明相同的淡色，依按鍵說明的順序，且只有按鍵說明在這個檢視中列出的綁定。預設按鍵對應在 `g` 之後：在音樂庫視窗上（`a`、`g`、`l`、`y`、`s`、`m`），在佇列頁面上（`a` 讀作「actions on the entry」），在動作彈出視窗之上（只有 `g`）。自訂按鍵對應的綁定會出現，未綁定的則消失
- **AC2** — 開頭更長序列的下一個鍵給出一個帶有 `+N` 且沒有文字的項目，只有在它之下的一切都是淡色時才是淡色；按下它之後，提示列出下一層
- **AC3** — 在下列情況沒有提示：沒有任何東西在等待；按鍵說明開啟；顯示整份清單載入；`key_hints` 關閉；等待中的按鍵只開頭在這個檢視中沒有作用的綁定
- **AC4** — 提示從不改變按鍵的作用：對一張按鍵序列的表格（完成的 `g l`、不符的 `g x`、取消的 `g esc`、巢狀的 `s l a`、彈出視窗之上的序列），`key_hints` 開啟與關閉時的效果與（除提示之外的）結果狀態相同，且序列完成、不符或被取消之後，提示就消失
- **AC5** — 延遲：TUI 的提示計時器（`HintTimer`，每次更新之後餵給它等待中的按鍵與時間）只有在相同的等待中按鍵已等待了延遲時間後，才表示要繪製提示：之前不繪製（1000 的 999 ms），達到及之後繪製，延遲為 0 時立即繪製，沒有等待中的按鍵時絕不繪製；讓序列繼續等待的後續按鍵會重新開始它；完成、取消或不符會重設它
- **AC6** — 繪製：提示方塊停靠在頁面區域的底部，標題為 `g …`，項目依上述方式分欄排列；淡色項目為淡色；它以 `…` 與 `… +N more` 截斷；在 3 × 12 以下或沒有頁面區域時沒有提示；在從 1 × 1 到 200 × 60 的任何終端機大小下繪製它都不會 panic。快照：80 × 24（音樂庫、佇列、動作彈出視窗）、40 × 12、120 × 30，以及帶有巢狀前綴的
- **AC7** — `app.toml` 中的 `key_hints` 與 `key_hints_delay_ms`、`TIDAL_PLAYER_KEY_HINTS` 與 `TIDAL_PLAYER_KEY_HINTS_DELAY_MS` 設定 TUI 的設定，採用 0008 的優先順序與錯誤（非布林值的 `key_hints`、超出 0–10 000 的延遲：一個指出該鍵的錯誤；以 2 結束）；`examples/app.toml` 列出兩者與它們的預設值；`docs/tui.md`、`docs/config.md` 與它們的 zh-TW 副本描述提示與設定

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

- **自訂按鍵對應單獨綁定一個前綴**（`g` 綁到一個指令）：0008 會拒絕它，除非較長的序列先被解除綁定，因此已綁定的鍵絕不會顯示提示
- **沒有任何項目在此有作用**（例如自訂的 `x y` 綁到 `RemoveFromQueue`，在音樂庫頁面上按了 `x`）：不繪製提示，等待中的按鍵與現在的行為相同
- **項目非常多**（自訂按鍵對應在一個前綴之下有 30 個綁定）：最後一格顯示 `… +N more`；按鍵說明（`?`）會全部列出
- **終端機閒置時的延遲**：主迴圈在沒有輸入時每 100 ms 滴答一次，因此方塊不需要按鍵就會出現
- **等待時調整大小**：下一個畫面依新的大小排版提示
- **連上的 TUI**：提示依該客戶端的按鍵對應與 `key_hints` 計算；沒有任何東西經過 socket
- **等待時連線中斷**：淡色規則跟隨連線狀態，如同按鍵說明

<a id="test-plan"></a>
## 測試計畫

每個測試都以它的條件命名。紅燈是針對存根型別與函式本體為存根的函式的斷言失敗（`hints` 回傳 `None`、繪製什麼都不畫、設定忽略該鍵），沒有 `todo!()`，也沒有編譯錯誤，如 0001–0011。

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/core/src/ui/help/tests.rs` :: `ac1_hints_entries` (table: view × keymap → title, keys, texts, dim) | 項目、順序、文字、情境、自訂按鍵對應 | 存根 `hints` 回傳 `None` |
| AC2 | `crates/core/src/ui/help/tests.rs` :: `ac2_hints_nested_prefix` | `+N` 項目、它的淡色、按下該鍵之後的下一層 | 存根 `hints` 回傳 `None` |
| AC3 | `crates/core/src/ui/help/tests.rs` :: `ac3_no_hints` (table) | 每個列出的情況都是 `None` | 存根 `hints` 不論狀態為何，都回傳 `Some`，內含前綴之下的每個綁定 |
| AC4 | `crates/core/src/ui/help/tests.rs` :: `ac4_hints_do_not_change_keys` (table) | 開啟／關閉時效果與狀態相同；之後提示消失 | 存根 `hints` 忽略 `pending`，序列結束後仍持續回傳項目 |
| AC5 | `crates/app/src/tui/render.rs` :: `ac5_hint_timer` (table: key presses and instants → draw or not; fake instants, no sleeping) | 延遲、重新開始、重設、延遲 0 | 存根計時器永遠說要繪製 |
| AC6 | `crates/app/src/tui/render.rs` :: `ac6_key_hints` (snapshot per row), `ac6_key_hints_layout` (table: entries × area → columns, rows, `… +N more`), `ac17_no_panic_any_size` (new rows with a pending `g`) | 位置、欄、標題、淡色、截斷、各種大小 | 存根的繪製不畫提示 |
| AC7 | `crates/app/src/config.rs` :: `ac10_app_toml`, `ac11_precedence` (new rows), `crates/app/tests/examples.rs` :: `ac18_example_app_toml_is_the_defaults` (unchanged, fails until the example has the key), `crates/app/tests/docs.rs` :: `ac7_key_hints_documented` + zh-TW copies reviewed at acceptance | 每個來源的值、錯誤訊息、範例、文件提到兩個鍵 | 存根設定忽略兩個鍵 |

驗收時手動檢查：在真實的終端機中，快速輸入 `g l` 會開啟音樂庫而沒有方塊；按住 `g` 一秒鐘會顯示方塊，接著按 `l` 就開啟音樂庫；`key_hints = false` 會隱藏它。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core::ui`：`help::hints(&State) -> Option<Hints>`（只有內容：沒有時間）（`Hints { prefix: String, entries: Vec<HintEntry> }`、`HintEntry { key: String, text: String, dim: bool, more: usize }`），從 `help::help` 建立，使情境與文字規則留在同一處；`State` 新增 `key_hints: bool`（預設 `true`）。沒有 I/O，沒有新的相依套件
- `tidal-player`：設定（兩個鍵與變數）、`ui.rs` 中的 `HintTimer`（等待中的按鍵 + 注入的 `Instant` → 繪製與否；主迴圈在每批更新之後餵給它 `Instant::now()`，並把它的答案傳給 `render`）、純的排版函式（項目與一個區域 → 方塊與它的格子），以及 `ui/pages.rs` 中的繪製
- `xtask layering`：沒有變動

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-10-09，取自程式碼）：等待中的按鍵存放在 `State::pending`（`crates/core/src/ui/dispatch.rs`：`resolve`、`set_pending`）；固定按鍵與開啟說明會清除它們；按鍵說明的區段、文字與淡色規則是 `help::help`、`text`、`dim`（`crates/core/src/ui/help.rs`）；預設的多鍵綁定是六個 `g` 的綁定（`keymap::defaults`）；0008 拒絕較長序列的已綁定前綴（`check_prefixes`）；終端機少於 8 列時沒有頁面區域（`crates/app/src/tui/render.rs` 的 `areas`）。

假設：沒有任何關於外部系統的假設（不涉及 API、音訊或終端機協定）。

<a id="decisions-answered-by-the-user-2026-10-09"></a>
## 決策（使用者已回覆，2026-10-09）

1. **延遲**：*已回答：預設 1 秒*，可設定（`key_hints_delay_ms`，`0` 表示立即），由 TUI 衡量，而不是核心模型（它沒有時鐘）
2. **關閉開關**：*已回答：要*，`app.toml` 中的 `key_hints`（加上環境變數），預設開啟
3. **位置**：*已回答：照提案*：停靠在頁面區域的底部
4. **情境**：*已回答：只有有意義的*：只有在這個檢視中有作用的綁定（按鍵說明的規則）

<a id="out-of-scope"></a>
## 不在範圍內

- 針對單一按鍵的提示（每個鍵之後都有提示），或針對文字輸入框固定按鍵的提示
- 在其他地方顯示等待中的按鍵（播放視窗的標頭）
- 提示上的滑鼠支援；捲動放不下的提示（按鍵說明做這件事）
- 篩選與排序（0012，issue #18）

<a id="implementation-notes-choices-made-where-the-spec-was-silent-2026-10-09"></a>
## 實作備註（規格沒有說明的地方所做的選擇，2026-10-09）

- **3 列的頁面區域**：方塊最多可占頁面區域的高度減一列，因此在恰好 3 列時，它只會剩下兩條邊框而沒有項目；那裡同樣不繪製提示（會繪製頁面區域的最小終端機是 8 列，此時頁面區域有 2 列，所以提示需要至少 10 列的終端機）
- **繪製**：標題為粗體，如按鍵說明的；`… +N more` 那一格是一般樣式（不是淡色）。提示在開啟中的彈出視窗之後繪製（蓋在它上面，如 80 × 24 動作彈出視窗的快照所示），並在按鍵說明之前繪製（按鍵說明反正會抑制它）。一個項目是 `key  text`，巢狀前綴是 `key  +N`；兩者都以 `…` 截斷以符合該欄
- **接線**：`render(state, frame, hint)` 接收計時器的答案；主迴圈以 `key_hints_delay_ms` 建立 `HintTimer`，並在每批更新之後餵給它 `State::pending` 與 `Instant::now()`（每批一個終端機事件，因此被取消又重新開始的 `g` 會被視為一次重設）。`PlayerSettings` 帶著 `key_hints` 與 `key_hints_delay`（與 `volume_step` 相同的方式解析；常駐程式解析後忽略它們）；`tui_state` 為獨立模式與連上的 TUI 都把 `key_hints` 複製到 `State`（`crates/app/src/main.rs` 中的 `ac7_tui_state_key_hints` 檢查它）
- **巢狀前綴計數**：`+N` 計算下一個鍵之下不同的已列出綁定（在它之下有兩個序列的綁定只算一次）；巢狀項目取代它所通往的、按鍵說明順序中第一個已列出的綁定的位置
- **紅燈存根**：一個存根服務全部四個核心測試：`hints` 不論狀態為何，都回傳前綴之下的每個綁定（AC3 的「預期的紅燈」），因此 AC1 與 AC2 是因錯誤的 `Some`（沒有文字、沒有 `+N`）而失敗，而不是因 `None`
- **文件**：「Key hints」是 `docs/tui.md` 中「Keys」之下的一個 `###` 區段；`docs/playback.md` 的設定表（及其 zh-TW 副本）也列出兩個鍵。`ac7_key_hints_documented` 檢查英文與 zh-TW 的 `config.md` 表格，以及兩份 `tui.md` 副本
