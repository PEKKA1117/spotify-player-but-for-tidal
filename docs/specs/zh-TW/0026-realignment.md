[English](../0026-realignment.md) | 正體中文

<a id="0026--realignment-the-code-the-specs-and-the-rules-say-the-same-thing"></a>
# 0026 — 重新對齊：程式碼、規格與規則說的是同一件事

- **狀態**：implemented（2026-11-10；同一天核准：使用者要求審查整個專案，並准許重新檢討任何規格或決策）
- **負責人**：tech-lead（主要工作階段）
- **相依於**：0001（implemented：各 crate 與它們的規則）、0018（implemented：CI 與發行）
- **使用者文件**：無（使用者看到的東西都沒有改變）

<a id="context"></a>
## 背景

五週內實作了二十五份規格。每一份都對照它自己的文字檢查過，但沒有任何東西拿整個專案對照它自己檢查，於是它在四個方面漂移了。2026-11-10 的稽核（每個 crate、每份規格、`CLAUDE.md`）發現：

- **二進位 crate 沒有形狀。** `crates/app/src` 是 23 個扁平的模組，以新增它們的規格命名：`play`（每種模式的設定，以及 `play` 指令）、`playback`（引擎的串接）、`player_runtime`、`oneshot`（`playback` 指令）、`cover`（TUI 的圖片）與 `covers`（播放器的快取）並存、`reserve` 與音訊 crate 的 `reserve` 並存。規格 0001 把程式分成播放器端與客戶端；目錄卻沒有
- **測試掩埋了程式碼。** `ui.rs` 是一個檔案，含 861 行繪製與 4,657 行測試；`player_runtime.rs` 是 1,860 與 1,910。`tidal-player-core` 早已把長的測試模組放在 `<module>/tests.rs`；其他 crate 沒有
- **規格指向已經搬走的東西。** 測試計畫點名了已不存在的檔案（`crates/core/src/ui.rs`、`crates/app/src/mpris/covers.rs`）以及已被改名的測試。「規格與程式碼一致」只在驗收時以肉眼檢查，之後就再也沒有檢查過
- **規格 0001 描述了一個從未建成的程式。** 它承諾透過 `tracing-appender` 寫記錄檔（從未安裝任何 subscriber：37 個 `tracing` 呼叫什麼都沒寫，0010 與 0025 各自重新發現了這件事），說 ALSA 後端以外的東西都不能是 Linux 專屬的（socket、鎖、systemd 與 MPRIS 都是），並說二進位檔的執行階段含有「沒有值得測試的決策」（二進位檔含有約 10,000 行在接縫之後、經過測試的決策）
- **遺留物。** `tidal_player_api::Client` 與 `ApiError` 是 0001 的鷹架，只被一個測它們自己的測試使用；`browse.rs` 以 `#[path]` 掛載；重構沒有分支種類，所以不可能不變成發行版就執行 CI

這些都不是使用者看得到的錯誤。但這一切都讓下一份規格寫起來更慢，也更容易出錯。

<a id="behaviour"></a>
## 行為

對使用者來說沒有任何改變：沒有按鍵、設定、訊息、檔案或結束碼的變動。之前通過的每個測試，之後都以原本的名稱通過。

<a id="the-binarys-layout"></a>
### 二進位檔的配置

`crates/app/src` 依循 0001 的分法：

```
main.rs          the command line and which mode runs
settings.rs      flags > environment > app.toml > defaults, resolved once
config.rs        the files in the config directory (app.toml, keymap.toml, theme.toml)
panic_hook.rs
player/          the player side: only a process that plays uses it
  runtime.rs       runs core's player::update and executes its effects
  playback.rs      the stream opener and the ALSA engine with its reservation
  http_source.rs   a resolved stream as bytes for the engine
  persist.rs       the remembered playback state (0009)
  cover_cache.rs   the cover cache (0010, 0016)
  hook.rs          the event hook's process side (0025)
  reserve.rs       org.freedesktop.ReserveDevice1 (0003)
  mpris/           the MPRIS adapter (0010)
client.rs        a client's connection to a player, in-process or over the socket
ipc/             the socket: paths, lock, framing, server, client
tui/             the terminal client
  input.rs         key presses to the UI model's actions
  render.rs        the UI model's state to a frame (render/pages.rs, render/theme.rs)
  cover.rs         the album cover's loader and slot (0016)
cli/             the subcommands
  play.rs          `play`, `devices`
  oneshot.rs       `playback …`
  playlist.rs      `playlist …`
  daemon.rs        `daemon`
session/         where the login session is kept (0002)
  store.rs, passphrase.rs, setup.rs, login.rs
```

模組放在它的使用者所在之處：只有播放器會用的東西在 `player/` 之下，只有 TUI 會用的東西在 `tui/` 之下。`client` 與 `ipc` 留在最上層，因為 TUI、一次性指令與行程內的播放器都會用到它們。

<a id="test-modules"></a>
### 測試模組

長於 200 行的 `mod tests` 放在 `<module>/tests.rs`（對 `mod.rs` 而言，是它旁邊的 `tests.rs`），每個 crate 都一樣。較短的留在原處。`main.rs` 的測試不論長短都留在原處（二進位檔根目錄的 `mod tests;` 會是 `src/tests.rs`，離程式碼很遠）。

<a id="cargo-xtask-specs"></a>
### `cargo xtask specs`

對照程式碼檢查每份規格的測試計畫，並在 CI 中與 `cargo xtask layering` 並列執行：

- `docs/specs/*.md` 中，以反引號點名本工作區 Rust 檔案（`` `crates/…​.rs` `` 或 `` `xtask/…​.rs` ``）的表格列，所點名的檔案必須存在
- 檔案之後接著 `::` 與反引號中的測試名稱時，每個名稱都必須是該檔案或它的測試模組（`<file stem>/tests.rs`；對 `mod.rs` 而言是它旁邊的 `tests.rs`）中的函式。所以計畫可以點名模組，並在它的測試搬到 `tests.rs` 時仍然有效
- 括號中的文字是備註，不檢查（`(renamed from …)`、`(0007's, …)`）
- 失敗時每一項印出 `docs/specs/NNNN-….md:LINE: <what is missing>`，且指令以 1 結束；乾淨的執行印出 `specs ok`

它讀的是現在已寫好的測試計畫；沒有任何規格需要為它改變格式。

<a id="branch-kinds"></a>
### 分支種類

`refactor` 加入 `feat`、`bugfix`、`docs` 與 `chore`：對 Rust 程式碼的變更，但不改變行為。來自 `refactor/*` 的 pull request 與 `feat/*` 和 `bugfix/*` 一樣執行 CI 工作，並且**不是**發行版：不升版本，不打標籤。它動到的 crate 隨下一次 `feat/*` 或 `bugfix/*` 發行版一起發布，而該發行版的升版已涵蓋「自上次發行版以來程式碼有變動的 crate」。

<a id="acceptance-criteria"></a>
## 驗收條件

- **AC1** — 當每個被引用的檔案與測試都存在時，`cargo xtask specs` 以 0 結束並印出 `specs ok`；對每個缺少的檔案與每個缺少的測試，它印出點名它的 `<spec path>:<line>: …`，並以 1 結束
- **AC2** — `::` 之後的測試名稱，在被點名的檔案或該檔案的測試模組中找得到；括號中的名稱不檢查；沒有 `::` 的工作區路徑只檢查是否存在；表格以外的列，以及 `crates/` 與 `xtask/` 以外的路徑都被忽略
- **AC3** — `cargo xtask specs` 在真正的工作區上以 0 結束：規格 0001–0026 的每份測試計畫所點名的檔案與測試都存在
- **AC4** — CI 在 `check` 工作中執行 `cargo xtask specs`，且來自 `refactor/*` 的 pull request 會執行這些工作：每個工作的 `if` 是 `github.event_name == 'push' || startsWith(github.head_ref, 'feat/') || startsWith(github.head_ref, 'bugfix/') || startsWith(github.head_ref, 'refactor/')`
- **AC5** — `crates/app/src` 具有上述配置，而 `lib.rs` 恰好宣告 `cli`、`client`、`config`、`ipc`、`panic_hook`、`player`、`session`、`settings`、`tui`
- **AC6** — `main.rs` 以外沒有任何 `mod tests { … }` 長於 200 行
- **AC7** — `tidal-player-api` 不匯出 `Client` 或 `ApiError`，且工作區中沒有任何模組以 `#[path]` 掛載
- **AC8** — 本規格之前就存在的每個測試，之後都以相同的名稱存在並通過；`cargo test --workspace` 執行相同數量的測試，減去測試已移除鷹架的那兩個（`ac8_get_json_from_fixture`、`url_has_exactly_one_slash`）
- **AC9** — 規格 0001 描述現存的程式（配置、平台規則、二進位檔的角色、記錄、AC8），而 `CLAUDE.md` 描述實際使用的規格格式、分支種類與 `cargo xtask specs`

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

- **規格點名的測試位在它所點名檔案以外的另一個檔案，或用的是較晚的名稱**（2026-11-10 發現：0006 的頁面測試已搬到 `ui/browse/tests.rs`，0010 的 `ac18_cover` 是整合測試，0004 與 0012 點名了後來被其他規格改名的測試）：更正計畫；不放寬檢查去搜尋整個工作區，因為那樣計畫就可以點名任何檔案
- **沒有測試的條件的測試計畫列**（`— reviewed at acceptance`）：沒有點名任何檔案，所以什麼都不檢查
- **規格引用另一個專案的路徑**（spotify-player 的 `docs/client-server.md`）或刻意刪除的探測腳本（`scripts/tidal-*-probe.sh`）：不在 `crates/` 或 `xtask/` 之下，不檢查
- **確實改變了行為的 `refactor/*` pull request**：它是 `feat` 或 `bugfix`，並被改名；種類是審閱者檢查的承諾，與 `docs/*` 相同

<a id="test-plan"></a>
## 測試計畫

| AC | 測試（檔案 :: 名稱） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `xtask/src/specs.rs` :: `ac1_reports_missing_files_and_tests`（表格） | 乾淨的計畫沒有發現；缺少的檔案、缺少的測試，以及同一列中的兩個，各給出一項帶有規格行號與名稱的發現 | 存根 `check` 不回傳任何發現 |
| AC2 | `xtask/src/specs.rs` :: `ac2_what_is_read`（表格） | 測試模組的備用位置（`foo.rs` → `foo/tests.rs`、`mod.rs` → `tests.rs`）；括號中的名稱被略過；沒有 `::` 的路徑只檢查是否存在；文字敘述與外部路徑被忽略；一個儲存格中有多個檔案時，各自擁有它之後的名稱 | 存根 `check` 不回傳任何發現，所以預期有發現的列失敗 |
| AC3 | `xtask/tests/workspace.rs` :: `ac3_specs_agree_with_the_code` | `cargo xtask specs` 在真正的工作區上以 0 結束 | 存根指令以 1 結束 |
| AC4 | `xtask/tests/release_workflow.rs` :: `ac8_ci_runs_once_per_commit`（0018 的，`if` 已改變）、`ac4_ci_checks_the_specs` | 每個工作的 `if` 是上面那一行；`ci.yml` 有一個執行 `cargo xtask specs` 的步驟 | 工作流程兩者都沒有 |
| AC5 | — 驗收時檢視 | 以配置檢視 `ls crates/app/src` 與 `lib.rs` | —（這是搬移；編譯器與 AC8 會檢查） |
| AC6 | `xtask/tests/workspace.rs` :: `ac6_long_test_modules_have_their_own_file` | `main.rs` 以外，`crates/` 或 `xtask/` 之下沒有任何 `.rs` 檔案有超過 200 行的內嵌 `mod tests {` 區塊 | 在搬移前的樹上失敗（驗收時在 `origin/master` 上檢查） |
| AC7 | `xtask/tests/workspace.rs` :: `ac7_no_path_mounted_modules` | 工作區的原始碼中沒有 `#[path` 屬性 | 在 `origin/master` 上失敗（`crates/api/src/library.rs`） |
| AC8 | — 指令檢查 | 之前與之後的 `cargo test --workspace -- --list` 測試名稱清單，只有模組路徑不同，減去被移除的那兩個 | — |
| AC9 | — 驗收時檢視 | 以程式碼檢視 0001 與 `CLAUDE.md` | — |

AC5–AC7 是在能先寫它們的測試之前就做好的搬移（搬移本身沒有紅燈狀態：搬移之前的樹就是紅燈狀態）。它們的測試改為在驗收時對 `origin/master` 檢查。

<a id="crate-placement"></a>
## Crate 配置

- `xtask/src/specs.rs`：檢查，對 `(spec path, text)` 與一個讀取工作區檔案的函式為純函式；`main.rs` 新增 `specs` 指令
- 其他一切都是在 crate 內部的搬移；沒有 crate 增加或減少相依套件

<a id="facts-vs-assumptions"></a>
## 事實與假設

2026-11-10 已驗證：

- `cargo test --workspace`、帶 `-D warnings` 的 clippy 與 `cargo xtask layering`，在任何變更之前於 `origin/master`（7770837）上都是乾淨的
- 沒有安裝任何 `tracing` subscriber：`Cargo.lock` 中沒有 `tracing-subscriber`
- `tidal_player_api::Client` 只被 `crates/api/tests/harness.rs` 與它自己的單元測試使用
- `insta` 以測試的模組路徑為快照命名，並把它放在測試檔案旁邊的 `snapshots/` 中，所以搬移的測試需要連同它的快照一起搬移並改名；它們的內容不變

<a id="decisions"></a>
## 決策

1. **規格 0001 在程式超出它的地方被更正，而不是被遵守。** 它的平台規則變成「只支援 Linux；ALSA 後端是唯一需要 ALSA 的程式碼」，而它對二進位檔的說法變成程式碼所遵循的規則：決策是純函式，I/O 放在接縫之後
2. **記錄仍然沒做，而 0001 現在這麼說。** 安裝 subscriber 是一個功能（檔案在哪裡、客戶端顯示什麼），必須由 issue #64（記錄頁）來設計；0001 不再承諾它
3. **`cargo xtask specs` 對測試模組寬鬆，對檔案嚴格。** 針對 `foo.rs` 寫的計畫，在它的測試搬到 `foo/tests.rs` 後仍然有效；點名錯誤檔案的計畫是錯誤
4. **`refactor/*` 執行 CI，而且不是發行版。** 替代方案（`chore/*`）會略過 CI，而 20,000 行的搬移不能這樣做；另一個（`feat/*`）會為沒有使用者可見變更的東西發布五個 crate
5. **純決策留在二進位 crate。** 把 MPRIS 模型、設定與儲存排程搬進 `tidal-player-core`，會讓 core 的規則（「沒有 I/O」）在紙面上更真實，卻讓它的 API 變大而沒有人受益：除了二進位檔之外沒有東西使用它們。`core` 保留跨越播放器／客戶端邊界的東西

<a id="out-of-scope"></a>
## 不在範圍內

- 記錄檔或 `tracing` subscriber（issue #64）
- 進一步拆分 `player/runtime.rs`（1,860 行）與 `core::ui`：它們的大小各是一個狀態機，而它們內部沒有任何邊界像這裡搬移的那些一樣清楚
- 二進位 crate 單元測試套件中的慢速測試（約 130 秒，大部分是 hook 與封面測試中真實的 sleep）
- 重新命名使用者或腳本看得到的任何東西
