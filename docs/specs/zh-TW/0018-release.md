[English](../0018-release.md) | 正體中文

<a id="0018--the-release-workflow"></a>
# 0018 — 發行工作流程

- **狀態**：implemented（2026-10-24；Bug 4、AC10 與 AC11 於 2026-11-11 加入）
- **負責人**：tech-lead（主要 session）
- **Issue**：[#50](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/50)、[#73](https://github.com/PEKKA1117/spotify-player-but-for-tidal/issues/73)（Bug 3）
- **相依於**：0001（implemented：工作區的 crate 與它們的相依規則）
- **使用者文件**：無（維護者工具）；`CLAUDE.md` 的「Build & tooling」指出 `cargo xtask publish`

<a id="context"></a>
## 背景

推送 `vX.Y.Z` 標籤會執行 `.github/workflows/release.yml`（在 #26、#29、#31 中加入，沒有規格）：它把標籤與 `tidal-player` 的版本核對、建置二進位檔、把它與原始碼及其 SHA-256 一起打包、等待該標籤的 CI 執行、把 crate 發佈到 crates.io，並建立 GitHub release。自 #43 起，一次發行只升級程式碼有改變的 crate，所以一次發行可能讓某個 crate 停在 crates.io 已有的版本。發佈步驟 `cargo publish --workspace` 無法跳過那些，而 `v1.2.0` 就在那裡失敗了（Bug 1）。

<a id="behaviour"></a>
## 行為

與目前相同，除了發佈步驟：

1. 標籤必須等於 `v` + `tidal-player` 的版本，否則執行在建置任何東西之前就失敗
2. 建置發行二進位檔；產生 `tidal-player-vX.Y.Z-src.zip`、`tidal-player-vX.Y.Z-x86_64-unknown-linux-gnu.zip` 與 `SHA256SUMS`
3. 執行會等待被標記的 commit（`GITHUB_SHA`）上對 `master` 的 push 的 CI 執行，若它失敗，或不存在（crates.io 的上傳無法收回），就失敗。發行標籤是推在 PR 的合併 commit 上，`master` 的 CI 已經檢查過它；標籤的推送不會執行自己的 CI（Bug 3，規格 0001 AC9），所以不在 `master` 上的 commit 上的標籤會被拒絕
4. **發佈**（`cargo xtask publish`）：每個可以發佈（不是 `publish = false`）、且其版本**尚未在 crates.io 上**的工作區 crate，都依相依順序發佈（`tidal-player-core` 在 `tidal-player-api` 與 `tidal-player-audio` 之前，那兩者在 `tidal-player` 之前）。版本已經在那裡的 crate 會被跳過並指名（`tidal-player-api 1.1.0: already on crates.io`）。被 crates.io 撤回（yanked）的版本算作已經在那裡（它拒絕重新上傳）。沒有新版本：`nothing to publish`，執行繼續
5. 建立 GitHub release：`.github/release-notes.md` 的安裝步驟、GitHub 產生的說明，以及這三個檔案（之後維護者加上它的 `## New in` 一節，`CLAUDE.md` 的「Conventions」）

`cargo xtask publish --dry-run` 印出同樣的計畫而不發佈任何東西，所以可以在打標籤之前於本機檢查計畫。

**crates.io 上有什麼**是從 crates.io 的稀疏索引讀取（`https://index.crates.io/<path>`，每行一個 JSON 物件，帶有它的 `vers` 與 `yanked`；`<path>` 依名稱長度為 `1/<name>`、`2/<name>`、`3/<n>/<name>` 或 `<na>/<me>/<name>`，轉為小寫）。404 表示該 crate 從未發佈過，所以它的所有版本都是新的。索引以 `curl` 取得（在 runner 上以及任何開發者機器上），藏在測試會以假實作取代的接縫（seam）之後。

**發佈**一個 crate 會執行 `cargo publish -p <name>`，它會等到 crates.io 有了它之後下一個才開始。第一個失敗會停止執行並指名該 crate；在它之前已發佈的 crate 維持已發佈（重新執行時就會依步驟 4 跳過它們）。

<a id="acceptance-criteria"></a>
## 驗收條件

`xtask`（`xtask/src/publish.rs`，純函式加上一個建立在兩個接縫之上的指令：索引的取得與行程執行器）：

- **AC1** — 計畫（表格測試）：輸入是工作區的 crate（名稱、版本、是否可發佈、工作區相依套件），順序任意，以及每個 crate 在 crates.io 上的版本；輸出是依相依順序要發佈的 crate 與被跳過的。列：每個版本都是新的；今天的情形（`api` 與 `audio` 在 1.1.0 已發佈，`core` 與 `tidal-player` 在 1.2.0 是新的 → `core`、`tidal-player`；`api` 與 `audio` 被跳過）；沒有新的 → 空；從未發佈過的 crate → 納入計畫；`publish = false`（`xtask`）→ 既不納入計畫也不被跳過；被撤回的版本 → 被跳過；輸入順序被打亂 → 相同的相依順序
- **AC2** — 索引（表格測試）：名稱長度為 1、2、3、4 與更長時的路徑，大寫轉成小寫；把索引行（索引真實格式的測試資料，兩個版本，一個被撤回）解析成帶有撤回旗標的版本；空白行被忽略；不是 JSON 或沒有 `vers` 的行 → 一個指出 crate 與行號的錯誤
- **AC3** — 指令（假的取得器與執行器，表格測試）：`publish` 對每個可發佈的 crate 取得一次，然後依序對每個納入計畫的 crate 執行 `cargo publish -p <name>`，並印出每個被跳過的；404 → 從未發佈過；404 以外的取得失敗 → 一個錯誤，且**不**執行任何 `cargo publish`；失敗的 `cargo publish` → 在那裡停止，錯誤指出該 crate，之後的 crate 不會執行；`--dry-run` → 印出計畫且什麼都不執行；空的計畫 → 印出 `nothing to publish` 並成功
- **AC4** — 工作流程：`release.yml` 的發佈步驟以 `CARGO_REGISTRY_TOKEN` 執行 `cargo xtask publish`，且沒有任何步驟執行 `cargo publish --workspace`（測試讀取該檔案）
- **AC5** — 重新發行 `v1.2.0`（驗收時手動；它需要真實的 crates.io 與一個標籤）：執行發佈 `tidal-player-core` 1.2.0 與 `tidal-player` 1.2.0，跳過 `api` 與 `audio` 1.1.0，並建立帶有三個檔案的 GitHub release。沒有自動化測試：它會真的發佈
- **AC6** — 固定的內部相依套件（Bug 2）：工作區 crate 對另一個帶有版本的工作區 crate 的每個相依套件，都要求恰好那個版本（`=x.y.z`，單一個 `=` 比較子），所以已發佈的 crate 需要與它一起發行的那些 crate（測試讀取真實的工作區）
- **AC7** — 發行的 CI 關卡（Bug 3；測試讀取 `release.yml`）：發佈步驟之前、等待 CI 的步驟以 `--event push`、`--branch master` 與 `--commit "${GITHUB_SHA}"` 列出 `ci.yml` 的執行，絕不以標籤的名稱（`GITHUB_REF_NAME`）作為分支；沒有執行時它會失敗（`exit 1`）；否則它以 `--exit-status` 監看該執行，所以失敗或被取消的執行會使發行失敗
- **AC8** — CI 每個 commit 只執行一次（Bug 3；測試讀取 `ci.yml`）：它的觸發條件恰好是推送到 `branches: [master]` 與 `pull_request`，所以標籤推送與推送到任何其他分支都不執行任何東西；每個工作的 `if` 是 `github.event_name == 'push' || startsWith(github.head_ref, 'feat/') || startsWith(github.head_ref, 'bugfix/') || startsWith(github.head_ref, 'refactor/')`（`master` 的推送，以及來自 `feat/*`、`bugfix/*` 與自 [0026](0026-realignment.md) 起的 `refactor/*` 的 pull request，後者會執行 CI 且不是一次發行）
- **AC9** — 修正之後的第一次發行（驗收時手動，如 AC5）：在它的合併 commit 上，Actions 顯示一次 CI 執行（對 `master` 的 push）而標籤沒有，且發行的等待步驟監看那次執行，然後發佈。沒有自動化測試：它需要真實的發行
- **AC10** — 過時的相依者（Bug 4；表格測試）：計畫跳過的每個 crate（已經在 crates.io 上，版本相同）的索引項目，說明那個版本需要哪些工作區 crate；在它恰好需要某一個（`=x.y.z`，AC6）、而工作區中該 crate 的版本是另一個時，`publish` 會在任何 `cargo publish` 之前失敗，`--dry-run` 下也一樣，在計畫的各行之後，每個這樣的需求一行：`<dependent> <version> on crates.io needs <dependency> =<old>, but this release has <dependency> <new>: bump <dependent>`。不論該相依套件是在這次執行中被發佈，還是被更早的、失敗的執行發佈，它都成立。不精確的需求（AC6 之前的各次發行）、dev-dependencies 與工作區之外的 crate 不比較；沒有這樣的需求時它如先前執行
- **AC11** — CI 中的 dry run（Bug 4；測試讀取 `ci.yml`）：`check` 工作在 `cargo xtask specs` 之後執行 `cargo xtask publish --dry-run`，所以有過時相依者的發行 PR 會在被合併與打標籤之前失敗

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 行為 |
|---|---|
| crates.io 無法連線，或索引回應 5xx | 在發佈任何東西之前出錯；重新執行工作 |
| 在檢查與上傳之間有版本被發佈（兩個執行同時進行） | `cargo publish` 在它上面失敗，執行停止；`concurrency: release` 已經讓執行一次只有一個 |
| 工作區新增了新的 crate | 從不在索引上（404）：與其他 crate 一起依相依順序發佈 |
| 一個 crate 已發佈，之後的一個失敗 | 已發佈的維持已發佈；重新執行會跳過它並發佈其餘的 |
| 缺少 `curl` | 指出它的錯誤；什麼都不發佈 |
| 標籤在不在 `master` 上的 commit 上（它沒有 `master` 的 CI 執行） | 在等待（5 分鐘）之後，錯誤 `no CI run on master for <sha>: tag a commit of master`；什麼都不發佈 |
| 被標記的 commit 上 `master` 的 CI 執行失敗或被取消 | 等待失敗；什麼都不發佈。在新的 PR 上修正並為它的合併 commit 打標籤，或重新執行該 CI 執行（若它不穩定），然後重新執行發行工作 |
| 推送標籤時 `master` 的 CI 執行仍在進行中 | 發行會等待它 |

<a id="test-plan"></a>
## 測試計畫

紅燈是對存根函式（存根本體，沒有 `todo!()`，沒有編譯錯誤）的失敗斷言。

| AC | 測試（檔案 :: 名稱） | 驗證內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `xtask/src/publish.rs` :: `ac1_plan`（表格） | 依順序的納入計畫與被跳過的 crate | 存根的 `plan` 依輸入順序把每個可發佈的 crate 都納入計畫 |
| AC2 | `xtask/src/publish.rs` :: `ac2_index_path`（表格）、`ac2_parse_index`（表格） | 路徑；版本與撤回旗標；錯誤 | 存根的路徑是單純的名稱；存根的解析器不回傳任何版本 |
| AC3 | `xtask/src/publish.rs` :: `ac3_publish_command`（表格） | 取得、`cargo publish` 呼叫及其順序、輸出、錯誤 | 存根指令對每個 crate 執行 `cargo publish -p` |
| AC4 | `xtask/tests/release_workflow.rs` :: `ac4_workflow_uses_xtask_publish` | 該步驟的 `run:`，且沒有 `--workspace` 的發佈 | 工作流程仍有 `cargo publish --workspace`（紅燈的 commit 留著它） |
| AC5 | 驗收時手動（見 AC5） | 真實的發行 | — |
| AC6 | `xtask/tests/workspace.rs` :: `ac6_internal_dependencies_are_pinned` | 每個內部相依套件的需求是一個 `=` 比較子 | `[workspace.dependencies]` 仍有脫字號（caret）需求（`^1.2.1`） |
| AC7 | `xtask/tests/release_workflow.rs` :: `ac7_release_waits_for_master_ci` | 等待步驟的 `gh run list` 旗標、沒有執行時的 `exit 1`、`gh run watch --exit-status`，以及它在發佈步驟之前 | 該步驟仍以 `--branch "${GITHUB_REF_NAME}"` 列出執行（紅燈的 commit 讓 `release.yml` 維持原樣） |
| AC8 | `xtask/tests/release_workflow.rs` :: `ac8_ci_runs_once_per_commit` | `ci.yml` 的 `on:` 區塊（去掉註解與空白行）恰好是那三行觸發條件；每個工作（`runs-on:`）恰有一個等於預期值的 `if:` | `on:` 仍有單純的 `push:`（紅燈的 commit 讓 `ci.yml` 維持原樣） |
| AC9 | 驗收時手動（見 AC9） | 合併之後的第一次真實發行 | — |
| AC10 | `xtask/src/publish.rs` :: `ac10_stale_dependents`（表格） | 失敗與它的各行；沒有發佈任何東西；dry run 也一樣；沒有這種情況就不失敗 | 存根找不到任何過時的相依者，所以執行成功 |
| AC11 | `xtask/tests/release_workflow.rs` :: `ac11_ci_dry_runs_the_publish` | 該步驟的 `run:`，以及它接在 specs 步驟之後 | `ci.yml` 沒有這樣的步驟 |

<a id="bugs"></a>
## 錯誤

<a id="bug-1--cargo-publish---workspace-fails-when-a-crate-keeps-a-published-version-50"></a>
### Bug 1 — 當某個 crate 維持已發佈的版本時，`cargo publish --workspace` 失敗（#50）

- **預期**：`v1.2.0` 標籤發佈 `tidal-player` 1.2.0 與 `tidal-player-core` 1.2.0，並建立 release
- **實際**：執行 40090212356 在「Publish to crates.io」失敗：`error: crate tidal-player-api@1.1.0 already exists on crates.io index`。什麼都沒上傳，也沒有建立 release
- **根本原因**：`cargo publish --workspace` 發佈每個可發佈的 crate，而且沒有跳過已在 crates.io 上的版本的選項；#43 的規則讓沒有改變的 crate 停在它們已發佈的版本
- **新的條件**：AC1（今天的情形）、AC4。`v1.2.0` 標籤被移到合併後的修正並再次推送（AC5）

<a id="bug-2--internal-dependencies-use-caret-requirements-74"></a>
### Bug 2 — 內部相依套件使用脫字號需求（#74）

- **預期**：`cargo install tidal-player --version <an older version>` 建置該版本發行時所搭配的 crate
- **實際（風險）**：`[workspace.dependencies]` 給 `tidal-player-core`、`-api` 與 `-audio` 脫字號需求（`version = "1.2.1"`）。修正或功能是修補版本發行，而主版本維持 `1`（`CLAUDE.md` 的「Versions」），所以函式庫之後的 `1.x` 可能以不相容的方式改變它的 API。沒有 `--locked` 時，cargo 會把較舊的 `tidal-player` 解析到最新的 `1.x`，而安裝無法編譯
- **根本原因**：需求遵循 cargo 的預設（`^`），它假設了發行規則並不保證的 semver 相容性
- **新的條件**：AC6。內部 crate 在 `[workspace.dependencies]` 中的項目使用 `version = "=x.y.z"`，而被升級的 crate 的項目得到 `=<new version>`。從修正之後的第一次發行起適用；到 v1.2.1 為止已發佈的版本維持 `^`（`cargo install --locked` 可以繞過它）

<a id="bug-3--every-release-commit-and-every-pr-push-runs-ci-twice-73"></a>
### Bug 3 — 每個發行 commit（以及每次 PR 推送）都執行 CI 兩次（#73）

- **預期**：每個 commit 只被檢查一次，而且除非 CI 在被發行的 commit 上通過，否則不發佈任何東西
- **實際**：`ci.yml` 在每次 `push` 與 `pull_request` 時執行。合併 PR 會把它的合併 commit 推送到 `master`（一次 CI 執行），然後發行標籤被推在同一個 commit 上（第二次、完全相同的執行）：`d04867c` 上的 `v1.2.1` 執行了 [master](https://github.com/PEKKA1117/spotify-player-but-for-tidal/actions/runs/40168880426) 與 [標籤](https://github.com/PEKKA1117/spotify-player-but-for-tidal/actions/runs/40169113802)。有開啟 PR 的 `feat/*` 或 `bugfix/*` 分支，每次推送都會執行一次 `push` 與一次 `pull_request`
- **根本原因**：觸發條件是單純的 `push` 與 `pull_request`，而發行（步驟 3）等待的是標籤自己的 CI 執行，所以標籤的那次執行無法被拿掉
- **新的條件**：AC7（發行等待 `GITHUB_SHA` 上 `master` 的執行）、AC8（CI 只在推送到 `master` 與 pull request 時執行；規格 0001 AC9 隨之改變）、AC9（修正之後的第一次發行）

<a id="bug-4--a-bumped-library-does-not-force-its-dependents-to-bump-99"></a>
### Bug 4 — 被升級的函式庫不會強迫它的相依者升級（#99）

- **預期**：crate 無法一起發佈的發行，會在它的 PR 上失敗
- **實際**：`v1.5.0` 把 `tidal-player-core` 升級到 1.5.0，並讓 `tidal-player-api` 停在 1.3.4（它的程式碼沒有改變）。在 AC6 的精確固定之下，已發佈的 `tidal-player-api 1.3.4` 需要 `tidal-player-core =1.4.4`，而 `tidal-player 1.5.0` 需要 `=1.5.0`，所以 `cargo publish -p tidal-player` 無法解析。`cargo xtask publish --dry-run` 印出計畫卻什麼都沒說，CI 通過了，而 [Release 執行](https://github.com/PEKKA1117/spotify-player-but-for-tidal/actions/workflows/release.yml) 發佈了 `tidal-player-core 1.5.0` 並在 `tidal-player` 失敗：沒有 `tidal-player 1.5.0`，也沒有 GitHub release
- **根本原因**：AC6 讓「某個 crate 相依於被升級的 crate」成為升級它的理由，卻沒有任何東西檢查這件事。索引說明每個已發佈的版本需要什麼，而 `publish` 從未讀取它
- **新的條件**：AC10（`publish` 把每個被跳過的 crate 已發佈版本所需要的，與工作區的版本比較）、AC11（CI 執行 dry run）。`CLAUDE.md` 的「Versions」也這樣說
- **復原**：`tidal-player-api` 1.3.5 固定 `tidal-player-core =1.5.0`；沒有任何 release 是從它建置出來的標籤 `v1.5.0`，移到修正的合併 commit（維護者的決定），而工作流程發佈 `tidal-player-api 1.3.5` 與 `tidal-player 1.5.0`

<a id="implementation-notes"></a>
## 實作備註

- 輸出的行：被跳過的是 `<name> <version>: already on crates.io`，每個 `cargo publish` 之前是 `publishing <name> <version>`，`--dry-run` 下是 `would publish <name> <version>`；先印出被跳過的，然後在沒有任何東西納入計畫時印 `nothing to publish`
- 所有索引的取得都發生在第一個 `cargo publish` 之前，所以 crates.io 當機時不會上傳任何東西；被跳過的清單也依相依順序
- 取得器執行 `curl -sS -w '\n%{http_code}'` 並從最後一行讀取狀態；200 是一個項目，404 是從未發佈過，其他任何東西都是錯誤
- 相依順序使用成員之間的一般與建置相依套件（`cargo metadata --no-deps`）；平手時依名稱

<a id="out-of-scope"></a>
## 不在範圍內

- 發佈到 crates.io 以外的任何地方（AUR、其他目標的二進位檔）
- 自動撰寫 `## New in` 一節
- 檢查有改變的 crate 的版本是否已升級（維護者的規則，`CLAUDE.md` 的「Conventions」）
