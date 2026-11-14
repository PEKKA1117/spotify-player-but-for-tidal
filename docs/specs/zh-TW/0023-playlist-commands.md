[English](../0023-playlist-commands.md) | 正體中文

<a id="0023--playlist-commands"></a>
# 0023 — 播放清單指令

- **狀態**：implemented（2026-11-05）
- **負責人**：tech-lead（主要工作階段）
- **Issue**：[#88](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/88)
- **相依於**：0005（implemented：socket、一次性指令、結束碼）、0006（implemented：音樂庫請求）、0022（implemented：建立、編輯與公開的請求）
- **使用者文件**：[`docs/daemon.md`](../../zh-TW/daemon.md)（新增「Playlists from a shell」一節）及其 zh-TW 副本、`README.md` 及其 zh-TW 副本（AC11）

<a id="context"></a>
## 背景

0022 讓 TUI 可以建立與編輯播放清單，但腳本只能透過 `tidal-player playback` 連到播放器，而它沒有播放清單指令。spotify-player 有 `playlist new/delete/import/fork/update/list`。本規格新增 `tidal-player playlist`，它像 `playback` 一樣與執行中的播放器溝通。

tidalt 沒有這些。

<a id="behaviour"></a>
## 行為

`tidal-player playlist <command>` 透過 socket 把它的請求送給執行中的播放器（獨立模式、常駐程式或 `play`），然後結束。

`PLAYLIST`、`FROM` 與 `TO` 是播放清單的 UUID（如 `list` 所印出的），或它的 Tidal 連結（見[項目](../../zh-TW/playback.md#items)）。不接受標題：標題會重複，而 `delete` 不能用猜的。

| 指令 | 作用 | 成功時印出 |
|---|---|---|
| `list`、`list --json` | 列出你的播放清單與你追蹤的播放清單，最新的在前 | 見下 |
| `new NAME [-d DESCRIPTION] [--public]` | 建立播放清單，除非有 `--public`，否則為私人 | 新的 UUID |
| `edit PLAYLIST [--name NAME] [--description TEXT] [--public \| --private]` | 在你自己的播放清單上，變更有給定的項目。`--description ""` 清除描述 | 無 |
| `delete PLAYLIST [--yes]` | 刪除你自己的一個播放清單 | 無 |
| `fork PLAYLIST [--name NAME]` | 把播放清單的曲目與描述複製到新的私人播放清單，預設名為 `<title> (copy)` | 新的 UUID |
| `import FROM TO [--duplicates]` | 把 `FROM` 的曲目依 `FROM` 的順序加到 `TO`（你自己的一個播放清單）的結尾 | `Added 12 tracks to <title>, skipped 3` |

結束碼與 `playback`（0005）相同：

- `0`：完成
- `1`：沒有執行中的播放器（`No player is running: …`）、stderr 上播放器的錯誤，或 5 秒內沒有回答（`The player did not answer`）
- `2`：錯誤的引數，沒有送出任何東西

<a id="list"></a>
### `list`

每行一個播放清單，以 tab 分隔：UUID、`own` 或 `followed`、曲目數、標題。

```
$ tidal-player playlist list
a5f9389f-ab20-4d4a-937a-a21ec2559abb	own	39	Late night
edad2071-060a-4392-870b-4dc9c5c6ac66	followed	25	Linkin Park Essentials
```

標題中的 tab 與換行會印成空格，讓一行仍是一行。`--json` 印出單一個 `{"uuid", "title", "own", "public", "tracks", "description"}` 的陣列，其中 `tracks` 與 `description` 在未知或為空時為 `null`。

<a id="asking-before-delete"></a>
### `delete` 之前先詢問

`delete` 先讀取播放清單。在終端機上，它在 stderr 詢問 `Delete <title>? (y/n)`：`y` 就刪除，其他任何回答都以 0 結束且不刪除任何東西。`--yes` 不詢問。stdin 沒有終端機又沒有 `--yes` 時，它以 2 結束並印出 `Use --yes to delete without a terminal`，不刪除任何東西。

<a id="your-own-playlists-only"></a>
### 只限你自己的播放清單

`edit`、`delete` 與 `import` 的 `TO` 先讀取播放清單的標頭。不是你擁有的那個會以 1 結束並印出 `<title> is not your playlist`，而且不寫入任何東西。

<a id="fork-and-import"></a>
### `fork` 與 `import`

兩者都在客戶端中由音樂庫請求組成：

1. 來源的曲目逐頁讀取（`More`，一次 100 首）。`import` 也讀取 `TO` 的
2. `fork` 建立播放清單。`import` 保留 `FROM` 中不在 `TO` 裡的曲目（以曲目 ID 判斷），以及 `FROM` 中重複者的第一個。`--duplicates` 全部保留
3. 曲目一次 100 首、依序加入：每次一個 `AddToPlaylist`，並設定 `allow_duplicates`，所以各自有自己的 5 秒

來源中的影片不會被複製：播放器的清單只含曲目（0006）。來源有影片時，stderr 會說 `Skipped 2 videos`，數字是它的總數減去它的曲目數。

若在寫入任何東西之後某個步驟失敗，stderr 會說明進行到哪裡，結束碼為 1：

```
Added 300 of 1 234 tracks to Late night (copy): Could not reach Tidal: timeout
```

`fork` 此時仍會在 stdout 印出新的 UUID，讓腳本找得到留下的東西。不會回復任何東西。

沒有曲目的來源沒問題：`fork` 建立空的播放清單，而 `import` 印出 `Added 0 tracks to <title>, skipped 0`。

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — 解析：表中的每一列都解析成它的計畫；`PLAYLIST` 接受單純的 UUID（`parse_item` 會拒絕它，所以在這裡先讀取）或播放清單連結；曲目、專輯或無法解析的文字是以 2 結束且不送出任何東西，空白的 `NAME`（`new`、`edit --name`、`fork --name`）、沒有任何要變更的 `edit`，以及 `edit --public --private` 也一樣
- **AC2** — `list` 透過 `More { Playlists }` 翻頁到底，印出以 tab 分隔的行，標題中的 tab 與換行變成空格；`--json` 印出陣列
- **AC3** — `new` 送出 `CreatePlaylist`（標題經過修剪）並印出 UUID；`--public` 接著送出 `SetPlaylistPublic`，若它失敗，stdout 仍有 UUID，結束碼為 1
- **AC4** — `edit` 送出一個只含有給定項目的 `EditPlaylist`，若有要求再送出 `SetPlaylistPublic`；只有公開旗標時不送出 `EditPlaylist`；`--description ""` 送出 `Some("")`
- **AC5** — 自己的播放清單：`edit`、`delete` 與 `import` 的 `TO` 讀取標頭，不是自己擁有的以 1 結束並印出 `<title> is not your playlist`，而且不寫入
- **AC6** — `delete`：`--yes` 不詢問就刪除；在終端機上 `y` 刪除，其他任何回答以 0 結束且不送出任何東西；沒有終端機又沒有 `--yes` 時，在送出任何東西之前以 2 結束
- **AC7** — `fork` 讀取來源的每一頁，以來源的描述建立播放清單（名稱為給定的，或 `<title> (copy)`），一次 100 首、依序加入曲目，並印出 UUID。空的來源建立空的播放清單
- **AC8** — `import` 把 `FROM` 的曲目依序、一次 100 首加到 `TO` 的結尾，略過 `TO` 中已有的以及 `FROM` 中重複的，並印出 `Added N tracks to <title>, skipped M`；`--duplicates` 全部加入
- **AC9** — 影片與部分失敗：曲目數少於總數的來源在 stderr 印出 `Skipped N videos`；寫入之後的失敗印出 `Added N of M tracks to <title>: <reason>`，結束碼 1，而 `fork` 仍印出 UUID
- **AC10** — 透過執行檔的結束碼：沒有執行中的播放器是 1，帶著 0005 的訊息；錯誤的引數是 2；沉默的播放器是 `The player did not answer`，1
- **AC11** — `docs/daemon.md`、`README.md` 及它們的 zh-TW 副本記載這些指令

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| 未登入 | 播放器的 `LoginRequired` 訊息，結束碼 1 |
| `FROM` 與 `TO` 是同一個播放清單 | 以 2 結束，不送出任何東西：`FROM and TO are the same playlist` |
| 不存在的播放清單 | `Playlist <uuid> was not found`，結束碼 1 |
| 讀取期間清單改變 | 頁面以偏移量讀取：曲目可能漏掉或被讀到兩次。`import` 移除重複，其他不再檢查 |
| `fork` 追蹤的播放清單 | 允許：fork 本來就是為此 |
| 從追蹤的播放清單 `import` | 允許：只有 `TO` 必須是你自己的 |
| 已不再提供的曲目 | 依 Tidal 列出的複製；Tidal 可能拒絕它們，這和其他失敗一樣是失敗（AC9） |
| 前後有空格的 `NAME` | 修剪，如 TUI 的輸入提示（0022） |
| 匯入期間另一個客戶端編輯 `TO` | 播放器每次加入都讀取 ETag 並重試一次（0006）；第二個 `412` 和其他失敗一樣是失敗（AC9） |
| 沒有播放清單的 `list --json` | `[]` |

<a id="test-plan"></a>
## 測試計畫

| AC | 測試 | 斷言內容 | 預期的紅燈失敗 |
|---|---|---|---|
| AC1 | `crates/app/src/cli/playlist/tests.rs` :: `ac1_parse` (table), `ac1_refused` (table) | 計畫與用法錯誤 | 存根 `plan` 拒絕一切 |
| AC2 | `crates/app/src/cli/playlist/tests.rs` :: `ac2_list_lines`, `ac2_list_pages`, `ac2_list_json` | 輸出與 `More` 請求 | 什麼都沒送出 |
| AC3 | `crates/app/src/cli/playlist/tests.rs` :: `ac3_new`, `ac3_new_public_fails` | 請求、stdout、結束碼 | 什麼都沒送出 |
| AC4 | `crates/app/src/cli/playlist/tests.rs` :: `ac4_edit` (table) | 請求 | 什麼都沒送出 |
| AC5 | `crates/app/src/cli/playlist/tests.rs` :: `ac5_not_your_playlist` (table: edit, delete, import) | 訊息、沒有寫入 | 什麼都沒送出 |
| AC6 | `crates/app/src/cli/playlist/tests.rs` :: `ac6_delete` (table: `--yes`, `y`, `n`, no terminal) | 請求、結束碼 | 什麼都沒送出 |
| AC7 | `crates/app/src/cli/playlist/tests.rs` :: `ac7_fork`, `ac7_fork_batches`, `ac7_fork_empty` | 依序的請求、stdout | 什麼都沒送出 |
| AC8 | `crates/app/src/cli/playlist/tests.rs` :: `ac8_import`, `ac8_import_duplicates` | 送出的曲目、順序、該行 | 什麼都沒送出 |
| AC9 | `crates/app/src/cli/playlist/tests.rs` :: `ac9_skipped_videos`, `ac9_partial_failure` (table: fork, import) | stderr、stdout、結束碼 | 什麼都沒送出 |
| AC10 | `crates/app/tests/daemon.rs` :: `ac10_playlist_exit_codes`; `crates/app/src/cli/playlist/tests.rs` :: `ac10_no_answer_and_not_found` | 執行檔的結束碼與訊息；沉默的播放器 | 存根以 1 結束且沒有訊息 |
| AC11 | 於驗收時閱讀：沒有測試能判斷文章。`cargo xtask wiki` 會建置 | | |

**手動檢查**（即時 Tidal，對一個使用既有工作階段的隔離播放器，用完即丟的播放清單事後刪除）：帶描述與 `--public` 的 `new`、`edit`、`list` 與 `list --json | jq`、對超過 100 首曲目的播放清單做 `fork`、`import` 兩次（第二次什麼也不加）、帶 `--yes` 以及從終端機執行的 `delete`。它印到 shell，所以 PR 引用輸出而不是截圖。

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player`：`playlist_cli.rs`（新）：clap 指令、`plan`、各流程，對 `Link` 泛型；`main.rs` 接上子指令。`oneshot.rs` 保留它的 `REPLY_TIMEOUT` 與 `NO_ANSWER`
- `tidal-player-core`、`tidal-player-api`：沒有變更，請求已存在

<a id="facts-vs-assumptions"></a>
## 事實與假設

事實，來自程式碼與 0022 的探測：

- `ClientMessage::Library { id, request }` 與 `ServerMessage::LibraryReply { id, result }` 透過 socket 為任何客戶端攜帶每個音樂庫請求
- `AddToPlaylist` 讀取 ETag，並且每個請求最多送 100 首曲目給 Tidal；較大的請求在回答之前，每 100 首需要一個請求
- `PlaylistTracks` 的頁面最多 100 個項目，而 `ListPage::total` 把影片也算進去
- `PageData::Playlist` 持有 `own`、`description`、`tracks` 與第一頁。`own` 是標頭的 `creator.id` 與使用者的比較結果，對任何播放清單都適用
- `parse_item` 只讀取連結：單純的 UUID 被拒絕，單純的數字是曲目

假設：

- Tidal 沒有值得使用的原生 fork：沒有去找，因為以既有的請求組成它不需要新的端點，也不需要探測。代價是影片

在動手執行中檢查（2026-11-05，對隔離的常駐程式）：`own` 區分了追蹤的播放清單與自己的；111 首曲目的 `fork` 用了兩個批次與 4 秒；`import` 含有 2 部影片的播放清單加入了它的 39 首曲目並這麼說，再次執行時什麼也沒加；`delete` 在沒有終端機時拒絕，在有終端機時詢問。之後音樂庫的播放清單與之前取得的清單相符。

<a id="decisions"></a>
## 決策

1. **`delete` 在終端機上詢問，沒有終端機時拒絕**：它無法復原，腳本應該刻意地說 `--yes`
2. **`import` 略過 `TO` 已有的**：因此執行兩次什麼也不會加，這是腳本想要的。`--duplicates` 是例外
3. **`fork` 與 `import` 在客戶端執行**：播放器或 API 都不必變更，而且客戶端看得到每個步驟，所以失敗時可以說明進行到哪裡
4. **客戶端以 100 為一批**：單一個 4 000 首曲目的 `AddToPlaylist` 會超過一次性指令等待的 5 秒；100 首是對 Tidal 的一個請求
5. **不回復**：刪除做到一半的 fork 不會損失任何東西，卻也什麼都解釋不了；UUID 與計數讓使用者自己決定
6. **`list` 納入，雖然 issue 沒有指名它**：沒有別的東西能告訴使用者 UUID。`edit` 納入，因為它只是接上 0022 的請求

<a id="out-of-scope"></a>
## 不在範圍內

- spotify-player 的 `sync`，它保留了已匯入內容的記錄
- 以標題指名播放清單
- 從 CLI 移動或移除曲目
- 複製播放清單的影片
- 其他一次性指令（#67）
