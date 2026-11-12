[English](../0001-architecture.md) | 正體中文

<a id="0001--architecture--project-scaffold"></a>
# 0001 — 架構與專案骨架

- **狀態**：implemented（2026-10-06；2026-11-10 由 [0026](0026-realignment.md) 修訂：平臺規則、執行檔的配置與角色、記錄、AC8 與 AC9 現在描述實際建成的程式）
- **負責人**：tech-lead（主要工作階段）

<a id="context"></a>
## 背景

tidalt（Go + CGO + FFmpeg + ALSA）變得太多錯誤，無法再繼續修補。它的問題有一部分是結構性的，不只是局部的：

- UI 狀態、I/O 與播放全部糾纏在一個龐大的 BubbleTea model 裡，因此大部分行為只能靠驅動整個 UI 來測試
- CGO 加上動態或靜態連結的 FFmpeg，讓建置、CI 與封裝都很脆弱
- `CLAUDE.md` 變成逐檔案的變更記錄，而不是規格，使得預期的行為不存在於任何可測試的地方

本規格選定技術堆疊與 crate 配置，並建立一個空的 workspace，讓之後的每份規格都能以測試先行的方式在其中建構。除了 `--help`/`--version` 與一個空的 TUI 框架之外，它**不加入任何使用者看得到的功能**。

<a id="decisions"></a>
## 決策

<a id="name"></a>
### 名稱

執行檔、crate 前綴，以及設定與資料目錄的名稱都是 **`tidal-player`**（`~/.config/tidal-player/`、`~/.local/state/tidal-player/`），以此致敬 tidalt 的原作者。本規格中的「tidalt」一律指舊的 Go 專案。

<a id="language--toolchain"></a>
### 語言與工具鏈

- Rust，edition 2024，穩定版工具鏈固定在 `rust-toolchain.toml`（目前為 1.97），並附有 `rustfmt` 與 `clippy` 元件
- **僅支援 Linux**，與 tidalt 相同：ALSA 輸出、播放器與用戶端之間的 Unix socket 與鎖定檔（0005）、供常駐程式使用的 systemd、供 MPRIS（0010）使用的 session bus，以及裝置保留（0003）。只有 ALSA 後端需要 ALSA 函式庫，且位於 `alsa` feature 之後（AC7）；其餘部分在任何 Unix 上都能建置。Windows 是 issue #21，不是對現有程式碼的限制

<a id="workspace-layout"></a>
### Workspace 配置

```
Cargo.toml                 # [workspace], shared [workspace.dependencies] and [workspace.lints]
rust-toolchain.toml
crates/
  core/    -> tidal-player-core   # domain types, protocol, player + UI state machines, keymap. NO I/O.
  api/     -> tidal-player-api    # Tidal HTTP API client; maps API DTOs into core types
  audio/   -> tidal-player-audio  # decode + output engine; Sink trait, ALSA backend behind a feature
  app/     -> tidal-player        # the binary: CLI, player runtime (in-process or daemon), TUI client, rendering
                                  #   src/ is split by side: player/, tui/, cli/, session/, ipc/ (see 0026 "The binary's layout")
xtask/     -> xtask        # dev-only tooling; `cargo xtask layering` enforces the dependency rules below,
                           #   `cargo xtask specs` checks every spec's test plan against the code (0026)
.cargo/config.toml         # `xtask` alias
.github/workflows/ci.yml
```

允許的相依邊（其餘一律禁止）：

| crate                | 可相依於                              |
|----------------------|---------------------------------------|
| `tidal-player-core`  | —（只能用純函式庫：serde、thiserror……） |
| `tidal-player-api`   | `tidal-player-core`                   |
| `tidal-player-audio` | —（不可相依 `core`、`api`）           |
| `tidal-player`       | 以上全部                              |
| `xtask`              | 以上皆不可（僅開發工具）              |

<a id="application-pattern-the-main-testability-fix"></a>
### 應用程式模式（可測試性的主要修正）

採用 Elm 風格：純粹的核心加上命令式的外殼，並沿著**播放器／用戶端的邊界**切分，讓承襲自 tidalt 的常駐程式－用戶端模式從一開始就內建，而不是事後硬加上去：

- **播放器端** — `tidal_player_core::player`：`PlayerState`（佇列、目前曲目、位置、音量、隨機播放／重複播放、輸出裝置）、`PlayerInput`（用戶端的 `Command`、音訊引擎事件、API 結果、tick）與 `PlayerEffect`（解析串流、啟動／停止／跳轉引擎、持久化、廣播 `Event`）。`fn update(&mut PlayerState, PlayerInput) -> Vec<PlayerEffect>`
- **用戶端** — `tidal_player_core::ui`：`State`（頁面、游標、覆蓋層、用戶端最後得知的播放器狀態副本）、`Action`（已透過按鍵對應解析的按鍵、來自播放器的 `Event`、瀏覽用的 API 結果、tick）與 `Effect`（送出 `Command`、取得一頁資料、結束用戶端）。`fn update(&mut State, Action) -> Vec<Effect>`
- **協定** — `tidal_player_core::protocol`：`Command`（用戶端 → 播放器：播放、暫停、下一首、跳轉、佇列編輯、音量……）與 `Event`（播放器 → 用戶端：狀態快照與變更）。兩者都衍生 `Serialize`/`Deserialize`，因為在常駐程式模式中它們會跨越行程邊界。用戶端絕不直接碰播放器的狀態：一切都經由 `Command`/`Event`
- 兩個 `update` 函式都是純函式且為同步。所有不需要 I/O 就能決定的行為，都在那裡決定，也在那裡做單元測試
- 執行檔的各個 runtime 把終端機事件、socket 訊息、API 結果與引擎事件轉成 input，呼叫 `update`，再執行它回傳的 effect
- 只有單一行程才決定的事，仍留在執行檔中，並採用相同的模式：決定是一個純函式（哪些設定優先、何時儲存、MPRIS 顯示什麼、要刪除哪些封面檔案），它所需的 I/O 則位於一個附有假實作（fake）的小型 trait 之後。`tidal-player-core` 放的是跨越播放器／用戶端邊界的東西（協定、兩個狀態機、領域型別）；它不是放所有純函式的地方（0026，決策 5）
- 繪製是 `fn render(&State, &mut Frame)`：用戶端狀態的純函式，以 ratatui 的 `TestBackend` 加上 `insta` 快照在固定尺寸下測試

<a id="run-modes"></a>
### 執行模式

一個執行檔、三種模式，每種模式使用相同的播放器程式碼（與 tidalt 相同）：

- **獨立模式**（`tidal-player`，沒有常駐程式在執行）— 播放器 runtime 與 TUI 用戶端在同一個行程中，以傳送 `Command`/`Event` 的記憶體內 channel 相連
- **常駐程式**（`tidal-player daemon`）— 只有播放器 runtime，無畫面，透過本機傳輸接受用戶端連線；設計上作為 systemd 使用者服務執行
- **用戶端**（有常駐程式在執行時的 `tidal-player`）— 只有 TUI 用戶端，透過傳輸連上常駐程式

傳輸（承載 JSON 行的 Unix socket；tidalt 用的是 D-Bus）、連上／中斷連線的行為、多個用戶端、一次性的 CLI 指令（`tidal-player playback next`，如 spotify-player 所有）以及暫停時釋放音訊裝置，都在 [0005](0005-daemon-and-clients.md) 中決定。本規格只固定邊界：`Command`/`Event` 型別，以及用戶端只能透過它們到達播放器的規則。

<a id="concurrency"></a>
### 並行

- 執行檔中使用 `tokio` 多執行緒 runtime，處理 HTTP 與事件迴圈
- 音訊解碼與輸出在 `tidal-player-audio` 擁有的**專用 OS 執行緒**上執行（絕不在非同步 runtime 上），透過指令 channel 控制，並透過事件 channel 回報。`tidal-player-audio` 提供同步 API，且不相依於 tokio

<a id="key-libraries"></a>
### 主要函式庫

| 用途               | crate                                          |
|--------------------|------------------------------------------------|
| TUI                | `ratatui` 0.30 + `crossterm` 0.29              |
| 非同步／HTTP       | `tokio` 1、`reqwest` 0.13（rustls）            |
| 解碼               | `symphonia` 0.6（FLAC、ALAC、AAC、MP4）— 不用 FFmpeg |
| 音訊輸出           | `alsa` 0.12（直接使用 `hw:` 以達成位元完美）   |
| CLI                | `clap` 4（derive）                             |
| 設定／serde        | `serde`、`toml`、`directories`                 |
| 機密資料           | `keyring` 4                                    |
| MPRIS（[0010](0010-mpris.md)，implemented） | `zbus` 5（原生；不使用 `mpris-server`） |
| 錯誤               | 函式庫用 `thiserror`，執行檔用 `anyhow`        |
| 記錄               | 在呼叫處使用 `tracing` 巨集；訂閱者把記錄行保留在緩衝區、狀態目錄中的 `tidal-player.log`，常駐程式還會寫到 stderr，並由記錄頁顯示（見 [0027](0027-logs-page.md)）。使用者必須看到的內容放在播放器的訊息列或 stderr，絕不只放在記錄裡 |
| 測試               | `insta`（快照）、`wiremock`（HTTP 測試資料）、`assert_cmd`（CLI） |

<a id="build--tooling-copied-into-claudemd-when-this-spec-is-approved"></a>
### 建置與工具（本規格核准時複製到 `CLAUDE.md`）

- **格式化**：`cargo fmt --all`（CI：`cargo fmt --all --check`）
- **Lint**：`cargo clippy --workspace --all-targets --all-features -- -D warnings`，再加上 `cargo xtask layering` 與 `cargo xtask specs`（0026）
- **建置**：`cargo build --workspace`
- **測試**：`cargo test --workspace`；單一 crate：`cargo test -p tidal-player-core`；單一測試：`cargo test -p tidal-player-core <name>`。快照的變更要經過審閱，絕不盲目接受（`cargo insta review`，或檢查 `.snap.new` 檔案）
- **系統相依套件**：`pkg-config`、`libasound2-dev`（僅 `alsa` feature 需要）。`cargo test -p tidal-player-audio --no-default-features` 不需要它們就能建置

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — 在安裝了系統相依套件的乾淨 checkout 上，`cargo build --workspace` 成功，並產生 `tidal-player` 執行檔
- **AC2** — `cargo fmt --all --check`、上述 clippy 指令與 `cargo test --workspace` 全部通過
- **AC3** — `cargo xtask layering` 在骨架上結束碼為 0；而當表格以外的任何邊被加入，或 `tidal-player-core` 以一般（非 dev）相依的形式（不論直接或遞移）取得 `tokio`、`reqwest`、`crossterm`、`ratatui`、`alsa`、`symphonia`、`zbus`、`keyring`、`serde_json` 任一個時，結束碼為非 0，並指出每一條違規的邊
- **AC4** — `tidal-player --version` 印出 `tidal-player <version>` 並以 0 結束；`tidal-player --help` 以 0 結束（以 `assert_cmd` 測試）
- **AC5** — `tidal_player_core::ui` 公開 `State`、`Action`、`Effect` 與 `update`；`update(&mut State::default(), Action::Quit)` 回傳 `[Effect::Quit]`，而在預設狀態下任何其他 action 都不回傳 effect（單元測試）
- **AC6** — 把 `State::default()` 的 `render` 結果繪製到 80×24 的 `TestBackend`，與已提交的 `insta` 快照相符，快照顯示帶有應用程式名稱的空版面
- **AC7** — `tidal-player-audio` 定義一個 `Sink` trait 與一個記憶體內的測試 sink；ALSA 後端位於預設開啟的 `alsa` feature 之後，且 `cargo test -p tidal-player-audio --no-default-features` 在沒有 `libasound2-dev` 的情況下通過
- **AC8** — `tidal-player-api` 的基底 URL 由呼叫端提供，它的測試以 `wiremock` 提供 `crates/api/tests/fixtures/` 中的 JSON 測試資料，並斷言請求路徑與解析結果。沒有任何測試碰到真正的網路。（骨架自己的 `Client` 與其測試已由 0026 移除：每個請求都經由 `auth::Authenticator`）
- **AC9** — `.github/workflows/ci.yml` 在 `ubuntu-latest` 上執行 AC2 的檢查與 `cargo xtask layering`，安裝 `libasound2-dev`，每個 commit 執行一次：在每次推送到 `master`，以及來自 `feat/*`、`bugfix/*` 與 `refactor/*` 分支的 pull request 時（0026）。推送到任何其他分支或 tag 不會執行任何東西，來自 `docs/*` 與 `chore/*` 的 pull request 會略過這些 job（發行版等待的是 `master` 的執行結果，而不是 tag 的，spec 0018 Bug 3）
- **AC10** — TUI 執行期間發生 panic 時，會在印出 panic 訊息**之前**還原終端機（關閉 raw mode、離開 alternate screen），做法是由執行檔安裝的 panic hook
- **AC11** — `tidal_player_core::protocol` 定義衍生 `Serialize`/`Deserialize` 的 `Command` 與 `Event`，至少包含 `Command::Shutdown` 與 `Event::ShuttingDown`；每個變體經過 JSON 往返後都保持不變，且 `tidal-player-core` 不相依於任何傳輸 crate（由 AC3 的禁用清單涵蓋）
- **AC12** — `CLAUDE.md` 的「Status」與「Build & tooling」依本規格更新，且規格的狀態改為 `implemented`

<a id="test-plan"></a>
## 測試計畫

每個自動化測試都以它所證明的條件命名（`ac5_…`）。「Red」是實作者在寫程式碼之前必須展示的失敗：是失敗的斷言，絕不是編譯錯誤。因此 red commit 已包含測試所呼叫的 stub 型別與函式，其本體為 stub（不允許使用 `todo!()`：它會 panic，而不是讓斷言失敗）。

| AC | 測試（file :: name） | 斷言內容 | 預期的 red |
|----|---------------------|-----------------|--------------|
| AC1 | — command check | `cargo build --workspace` 在 CI 中以 0 結束，且 AC4 的測試會執行建出來的 `tidal-player` 執行檔 | — |
| AC2 | — command check | `cargo fmt --all --check`、clippy（`-D warnings`）與 `cargo test --workspace` 以 0 結束 | — |
| AC3 | `xtask/src/layering.rs` :: `ac3_rules`（表格驅動，針對手工建構的相依圖） | 允許的圖 → 無違規；`core → tokio` → 違規 `tidal-player-core -> tokio`；`core → serde_x → tokio`（遞移）→ 違規；`audio → core`、`api → audio`、`core → api` → 各一條違規，並指出該邊；`core` 僅以 dev 相依取得 `tokio` → 無違規 | stub `check()` 不回傳任何違規，所以每個違規的列都失敗 |
| AC3 | `xtask/tests/workspace.rs` :: `ac3_real_workspace_is_clean` | 在真實 workspace 上執行 `cargo xtask layering` 以 0 結束 | stub 指令以 1 結束 |
| AC4 | `crates/app/tests/cli.rs` :: `ac4_version` | `tidal-player --version` 的 stdout 恰為 `tidal-player <CARGO_PKG_VERSION>\n`，結束碼 0 | stub `main` 什麼都不印 |
| AC4 | `crates/app/tests/cli.rs` :: `ac4_help` | `tidal-player --help` 以 0 結束，且 stdout 含有 `Usage:` | stub `main` 什麼都不印 |
| AC5 | `crates/core/src/ui/mod.rs` :: `ac5_quit_emits_quit` | `update(&mut State::default(), Action::Quit) == vec![Effect::Quit]` | stub `update` 回傳 `vec![]` |
| AC5 | `crates/core/src/ui/mod.rs` :: `ac5_other_actions_emit_nothing`（表格涵蓋每個非 `Quit` 的變體，至少含 `Action::Tick`） | 每個都回傳 `vec![]`，且 `State` 仍等於預設值 | 在 `ac5_quit_emits_quit` 轉綠之後的第二輪 red/green：讓第一個測試通過的最簡單程式碼（對每個 action 都回傳 `vec![Effect::Quit]`）會讓這個測試失敗 |
| AC6 | `crates/app/src/tui/render.rs` :: `ac6_empty_state_80x24` | 80×24 `TestBackend` 緩衝區含有 `tidal-player`，**而且**與已提交的 `insta` 快照相符。tech-lead 在驗收時以肉眼審閱快照 | stub `render` 什麼都不畫，所以 `contains` 斷言失敗（只是缺少快照不算 red） |
| AC7 | `crates/audio/src/sink.rs` :: `ac7_memory_sink_records_samples` | 在 `open(format)` 與兩次 `write` 之後，`MemorySink` 回報該格式，以及依序串接的兩個緩衝區 | stub `write` 是空操作 |
| AC7 | — CI job `audio-no-alsa` | `cargo test -p tidal-player-audio --no-default-features` 在**沒有** `libasound2-dev` 的 runner 上通過 | — |
| AC8 | `crates/api/tests/metadata.rs` :: `ac17_get_track`（0004 的；骨架的 `ac8_get_json_from_fixture` 已隨它的 `Client` 一併移除，0026） | `wiremock` 在曲目的路徑上提供錄製好的測試資料；以 mock 伺服器 URL 建立的用戶端回傳解析後的 `Track` | —（骨架的 red 是 stub `get_json` 回傳錯誤） |
| AC9 | — 驗收時審閱；觸發條件與 job 的 `if` 由 spec 0018 AC8 測試（`xtask/tests/release_workflow.rs` :: `ac8_ci_runs_once_per_commit`） | workflow 檔案含有 AC2 的步驟、`cargo xtask layering` 與 `audio-no-alsa` job，且 PR 自己的 CI 執行是綠的 | — |
| AC10 | `crates/app/tests/panic_hook.rs` :: `ac10_restore_runs_before_report`（自己的測試執行檔，因為 panic hook 是行程全域的） | 以一個假的 `restore` 與一個假的前一個 hook（各自附加到共用記錄），`catch_unwind(\|\| panic!())` 之後，記錄為 `["restore", "report"]` | stub `install_panic_hook` 什麼都不做，所以只有假的前一個 hook 會執行，記錄為 `["report"]` |
| AC11 | `crates/core/src/protocol.rs` :: `ac11_round_trip`（表格涵蓋每個 `Command` 與 `Event` 變體；`serde_json` 僅作為 dev 相依） | 每個都滿足 `from_str(&to_string(&v)) == v` | stub `Serialize` 實作寫出 `null`（手寫，在 green 時以 `#[derive]` 取代），所以反序列化回來會失敗 |
| AC12 | — 驗收時審閱 | `CLAUDE.md` 與本規格相符；狀態為 `implemented` | — |

刻意不由自動化測試涵蓋的：真正的終端機 raw mode 處理與真正的 ALSA 輸出。它們位於上述的 `restore` 與 `Sink` 接縫（seam）之後，只有當之後的規格加入依賴它們的行為時，才會以手動方式演練。

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

- 沒有 `libasound2-dev` 的機器，仍必須能執行 ALSA 後端以外的一切（AC7）
- TUI 在 raw mode 中發生 panic（AC10）

<a id="out-of-scope"></a>
## 不在範圍內

使用者所做的一切。本規格原本規劃的後續規格（auth、播放引擎、佇列、常駐程式、音樂庫、搜尋、按鍵對應與設定、持久化、MPRIS、mix 與電台）已作為 0002–0011 實作；`CLAUDE.md` 的「Features」列出之後的每份規格。

<a id="facts-vs-assumptions"></a>
## 事實與假設

2026-10-06 已驗證：

- 上列的 crate 版本都是最新發行的（`cargo search`）
- 開發容器有 Rust 1.97.0；它**沒有**安裝 `libasound2-dev`

假設，在依賴它的規格中驗證：

- `symphonia` 0.6 能解碼 Tidal 提供給我們的每一種格式 — 特別是 `HI_RES_LOSSLESS` DASH manifest 中位於分段 MP4 內的 FLAC。若不能，由 0003 在 symphonia 修補與 feature 之後的選用 FFmpeg 後端之間決定
- Linux 上的 `keyring` 4 不需原生函式庫就能透過 D-Bus 連到 Secret Service，且在沒有 Secret Service 執行時需要檔案後備方案（tidalt 用的是以 age 加密的檔案）— 在 0002 中決定

<a id="bugs"></a>
## 錯誤

- **測試計畫錯誤，AC10 的 red（驗收時發現）。** 計畫寫的是 stub hook 會讓記錄為 `[]`。實際上假的前一個 hook 仍會執行，所以記錄是 `["report"]`。測試是對的；計畫的該列已更正。
