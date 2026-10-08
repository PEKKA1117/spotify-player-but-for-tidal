[English](../playback.md) | 繁體中文

<a id="playing-tracks"></a>
# 播放曲目

`tidal-player` 自行解碼 Tidal 的串流（FLAC 與 AAC，以純 Rust 實作），並寫入 ALSA 裝置：預設經由系統混音器，或在你要求時直接送到你的 DAC，位元完美（bit-perfect）。設計：[spec 0003](../specs/0003-playback-engine.md)（播放引擎）與 [spec 0004](../specs/0004-queue-and-controls.md)（佇列與控制）。

三種播放方式：TUI（`tidal-player [ITEM]...`，見 [TUI](tui.md)）、從命令列以無介面方式播放（`tidal-player play <ITEM>...`，見下文），或常駐程式（daemon）（`tidal-player daemon`，以 `tidal-player playback …` 及連接上的 TUI 控制，見 [常駐程式與用戶端](daemon.md)）。三者都播放一個由[項目](#items)填入的**佇列**。

<a id="items"></a>
## 項目

**項目**（item）是你交給播放器加入佇列的東西：曲目 ID 或 Tidal 連結，就像 Tidal 應用程式的分享選單所複製的那樣。

| 項目 | 加入佇列的內容 |
|---|---|
| `77640617` | 曲目 77640617 |
| `https://tidal.com/browse/track/77640617`、`https://tidal.com/track/77640617`、`https://listen.tidal.com/track/77640617`、`tidal://track/77640617`（有無 `www.`、分享選單的 `/u` 後綴、結尾的 `/` 或查詢字串皆可） | 曲目 77640617 |
| `https://tidal.com/browse/album/123`、`https://tidal.com/album/123/u` | 專輯 123 的每一首曲目，依專輯順序 |
| `https://tidal.com/browse/playlist/<uuid>`、`…/playlist/<uuid>/u` | 播放清單的每一首曲目，依播放清單順序；音樂錄影帶會被略過 |

其他任何東西（藝人、合輯（mix）或影片連結、其他網站、打錯字）都會在開始播放前被拒絕，並顯示 `Not a Tidal track, album or playlist: <item>`（結束代碼 2）。多個項目會依給定順序一個接一個加入佇列。專輯與播放清單無論多長，都會在播放開始前完整取得。

Tidal 不提供串流的曲目仍會加入佇列並顯示，但會被略過（`Track 123 is not available in NO`）。

<a id="commands"></a>
## 指令

<a id="tidal-player---add-to-queue----play-next-item"></a>
### `tidal-player [--add-to-queue | --play-next] [ITEM]...`

啟動 TUI。給了項目時，佇列會載入這些項目並播放第一個；沒有給時，佇列一開始是空的（用 `o` 加入，見 [TUI](tui.md#adding-tracks)）。若某個項目無法取得（`Album 123 was not found`、網路錯誤）或沒有曲目，佇列維持原狀，訊息會顯示在播放視窗中。

`--add-to-queue` 把項目加到佇列結尾，`--play-next` 則加在目前曲目之後，而不是取代佇列；若原本沒有在播放，第一個加入的曲目會開始播放。若已有播放器在執行（常駐程式或另一個 TUI），TUI 會連接到它，項目會進入該播放器的佇列：見[連接 TUI](daemon.md#attaching-a-tui)。兩者不能同時使用，且都至少需要一個項目（結束代碼 2）。

<a id="tidal-player-play-item---shuffle---repeat-offqueuetrack---autoplay---quality-q---device-pcm---start-seconds"></a>
### `tidal-player play <ITEM>... [--shuffle] [--repeat off|queue|track] [--autoplay] [--quality Q] [--device PCM] [--start SECONDS]`

在前景把項目當作一個佇列播放，不使用 TUI，佇列結束時退出。播放期間可用 [`tidal-player playback …`](daemon.md#one-shot-commands) 控制它。若有另一個播放器在執行，它以結束代碼 3 退出（`Another player is running (pid N): use "tidal-player playback load"`）。它需要已儲存的工作階段（見[登入](login.md)）；沒有的話以結束代碼 1 退出，並顯示 `Not logged in: run "tidal-player login"`。

```
$ tidal-player play 77640617 --device hw:1,0
Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo
Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect
  1:23 / 4:56
```

- **Track** 行：Tidal **核發**的音質（可能低於你要求的：Tidal 依曲目與帳號決定），以及解碼器偵測到的格式
- **Output** 行：實際開啟的裝置、其類型（見下文）、運作時的取樣格式與取樣率，接著是 `bit-perfect` 或不是位元完美的原因
- 進度行會原地重繪，且只在 stdout 為終端機時才顯示（把輸出導向管線時不會印出任何東西）。串流未提供長度時，長度顯示為 `?:??`
- 每當一首曲目開始，Track 與 Output 行會再印一次；進度行跟隨目前曲目
- `--start 90` 讓第一首曲目從第 90 秒開始
- `--shuffle` 隨機播放佇列（第一個項目仍最先播放）；`--repeat queue` 在結尾時從頭開始，`--repeat track` 重複播放每一首曲目。見[隨機播放與重複播放](tui.md#shuffle-repeat-and-autoplay)
- `--autoplay`（或 `TIDAL_PLAYER_AUTOPLAY=on`）在佇列播完後以 Tidal 建議的曲目繼續播放，直到按下 Ctrl-C

無法播放的曲目（在你的國家無法取得、無法解碼）會被略過，訊息輸出到 stderr；連續 5 首（或整個佇列，若較短）之後播放就會停止。網路、`429`/`5xx`、輸出或工作階段錯誤會立即停止，絕不略過（見[失敗](tui.md#failures)）。

結束代碼：`0` 佇列播完且至少有一首曲目播到結尾；`1` 因錯誤停止，或沒有任何曲目可播放（stderr 上一行訊息，見[錯誤](#errors)）；`2` 參數、設定或項目錯誤；`130` Ctrl-C（會先關閉並釋放裝置）。只給一個曲目 ID 且不使用任何新旗標時，`play` 的行為與佇列出現之前完全相同。

<a id="tidal-player-devices"></a>
### `tidal-player devices`

列出 `--device` 接受的裝置：先是 `default`，接著是每個能播放的 ALSA 硬體裝置（`hw:CARD,DEVICE`），附上音效卡名稱。`play` 會使用的裝置以 `*` 標示：

```
$ tidal-player devices
* default  shared, through the system mixer
  hw:0,0   HDA Intel PCH: ALC892 Analog
  hw:0,1   HDA Intel PCH: ALC892 Digital
  hw:1,0   E30 II: USB Audio
```

它讀取 `/proc/asound/cards` 與 `/proc/asound/pcm`；只能錄音的裝置（麥克風）不會列出。

<a id="settings"></a>
## 設定

每項設定都可以在 [`app.toml`](config.md#apptoml)（位於[設定資料夾](config.md#where-the-files-are)，預設為 `~/.config/tidal-player`）中設定，也可以用環境變數設定，部分還可以用旗標設定。

| 設定 | `app.toml` 鍵 | 旗標 | 環境變數 | 預設值 |
|---|---|---|---|---|
| 要求的最高音質 | `quality` | `--quality`（`hi-res`、`lossless`、`high`） | `TIDAL_PLAYER_QUALITY` | `hi-res` |
| 輸出裝置 | `output_device` | `--device`（任何 ALSA PCM 名稱） | `TIDAL_PLAYER_DEVICE` | `default` |
| TUI 中 `+`/`-` 的音量級距（%） | `volume_step` | | `TIDAL_PLAYER_VOLUME_STEP`（1–25） | `5` |
| TUI 中 `>`/`<` 的跳轉級距（秒） | `seek_duration_secs` | | `TIDAL_PLAYER_SEEK_STEP`（1–600） | `5` |
| 超過此秒數後「上一首」改為重新播放目前曲目；`0`：「上一首」一律回到上一首 | `previous_restart_secs` | | `TIDAL_PLAYER_PREVIOUS_RESTART`（0–60） | `3` |
| 啟動時開啟自動播放 | `autoplay`（`true`、`false`） | `--autoplay`（僅 `play`） | `TIDAL_PLAYER_AUTOPLAY`（`on`、`off`） | `off` |
| 暫停多久（秒）後釋放裝置；見[暫停時釋放裝置](daemon.md#releasing-the-device-while-paused) | `release_paused_secs`（或 `"never"`） | | `TIDAL_PLAYER_RELEASE_PAUSED`（0–3600，或 `never`） | `10` |
| TUI 中音樂庫清單每次取得的列數；見[清單隨捲動載入](tui.md#lists-load-as-you-scroll) | `page_size` | | `TIDAL_PLAYER_PAGE_SIZE`（1–10 000） | `100` |
| TUI 中搜尋結果每次取得的列數；見[搜尋](tui.md#search) | `search_page_size` | | `TIDAL_PLAYER_SEARCH_PAGE_SIZE`（1–1000） | `20` |
| 在藝人的 *All tracks*（所有曲目）中隱藏其他版本的關鍵字（環境變數中以逗號分隔，檔案中為陣列；空值表示不隱藏任何東西）；見[藝人的 All tracks](tui.md#the-artists-all-tracks) | `hide_versions` | | `TIDAL_PLAYER_HIDE_VERSIONS` | `instrumental, inst, off vocal, karaoke, tv version, tv ver, tv size, tv edit, sped up, speed up, nightcore, slowed, slowed + reverb, reverb, 8d, 8d audio` |

每項設定的優先順序為：旗標優先於環境變數，環境變數優先於 `app.toml`，`app.toml` 優先於預設值。空的環境變數視為未設定（`TIDAL_PLAYER_HIDE_VERSIONS` 除外，空值表示不隱藏任何東西），因此會套用檔案中的值。無效的值會在任何東西啟動前以結束代碼 2 退出，並指出該環境變數（或檔案與鍵）以及它接受的值（`invalid TIDAL_PLAYER_SEEK_STEP: expected an integer from 1 to 600, got "0"`、`…/app.toml: invalid seek_duration_secs: expected an integer from 1 to 600, got 0`）；即使檔案有效，無效的環境變數仍是錯誤。未知的音質以結束代碼 2 退出。`low` 也會被拒絕：Tidal 的 `LOW` 串流是 HE-AAC，播放器無法解碼；`high`（AAC 320 kbit/s）是最低的設定。檔案的語法與錯誤見[設定](config.md)。

音質是要求的**最高**音質。Tidal 會依曲目與你的訂閱方案所允許的給出回應：以 `hi-res` 要求的 CD 音質曲目會以 `LOSSLESS`（FLAC 16-bit 44.1 kHz）提供，有些曲目則只有 `HIGH`。

要把 DAC 設為預設，在 `~/.config/tidal-player/app.toml` 中加上 `output_device = "hw:1,0"`（或在 shell 設定檔中加上 `export TIDAL_PLAYER_DEVICE=hw:1,0`）。

<a id="output-kinds-and-bit-perfect"></a>
## 輸出類型與位元完美

| `--device` | 類型 | 會發生什麼 |
|---|---|---|
| `hw:C,D`（或 `hw:C`） | **exclusive**（獨占） | 直接開啟音效卡。播放器會先請 PipeWire/PulseAudio 釋放它（`org.freedesktop.ReserveDevice1`），並在曲目播放期間持有它；在此期間其他應用程式無法使用該音效卡。裝置以曲目的精確取樣率運作，不做重新取樣 |
| 拒絕曲目格式的 `hw:C,D` | **fallback**（退回） | 改以 `plughw:C,D` 重新開啟：由 ALSA 轉換取樣率或格式。Output 行會說明原因，例如 `resampled (plughw fallback: device refused S24_3LE/S24_LE/S32_LE at 96 kHz)` |
| `plughw:…` | **fallback**（退回） | 同上，由你選擇 |
| `default` 或任何其他名稱 | **shared**（共用） | 經由系統混音器（PipeWire、PulseAudio、dmix），與其他應用程式並用。永遠不是位元完美 |

**位元完美**表示檔案中的取樣原封不動地送達 DAC。只有在以下條件全部成立時，Output 行才會顯示 `bit-perfect`：

- 類型為 exclusive（`hw:`），
- 曲目為無損（FLAC；AAC 為 `lossy source`），
- 裝置以曲目的取樣率並以立體聲運作（單聲道曲目會以相同的左右聲道送出，仍維持位元完美），
- 取樣格式至少能容納曲目的位元數（16-bit 曲目以 `S32_LE` 送出時會補零，仍是位元完美；24-bit 曲目以 `S16_LE` 送出則不是）。

否則會說明原因：`shared (system mixer)`、`lossy source`、`plughw (ALSA may convert the samples)`、`resampled (plughw fallback: …)`、`device runs at 48 kHz, source is 44.1 kHz`、`24-bit source truncated to S16_LE`、……。

要在曲目播放時檢查，請讀取音效卡的硬體參數；格式與取樣率必須與 Output 行相符：

```sh
cat /proc/asound/card1/pcm0p/sub0/hw_params
```

<a id="finding-your-dacs-device"></a>
### 找出你的 DAC 裝置

1. 插上 DAC 並執行 `tidal-player devices`。它會以新的音效卡出現，名稱通常取自該 DAC（`hw:1,0   E30 II: USB Audio`）。大多數 USB DAC 只有一個裝置，`,0`
2. 試試看：`tidal-player play <track-id> --device hw:1,0`。播放 FLAC 曲目時，Output 行應顯示 `(exclusive)` 與 `bit-perfect`
3. 保留設定：`export TIDAL_PLAYER_DEVICE=hw:1,0`

以不同順序插入裝置時，音效卡編號可能改變。ALSA 也接受以音效卡的 ID 取代編號（`hw:CARD=DAC,DEV=0`，ID 是 `/proc/asound/cards` 中方括號內的名稱）；`devices` 清單顯示的是編號。

<a id="errors"></a>
## 錯誤

| 訊息 | 該怎麼做 |
|---|---|
| `Not logged in: run "tidal-player login"` | 先登入（[登入](login.md)） |
| `Session expired: run "tidal-player login"` | Tidal 不再接受已儲存的登入：請重新登入 |
| `Track 123 is only available as a preview for this account` | 你的訂閱方案或國家只能取得這首曲目的 30 秒試聽 |
| `Track 123 is not available in NO` | Tidal 不向你的帳號或國家提供這首曲目 |
| `Track 123 was not found, or cannot be streamed in NO` | 檢查曲目 ID；對於在你的國家未取得授權的曲目，Tidal 也會給出相同的回應 |
| `Track 123 is not playable: …` | 串流的形式是播放器無法解碼的（加密、Dolby Atmos / Sony 360、HE-AAC）。寧可什麼都不播，也不播出雜訊 |
| `Output hw:1,0 is busy (used by …): close it, or use --device default` | 另一個應用程式占用了音效卡，或拒絕釋放它。關閉該應用程式，或以 `--device default` 經由混音器播放。播放器絕不會自行悄悄改用其他裝置 |
| `No such output device hw:5,0: see "tidal-player devices"` | 打錯字或裝置已拔除：從 `tidal-player devices` 中挑選名稱 |
| `Output hw:1,0 was lost` | 播放時 DAC 被拔除（或故障）。裝置及其保留都會被釋放 |
| `Network error while streaming track 123` | 連線中斷，且重試（0.5 秒、1 秒與 2 秒）後仍未恢復。短暫的中斷會由預讀緩衝與重試處理，你不會察覺 |
| `Not a Tidal track, album or playlist: …` | 該項目不是曲目 ID，也不是曲目、專輯或播放清單連結（見[項目](#items)） |
| `Track 1 was not found`、`Album 1 was not found`、`Playlist <uuid> was not found` | Tidal 不認得該 ID：請檢查連結 |
| `Album 1 has no tracks`、`Playlist <uuid> has no tracks` | 專輯或播放清單是空的，或只有影片（`play` 以結束代碼 2 退出） |
| `Stopped: 5 tracks in a row could not be played` | 連續數首曲目無法播放（例如某張專輯在你的國家不提供）；播放會停止，而不是跑完整個佇列 |
| `Autoplay: no suggestions (…)` | Tidal 對最後一首曲目沒有建議，或請求失敗；佇列在結尾處停止 |
| `this build has no ALSA output` | 此執行檔建置時未啟用 `alsa` 功能：以預設功能重新建置 |

串流連結過一段時間就會失效（長時間暫停、很久之後才跳轉）。播放器此時會向 Tidal 要一次新連結，並從原處繼續；你不會察覺任何事。
