[English](../0030-clipboard-links.md) | 正體中文

<a id="0030--copy-a-link-open-a-link-from-the-clipboard"></a>
# 0030 — 複製連結、從剪貼簿開啟連結

- **狀態**：approved（2026-11-16；使用者事先核准所有規格，並要求把 PR 留給他審閱）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#57](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/57)
- **相依於**：0004（implemented：連結與曲目 ID、`Open`）、0006（implemented：動作彈出視窗、頁面）、0007（implemented：`o`／`O` 輸入提示）、0008（implemented：按鍵對應、`[[actions]]`、略過的名稱）、0011（implemented：mix 頁面）、0020（implemented：選取的動作）
- **使用者文件**：[`docs/tui.md`](../../tui.md)（新增「Links and the clipboard」一節、按鍵表）、[`docs/config.md`](../../config.md)（指令與動作表、略過的名稱）、`examples/keymap.toml`、`README.md`，以及它們的 zh-TW 版本

<a id="context"></a>
## 背景

目前要分享正在播放的東西，或開啟別人分享的東西，都得離開 TUI：沒有辦法從 TUI 拿到項目的 Tidal 連結，而在瀏覽器複製的連結只能貼進 `o` 輸入提示，它只會加入佇列（而且拒絕藝人與 mix 連結）。spotify-player 為此有 `CopyLink` 動作與 `OpenSpotifyLinkFromClipboard` 指令；兩者目前都在我們按鍵對應略過的 spotify-player 名稱清單上（0008 決策 3），所以綁定它們的 spotify-player `keymap.toml` 會失去這些綁定，並出現提示。

tidalt 兩者都沒有。

<a id="behaviour"></a>
## 行為

<a id="copy-link"></a>
### Copy link

動作彈出視窗在每個有 Tidal 連結的項目上新增 **Copy link**，排在最後一項：

| 列 | 複製的連結 |
|---|---|
| 曲目（任何曲目清單、演出名單、佇列項目、用 `a` 開啟的正在播放曲目） | `https://tidal.com/browse/track/<id>` |
| 專輯 | `https://tidal.com/browse/album/<id>` |
| 藝人 | `https://tidal.com/browse/artist/<id>` |
| 播放清單（自己的或別人的） | `https://tidal.com/browse/playlist/<uuid>` |
| mix | `https://tidal.com/browse/mix/<id>` |

*Collected Tracks*（0028）沒有連結，也沒有 *Copy link*；瀏覽連結（0024）沒有動作彈出視窗；選取的動作（`s a`，0020）不會複製。

選擇它會關閉彈出視窗，並把連結放到剪貼簿（見「剪貼簿」）。接著訊息列會顯示 `Link copied: https://tidal.com/browse/album/10`，或無法複製的原因。

`CopyLink`（spotify-player 的名稱）是一個 `[[actions]]` 動作：`target = "SelectedItem"`（預設）複製游標所在列的連結，`target = "PlayingTrack"` 複製正在播放曲目的連結，和彈出視窗的做法完全相同，在彈出視窗不會列出它的地方則什麼都不做。它沒有預設按鍵，和 spotify-player 一樣。

<a id="open-a-link-from-the-clipboard"></a>
### 從剪貼簿開啟連結

`OpenSpotifyLinkFromClipboard`（spotify-player 的名稱，所以它的 `keymap.toml` 項目可以原樣使用）讀取剪貼簿，並開啟其中的 Tidal 連結。預設按鍵是 `g p`（「paste」）：spotify-player 的 `O` 在這裡是 `PlayNextPrompt`（0007），而 `g p` 尚未使用。它和頁面指令一樣在頁面上作用，不會在彈出視窗、輸入提示或按鍵說明之上作用。

剪貼簿的文字去掉頭尾空白後，以 0004 的連結規則讀取（`tidal.com`、`www.tidal.com` 或 `listen.tidal.com` 上的 `https://` 連結，或 `tidal://`；可有 `/browse`；可有結尾斜線、`/u`、查詢字串或片段），並擴充到藝人與 mix 連結，單純的數字則是曲目 ID：

| 剪貼簿內容 | 結果 |
|---|---|
| 專輯、播放清單、藝人或 mix 連結 | 開啟它的頁面（推入頁面歷程，和 *Open* 一樣）；不會播放，佇列也不變 |
| 曲目連結或曲目 ID | 曲目加到佇列尾端，和 `o` 一樣 |
| 其他任何東西 | 訊息列顯示 `Not a Tidal link: <text>`，文字截到 60 個字元並加上 `…` |
| 什麼都沒有（或只有空白） | `The clipboard is empty` |
| 沒有可讀取的剪貼簿 | 原因（見下文） |

回應會在按鍵後稍晚才回來；若這段期間開啟了彈出視窗、輸入提示或按鍵說明，回應就會被丟棄，以免它作用在使用者沒看到的東西上。

`o`／`O` 輸入提示不變：因為它們是加入佇列，所以仍然只接受曲目、專輯與播放清單連結。

<a id="the-clipboard"></a>
### 剪貼簿

TUI 使用它繪製所在機器的剪貼簿，每次都依環境選擇：

| 環境 | 複製 | 讀取 |
|---|---|---|
| 設定了 `WAYLAND_DISPLAY` | `wl-copy` | `wl-paste --no-newline` |
| 否則設定了 `DISPLAY` | `xclip -selection clipboard`，沒有安裝 `xclip` 時用 `xsel --clipboard --input` | `xclip -selection clipboard -o`，或 `xsel --clipboard --output` |
| 兩者皆無（主控台、SSH） | OSC 52：終端機自己的剪貼簿跳脫序列，寫到終端機 | 無法讀取 |

- 工具沒有安裝：Wayland 上顯示 `Copying the link needs wl-copy: install wl-clipboard`，X11 上顯示 `Copying the link needs xclip or xsel: install one of them`（讀取時為 `Reading the clipboard needs …`，工具相同）
- 兩個變數都沒有設定時讀取：`Reading the clipboard needs wl-paste (Wayland), xclip or xsel (X11)`
- 工具以錯誤結束：複製時顯示 `Could not copy the link: <tool> exited with status <n>`；讀取時顯示 `The clipboard holds no text`（剪貼簿為空或不是文字時，`wl-paste` 與 `xclip` 就是這樣）
- 工具 2 秒後仍未結束就會被終止：`Could not copy the link: <tool> did not answer`／`Could not read the clipboard: <tool> did not answer`
- 從 TUI 這端看，OSC 52 一定「成功」：終端機是否遵循它是終端機的事（kitty、foot、WezTerm、iTerm2 與開啟 `allowWindowOps` 的 xterm 會；在 tmux 內需要 `set -g set-clipboard on`）。訊息列顯示 `Link copied (OSC 52): <link>`，讓終端機忽略它的使用者知道走的是哪條路

只有 TUI 會碰剪貼簿：複製與讀取都不會送到播放器，所以連上常駐程式的 TUI 不論常駐程式在哪，都是在自己的機器上複製。

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — 連結由同一個純函式模組組成與讀取：每種類型（曲目、專輯、藝人、播放清單、mix）產生「Copy link」表中的連結，讀回時得到相同的類型與 ID。藝人與 mix 連結以 0004 的所有變化讀取（主機、`tidal://`、`/browse`、結尾斜線、`/u`、查詢字串、片段）；mix ID 由字母與數字組成，保留原樣。0004 的 `parse_item` 仍拒絕藝人與 mix 連結
- **AC2** — 動作彈出視窗在曲目列、演出名單列、佇列項目、正在播放的曲目、專輯、藝人、播放清單（自己的與別人的）與 mix 上，把 *Copy link* 列在最後，帶著該列的連結；*Collected Tracks* 上沒有，選取的動作中也沒有
- **AC3** — 選擇 *Copy link* 會關閉彈出視窗並請 TUI 複製連結；其結果設定訊息列：`Link copied: <link>`、`Link copied (OSC 52): <link>`，或失敗的原因
- **AC4** — `CopyLink` 是有兩種 target 的 `[[actions]]` 動作，執行的正是彈出視窗會做的事（在彈出視窗沒有 *Copy link* 的地方什麼都不做）；它沒有預設按鍵，也不再是略過的名稱（綁定它的 `keymap.toml` 不會出現提示）
- **AC5** — `OpenSpotifyLinkFromClipboard` 是一個指令，預設綁定 `g p`，列在按鍵說明中（「open the Tidal link in the clipboard」）；在頁面上它請 TUI 讀取剪貼簿，在彈出視窗、輸入提示或按鍵說明之上什麼都不做，而且不再是略過的名稱
- **AC6** — 剪貼簿的文字依「從剪貼簿開啟連結」的表處理：專輯、播放清單、藝人或 mix 連結開啟其頁面（請求該頁面，不送任何東西給播放器）；曲目連結或 ID 送出把該曲目加到佇列尾端的 `Open`；其他任何東西、空內容與讀取失敗以表中的措辭設定訊息列（60 個字元與 `…`）；在彈出視窗、輸入提示或按鍵說明開啟時才到的回應會被丟棄
- **AC7** — 依「剪貼簿」的表從環境選擇剪貼簿：先 Wayland，再 X11，否則複製用 OSC 52 且無法讀取
- **AC8** — 複製時執行表中的工具，把連結交給它的標準輸入，最多等它 2 秒；沒有安裝 `xclip` 時改試 `xsel`；工具不存在、以錯誤結束與逾時時顯示上述訊息
- **AC9** — 讀取時執行表中的工具並傳回其輸出；工具不存在、以錯誤結束、逾時與沒有顯示器時顯示上述訊息；超過 64 KiB 的輸出在該處截斷（反正那不是連結）
- **AC10** — OSC 52 是 `ESC ] 52 ; c ; <連結的 base64> BEL`，在兩個畫面之間寫到終端機
- **AC11** — 使用者文件（`docs/tui.md`、`docs/config.md`）、`examples/keymap.toml`、`README.md` 及它們的 zh-TW 版本說明 *Copy link*、`CopyLink`、`OpenSpotifyLinkFromClipboard` 與 `g p`，而這兩個名稱從文件中略過的 spotify-player 名稱清單移除

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

- **播放器不在了**（中斷連線）：複製仍然可用（那是 TUI 自己的事）；剪貼簿中的曲目不會加入佇列，因為不在的播放器什麼都收不到（0005）；頁面會開啟，並和任何頁面一樣顯示載入失敗
- **頁面已不存在的連結**（已刪除的播放清單、錯誤的 ID）：頁面會開啟並說它無法載入，和在過時的列上按 *Open* 一樣
- **連結前後有其他文字**（`Listen: https://tidal.com/…`）：不是連結；整段文字必須就是一個連結，和 `o` 輸入提示相同
- **非常大的剪貼簿內容**（圖片、檔案）：讀取在 64 KiB 停止，而那段文字不是連結
- **非文字的剪貼簿**：`wl-paste` 與 `xclip` 會失敗：`The clipboard holds no text`
- **同時設定了 `WAYLAND_DISPLAY` 與 `DISPLAY`**（XWayland）：使用 Wayland 的工具，它們看到的是同一個剪貼簿
- **會 fork 以持續提供剪貼簿內容的工具**（`wl-copy`、`xclip`）：不等它的輸出（標準輸出與標準錯誤導向 `/dev/null`），只等啟動的那個行程結束
- **極小的終端機**：除了訊息列與彈出視窗的一個項目外，沒有新的繪製內容

<a id="test-plan"></a>
## 測試計畫

| AC | 測試 | 斷言 | 預期的紅燈失敗 |
|---|---|---|---|
| AC1 | `crates/core/src/item.rs` :: `ac1_link_round_trip`（表格）+ `crates/core/src/item.rs` :: `ac1_artist_and_mix_links`（表格）+ `crates/core/src/item.rs` :: `ac1_parse_item_refuses_artist_and_mix` | 類型 → 連結 → 類型；藝人與 mix 連結的每種變化；`parse_item` 不變 | 存根 `parse_link` 什麼都讀不出來 |
| AC2 | `crates/core/src/ui/popup/tests.rs` :: `ac2_copy_link_entries`（表格） | 各種列的動作的最後一項，以及在 *Collected Tracks* 與選取上沒有它 | 沒有 *Copy link* 項目 |
| AC3 | `crates/core/src/ui/clipboard.rs` :: `ac3_copy_link_from_popup` + `crates/core/src/ui/clipboard.rs` :: `ac3_copied_message`（表格） | 彈出視窗關閉且 `Effect::Copy` 帶著連結；每種結果的訊息 | 該項目什麼都不做 |
| AC4 | `crates/core/src/ui/keymap/tests.rs` :: `ac4_copy_link_action` + `crates/core/src/ui/clipboard.rs` :: `ac4_copy_link_binding`（表格） | 名稱以兩種 target 解析且不產生提示；兩種 target 的 effect，在 *Collected Tracks* 上沒有 | `CopyLink` 被略過並出現提示 |
| AC5 | `crates/core/src/ui/keymap/tests.rs` :: `ac5_open_link_from_clipboard_command` + `crates/core/src/ui/clipboard.rs` :: `ac5_reads_the_clipboard_on_a_page`（表格） | 預設 `g p`、沒有提示、說明文字；頁面上產生 `Effect::ReadClipboard`，在彈出視窗、輸入提示或說明之上沒有 | 指令被略過並出現提示 |
| AC6 | `crates/core/src/ui/clipboard.rs` :: `ac6_clipboard_text`（表格）+ `crates/core/src/ui/clipboard.rs` :: `ac6_late_answer_dropped` | 每段文字：推入並請求的頁面、加到尾端的 `Open`，或訊息；在彈出視窗、輸入提示或說明之下丟棄回應 | 回應被忽略 |
| AC7 | `crates/app/src/tui/clipboard.rs` :: `ac7_provider`（表格） | 環境 → 提供者 | 存根一律選 OSC 52 |
| AC8 | `crates/app/src/tui/clipboard.rs` :: `ac8_copy`（表格、假實作 runner） | 執行的程式與參數、給的輸入、每種 runner 結果對應的結果 | 存根什麼都不執行並失敗 |
| AC9 | `crates/app/src/tui/clipboard.rs` :: `ac9_read`（表格、假實作 runner）+ `crates/app/src/tui/clipboard.rs` :: `ac9_runner_cuts_and_times_out`（真的 `sh`） | 執行的程式與每種結果；以真實行程驗證 64 KiB 截斷與 2 秒終止 | 存根什麼都不執行並失敗 |
| AC10 | `crates/app/src/tui/clipboard.rs` :: `ac10_osc52`（表格） | 幾個連結的序列 | 存根傳回空序列 |
| AC11 | 驗收時檢查：對照本規格閱讀這些頁面、範例與 README，並以 `grep` 確認略過清單中已沒有這兩個名稱 | 無測試：文字內容 | — |

手動檢查（實際操作，live Tidal、tmux 150 × 42、Wayland）：在專輯上 *Copy link*，接著 `wl-paste` 印出它的連結；`wl-copy` 一個藝人連結，`g p` 開啟該藝人頁面；`wl-copy` 一個曲目連結，`g p` 把它加入佇列；`wl-copy` 一段文字，`g p` 顯示 `Not a Tidal link: …`。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core::item`：`Link`（五種類型）、`Link::url`、`parse_link`，與 `parse_item` 共用讀取邏輯
- `tidal-player-core::ui`：`MenuAction::CopyLink` 與彈出視窗的清單（`popup`）、`ActionKind::CopyLink` 與 `UiCommand::OpenSpotifyLinkFromClipboard`（`keymap`）、說明文字（`help`）、`Effect::Copy`／`Effect::ReadClipboard` 與 `Action::Copied`／`Action::ClipboardRead`（`mod`）、它們的處理（新的 `clipboard` 模組）
- `tidal-player` 執行檔，`tui::clipboard`：提供者的選擇、各提供者的指令、訊息、OSC 52（純函式），以及位於 `Runner` trait 後、有假實作的行程執行器；`main.rs` 在 TUI 的迴圈中執行這些 effect，並把結果當成下一個畫面的動作送回

<a id="facts-vs-assumptions"></a>
## 事實與假設

已驗證（2026-11-16，在維護者的機器上，Sway、wl-clipboard 2.3.0、xclip 0.13、xsel 1.2.1；剪貼簿本身未被改動）：

- 剪貼簿中沒有所要求類型的內容時，`wl-paste --no-newline` 與 `xclip -selection clipboard -o` 以狀態 1 結束，並在標準錯誤寫出一行（以沒有任何程式提供的類型探測）
- `wl-copy` 與 `xclip` 預設會 fork，並在背景提供剪貼簿內容（見它們的 man page：`--foreground`、`-quiet` 可關閉），所以啟動的行程很快就結束，而子行程仍開著繼承來的標準輸出與標準錯誤
- spotify-player 的原始碼（`spotify_player/src/command.rs`、`event/mod.rs`、它的 README）：`CopyLink` 是曲目、專輯、藝人與播放清單上的動作；`OpenSpotifyLinkFromClipboard` 是綁定 `O` 的指令，曲目連結會直接播放，播放清單、藝人或專輯連結則開啟其頁面。兩者目前都在本按鍵對應的略過清單中（`crates/core/src/ui/keymap.rs`）

在手動檢查前的假設：

- mix 連結的形式是 `https://tidal.com/browse/mix/<id>`（0011 的 mix ID 與 Tidal 分享選單使用的形式）；手動檢查會開啟一個

<a id="decisions"></a>
## 決策

1. **指令的名稱**：spotify-player 的 `OpenSpotifyLinkFromClipboard`，讓它的 `keymap.toml` 原樣可用（0008 的規則：有 spotify-player 名稱時就用它）。落選：自己的名稱（`OpenLinkFromClipboard`），較清楚，但同一件事多了第二個名稱
2. **它的預設按鍵**：`g p`。spotify-player 的 `O` 在這裡是 `PlayNextPrompt`，改動它會破壞 0007 的按鍵。落選：完全不設預設按鍵
3. **剪貼簿中的曲目連結**：加到佇列尾端，和 `o` 一樣。spotify-player 會直接開始播放它，取代正在播放的內容；這裡沒有東西會在清單上沒按 `Enter` 時取代佇列。落選：立刻播放；開啟它的專輯
4. **剪貼簿工具**：系統的工具（`wl-clipboard`、`xclip`、`xsel`），以行程執行，沒有顯示器時用 OSC 52。落選：剪貼簿 crate（`arboard`），新的相依套件，且在 Wayland 上需要 TUI 沒有的視窗焦點；強制指定提供者的設定（等有人需要再說）
5. **透過 SSH 讀取**：不提供；多數終端機關閉 OSC 52 的讀取，而且它透過終端機的輸入回應，TUI 會把它當成按鍵讀取。落選：帶逾時的 OSC 52 讀取

<a id="out-of-scope"></a>
## 不在範圍內

- 複製一個選取的多個連結（多行），或沒有游標所在列時複製頁面（顯示中的專輯或播放清單）的連結
- 佇列、*Collected Tracks*、電台或瀏覽頁面的連結
- 從前後有其他文字的內容中讀出連結
- `app.toml` 中的 `clipboard` 設定，或自訂的複製／貼上指令（spotify-player 的 `copy_command`／`paste_command`）
- 處理連結的 `tidal-player playback` 一次性指令（#67）
