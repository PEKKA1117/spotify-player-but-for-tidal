[English](../0003-playback-engine.md) | 正體中文

<a id="0003--playback-engine-stream-resolution-decode-alsa-output"></a>
# 0003 — 播放引擎：串流解析、解碼、ALSA 輸出

- **狀態**：implemented（2026-10-07；「測試計畫」下的手動檢查在使用者的機器上執行）
- **負責人**：tech-lead（主要工作階段）
- **相依於**：0001（implemented）、0002（implemented；AC18 與 AC19 由這項工作加入，見 0002「錯誤」）
- **使用者文件**：[`docs/playback.md`](../../zh-TW/playback.md)（由本規格的實作撰寫，AC27）

<a id="context"></a>
## 背景

本規格讓播放器發出聲音：把一首 Tidal 曲目解析成串流、解碼，再送到 ALSA 裝置，在裝置允許時達到位元完美（bit-perfect）。它為引擎提供之後的規格所建構的基本操作（播放、預先載入下一首、暫停、繼續、跳轉、停止、切換裝置）。它不涵蓋佇列、播放器狀態機、TUI 控制或常駐程式：那些是 0004 與 0005。在它們存在之前，以無畫面的 `tidal-player play <track-id>` 指令驅動引擎，使它能在真實硬體上驗收。

<a id="what-tidalt-did"></a>
### tidalt 的做法

讀自 `internal/tidal/api.go` 與 `internal/player/{mpv.go,alsa.c,avcodec.c,shared.go}`，以及它的 `fix(player)` commit 歷史：

- **解析**：`GET /v1/tracks/{id}/urlpostpaywall?audioquality=Q&urlusagemode=STREAM&assetpresentation=FULL`，依序嘗試 `HI_RES_LOSSLESS → LOSSLESS → HIGH → LOW`，取第一個 `200`。它播放回來的任何單一 URL，並由副檔名猜測格式。依 probe，該端點對這個 client 拒絕 `HI_RES_LOSSLESS`，所以 tidalt 從未取得 hi-res。在大約 2026 年 5 月以前，它在 `LOSSLESS` 取得的是 16/44.1 FLAC（它的純 FLAC 解碼器自 2026-03-13 起播放音樂；2026-05-09 的 commit 0573cda 記載 Tidal 在 `LOSSLESS` 回傳 AAC），之後 Tidal 開始對這個 client ID 的 `LOSSLESS` 以 AAC `HIGH` 回答（python-tidal issue #404，2026-04-27）。從那時起，CD 音質的曲目得到的是 AAC 320
- **解碼**：透過 CGO 使用 FFmpeg（libavformat/libavcodec/libswresample），以自訂的 AVIO 回呼從 HTTP 本文餵入，一律轉換為 S32LE。這就是 spec 0001 移除的建置與封裝負擔
- **輸出**：直接開啟 ALSA `hw:`，依裝置協商格式（16 位元來源：`S32_LE → S16_LE → S24_3LE → S24_LE`；24 位元來源：`S24_3LE → S24_LE → S32_LE`），以 `org.freedesktop.ReserveDevice1` 從 PipeWire 取得音效卡，只有在格式協商被拒絕時才退回 `plughw:`（絕不在裝置忙碌時），並依裝置記憶。「shared」模式（`default` PCM）略過保留並使用較大的緩衝區。緩衝區：1024 frame 的 period，`hw:` 上 4 個 period，shared 時 8 個
- **時間預算**：`RequestRelease` 呼叫 500 ms、穩定等待 200 ms、`EBUSY` 開啟重試 800 ms（每 100 ms 一次）、關閉等待 3 秒

<a id="what-went-wrong-there-and-the-criterion-that-covers-it-here"></a>
### 那裡出了什麼問題，以及這裡涵蓋它的條件

1. **音質階梯把每個錯誤都當成「這個音質不可用」。** `HI_RES_LOSSLESS` 的逾時或 `5xx` 會悄悄地以 `LOW` 播放曲目。而且 `urlpostpaywall` 從不對這個 client 授予 `HI_RES_LOSSLESS`（`401`/`4005`，probe），所以 tidalt 根本無法播放 hi-res → AC1、AC5
2. **中止的播放看起來像曲目結束。** 在修正 #8 之前，曲目中途的錯誤會關閉「done」channel，所以 UI 自動前進到相同的壞狀態，一首接一首 → AC16
3. **兩個播放迴圈可能同時執行**，當舊的在逾時內沒有停止時，它們會爭奪裝置與保留（修正 #8），而暫停 → 跳過的路徑會解參考已關閉的 PCM handle（SIGSEGV）→ 引擎是單一執行緒，獨自擁有裝置並一次處理一個指令（AC15、AC22）
4. **每次自動前進都有爆音**，因為曲目結束時沒有 drain PCM（修正 #13）。而該修正在相同格式的曲目之間 drain，所以播放是「有靜音間隙」而不是無縫播放 → AC17
5. **固定格式的介面上靜音播放**（Focusrite Vocaster），直到 `plughw:` 後備方案（#7）→ AC12
6. **音量是靠縮放每個取樣來套用**，而 UI 仍然說「bit-perfect」→ 本規格精確定義 `bit_perfect`（AC12、AC15）；音量本身是 0004，它必須在增益 ≠ 1 時清除該旗標
7. **取樣率設為「最接近」**（`snd_pcm_hw_params_set_rate_near`，tidalt `internal/player/alsa.c:72`）：沒有來源取樣率的裝置可能以另一個取樣率開啟，並透過隱藏的重新取樣器或以錯誤的速度播放 → AC12（`hw:` 上使用精確的取樣率，在 `hw_params` 之後讀回；不符視為被拒絕並轉到後備方案，且會被回報）
8. **跳轉會從位元組 0 重新下載串流並解碼到目標位置之前的所有內容**，所以在 hi-res 曲目接近結尾處跳轉要花數秒 → AC19
9. **暫停會遺失仍在裝置緩衝區中的音訊**（暫停時 PCM 被丟棄並關閉），所以繼續播放時會往前跳最多一個緩衝區 → AC18（暫停時釋放裝置是 0005 的；它也不能遺失 frame）
10. **位置來自「已寫入的 frame」**，這比聽者聽到的領先一個裝置緩衝區 → AC20

<a id="behaviour"></a>
## 行為

<a id="commands"></a>
### 指令

| 指令 | 作用 |
|---|---|
| `tidal-player play <track-id> [--quality Q] [--device PCM] [--start SECONDS]` | 在前景、無畫面（沒有 TUI）地解析並播放一首曲目，結束時退出。這是永久的 CLI 指令（決策 3）；在 0004 加入佇列與 TUI 控制之前，引擎也靠它驗收。需要儲存的工作階段（spec 0002）；沒有的話以 1 結束並印出 `Not logged in: run "tidal-player login"` |
| `tidal-player devices` | 列出播放裝置：`default`（shared）在前，然後是每個有播放串流的 `hw:C,D`，附上音效卡的名稱。`play` 會使用的裝置以 `*` 標示 |

`play` 會印出下列內容，然後是一條進度行，stdout 是終端機時就地重繪（不是終端機時就不再多印任何東西）：

```
Track 77640617: HI_RES_LOSSLESS, FLAC 24-bit 96 kHz stereo
Output: hw:1,0 (exclusive) S32_LE 96 kHz 2 ch, bit-perfect
  1:23 / 4:56
```

- 第一行給出 **Tidal 授予的**音質（可能比要求的低，AC5）與解碼器找到的來源格式
- 第二行給出實際開啟的裝置、它的種類、協商的取樣格式，以及 `bit-perfect` 或它不是的原因：`resampled (plughw fallback: device refused S24_3LE/S24_LE/S32_LE at 96 kHz)`、`shared (system mixer)`、`lossy source`
- 串流沒有說明長度時，長度顯示為 `?:??`
- 結束碼：`0` 曲目播放到結束；`130` Ctrl-C（先關閉並釋放裝置）；`1` 任何錯誤，stderr 一行（範例見「邊界情況與錯誤」）；`2` 參數錯誤

<a id="settings-until-spec-0008-adds-apptoml"></a>
### 設定（在 spec 0008 加入 `app.toml` 之前）

| 設定 | 旗標 | 環境變數 | 預設 |
|---|---|---|---|
| 要求的最高音質 | `--quality`（`hi-res`、`lossless`、`high`） | `TIDAL_PLAYER_QUALITY` | `hi-res` |
| 輸出裝置 | `--device`（任何 ALSA PCM 名稱） | `TIDAL_PLAYER_DEVICE` | `default`（shared；決策 2） |

旗標優先於環境變數，環境變數優先於預設。0008 把兩者都移到 `app.toml`，並讓這些變數繼續有效。

<a id="output-kinds"></a>
### 輸出種類

| 給定的裝置 | 種類 | 保留 | 重新取樣 | 緩衝區 | `bit_perfect` 可以為真 |
|---|---|---|---|---|---|
| `hw:C,D`（或 `hw:C`） | **exclusive** | 對音效卡 C 使用 ReserveDevice1 | 關閉（明確指定） | 4 × 1024 frame 的 period | 是 |
| 格式協商被拒絕的 `hw:C,D` | **fallback** → 以 `plughw:C,D` 開啟 | 與 exclusive 相同 | 開啟 | 4 個 period | 否 |
| 使用者給定的 `plughw:…` | **fallback** | 與 exclusive 相同 | 開啟 | 4 個 period | 否 |
| `default` 或任何其他名稱 | **shared** | 無 | 開啟 | 8 個 period | 否 |

當且僅當下列條件成立時，`bit_perfect` 為真：種類是 exclusive、來源是無損（FLAC）、裝置以來源的精確取樣率與聲道數執行，且協商的取樣格式至少容納來源的位元數（所以轉換只是補零）。0004 再加上「且增益為 1」。

<a id="stream-resolution"></a>
### 串流解析

`GET {api_base}/tracks/{id}/playbackinfopostpaywall?audioquality=Q&playbackmode=STREAM&assetpresentation=FULL&countryCode={session country}`，透過 `Authenticator`（spec 0002）。回應（格式為假設，見「事實與假設」）帶有授予的 `audioQuality`、`assetPresentation`、`audioMode`、`bitDepth`、`sampleRate`、`manifestMimeType` 與 base64 的 `manifest`：

- `application/vnd.tidal.bts`：JSON `{mimeType, codecs, encryptionType, urls: [..]}` → **單一檔案**串流（`urls[0]`）
- `application/dash+xml`：MPEG-DASH MPD，有一個音訊 `Representation`，其 `SegmentTemplate` 有 `initialization`、`media`（含 `$Number$`）、`startNumber`、`timescale` 與 `SegmentTimeline` → **分段**串流：init 分段，然後是每個媒體分段及其開始時間與長度
- 其他任何東西、`encryptionType` 不是 `NONE`、`assetPresentation` 不是 `FULL`、`audioMode` 不是 `STEREO`，或下表以外的 codec → 一個指名不支援項目的 `StreamError`。不會下載任何東西

支援的 codec：FLAC（`flac`，位於原始 FLAC 檔案或（分段）MP4 中）與 AAC-LC（`mp4a.40.2`，位於 MP4 中）。**不**支援 HE-AAC（`mp4a.40.5`、`mp4a.40.29`）（symphonia 拒絕 SBR，見「事實與假設」）。

**音質**：一個請求，使用設定的最高音質。Tidal 自己會降級：在 CD 音質的曲目上要求 `HI_RES_LOSSLESS` 時，它以 `200` 與 `audioQuality: HIGH` 回答（probe，2026-10-07），所以沒有客戶端的階梯要走。授予的音質照單全收並顯示給使用者。失敗照原樣回傳，不送第二個請求：

| 回應 | 結果 |
|---|---|
| `401` 且 `subStatus` 為 `4005`（「Asset is not ready for playback」） | `StreamError::NotAvailable`。這**不是**認證失敗：`Authenticator` 不得為它更新權杖（spec 0002，錯誤） |
| `500` 且 `subStatus` 為 `999`（Tidal 對未知曲目 ID，以及對在該國家無法串流的曲目的回答） | `StreamError::NotFound` |
| 其他 `500` | `StreamError::Server(500)` |
| 其他 `5xx`、`429`、傳輸錯誤、逾時 | 暫時性錯誤，不變 |
| 來自 `Authenticator` 的 `LoginRequired` | 不變 |

`LOW` 是 HE-AAC（`mp4a.40.5`，probe），symphonia 無法解碼，所以 `low` 不被接受作為設定（決策 4）。

<a id="fetching"></a>
### 擷取

位元組由獨立的擷取執行緒取得，它向前讀入有界的緩衝區（目標 10 秒的音訊，最多 16 MiB），所以只要緩衝區有內容，音訊執行緒就從不等待網路：

- 單一檔案串流使用 HTTP `Range` 請求；跳轉以位元組偏移量進行（FLAC seek table／MP4 sample table，透過 symphonia）
- 分段串流先擷取 init 分段，再依序擷取媒體分段；跳轉從包含目標的分段重新開始，所以不會擷取更早的分段
- 傳輸錯誤或被截短的本文，會從到達的精確位元組偏移量重試 3 次，分別在 0.5 秒、1 秒與 2 秒之後。然後曲目以 `SourceError::Network` 失敗
- 串流 URL 上的 `403` 或 `410`（簽署的 URL 會過期，見「事實與假設」）會重新解析曲目**一次**，並在新的計畫中從相同的偏移量或分段繼續，只要它回報相同的 codec、取樣率與位元深度。否則曲目失敗

<a id="decode"></a>
### 解碼

symphonia 0.6，純 Rust。解碼器的輸出是交錯的 `i32`，靠左對齊（16 位元取樣 `s` 變成 `s << 16`），這是 0001 的 `Sink` 契約。來源格式（取樣率、聲道、每取樣位元數；有損沒有位元數）來自 codec 參數。單聲道來源會逐取樣複製到兩個聲道，所以輸出一律是立體聲（Tidal 的單聲道曲目因此在拒絕單聲道的 DAC 上仍保持位元完美）。

<a id="output-alsa"></a>
### 輸出（ALSA）

- **Exclusive**（`hw:`）：取得保留（見下），開啟 PCM，對開啟本身的 `EBUSY` 以每 100 ms 重試、共 800 ms，然後協商：交錯存取、**精確的**來源取樣率與 2 聲道，以及來源位元數的偏好清單中裝置接受的第一個取樣格式：
  - 16 位元來源：`S32_LE → S16_LE → S24_3LE → S24_LE`（承襲：有些 DAC 的 `S16_LE` 端點是壞的）
  - 24 位元來源：`S24_3LE → S24_LE → S32_LE`
  - 有損來源：`S32_LE → S24_3LE → S24_LE → S16_LE`（反正不是位元完美；盡量保留解碼器的精度）

  若協商被拒絕（列出的格式、取樣率或聲道數都不被接受），就關閉，並在同一張音效卡上開啟 `plughw:`（種類為 *fallback*）。在這個行程的其餘時間中依裝置記住它，讓之後的曲目略過失敗的嘗試。忙碌的裝置（在預算之後仍是 `EBUSY`，或保留被拒絕或逾時）以 `OutputError::Busy` 失敗，且**絕不**降級
- **Shared**：照給定的名稱開啟，沒有保留，開啟重新取樣，先 `S32_LE`，然後是與 exclusive 相同的清單
- 取樣從靠左對齊的 `i32` 封裝成協商的格式，只丟棄低位元（`S16_LE`：`>> 16`；`S24_LE`、`S24_3LE`：`>> 8`；`S32_LE`：原樣）。對於位元數符合格式的來源，這是無損的
- **Draining**（曲目結束時，有不同格式的預先載入或沒有預先載入，見下）：sink 輪詢裝置的延遲（`snd_pcm_delay`）以等待裝置播完它持有的內容，直到延遲為 0 或以下，或裝置回報 underrun 或 suspend，最多等待該音訊的長度（最多 2 秒）再加 500 ms，然後丟棄剩下的任何內容並準備（prepare）裝置。它從不呼叫 `snd_pcm_drain`，它的等待沒有上限，在某些裝置上永不結束（AC31）
- Underrun（`EPIPE`）與 suspend（`ESTRPIPE`）會被復原（`snd_pcm_recover`）並繼續播放；每一次都會被計數並以 `warn` 記錄。任何其他寫入錯誤（裝置被拔除：`ENODEV`、`EIO`）都使曲目以 `OutputError::Lost` 失敗
- **保留**（`org.freedesktop.ReserveDevice1.Audio{C}`，在 session bus 上），如 tidalt 所做：

  | 情況 | 結果 |
  |---|---|
  | 沒有 session bus | 不保留就繼續 |
  | 名稱沒有擁有者（`RequestRelease` 呼叫以「no owner」出錯） | 取得該名稱，繼續 |
  | 擁有者回覆 `true` | 等待 1 秒（穩定，AC30），取得該名稱（`ReplaceExisting`、`AllowReplacement`），繼續 |
  | 擁有者回覆 `false` | `OutputError::Busy`（「in use by <owner's application name, if known>」） |
  | 500 ms 內沒有回覆 | `OutputError::Busy`（絕不從反應慢的擁有者手中奪取名稱） |

  名稱在引擎關閉裝置時釋放。回應其他應用程式的 `RequestRelease` 不在範圍內（0005）；在那之前，引擎在播放時會拒絕它

<a id="engine"></a>
### 引擎

`tidal-player-audio` 在一個專用的 OS 執行緒上執行引擎。它擁有解碼器與輸出裝置；沒有其他東西會開啟裝置。它從一個 channel 接收指令，並在另一個 channel 上回報事件。指令依序一次處理一個：

| 指令 | 作用 |
|---|---|
| `Play { source, start_at }` | 停止正在播放的任何東西，然後開啟、解碼並從 `start_at` 播放 `source` |
| `Preload { source }` | 把 `source` 開啟為下一首曲目，準備好無縫轉換。取代任何較早的預先載入 |
| `Pause` / `Resume` | 停止／重新開始輸出。沒有 frame 遺失或重複；裝置保持開啟（暫停時釋放它是 0005） |
| `Seek(position)` | 從 `position` 繼續（精確到 frame）。保持暫停／播放的狀態 |
| `SetDevice(device)` | 從現在起使用 `device`；正在播放的曲目在目前位置移到它 |
| `Stop` | 停止、關閉裝置、釋放保留、忘記任何預先載入 |
| `Shutdown` | `Stop`，然後結束執行緒 |

| 事件 | 時機 |
|---|---|
| `Started { source: SourceFormat, output: OutputInfo }` | 曲目的第一個 frame 已被寫入 |
| `Position(Duration)` | 播放期間至少每 250 ms 一次，且在每次跳轉、暫停與繼續之後各一次。位置是聽者聽到的：已寫入的 frame 減去裝置回報的延遲 |
| `Buffering` / `Buffered` | 擷取緩衝區耗盡／重新填回 2 秒的音訊。期間不寫入任何東西；位置不會移動 |
| `Transitioned { source, output }` | 播放移入預先載入的曲目（無縫或不是） |
| `TrackEnded` | 曲目的最後一個 frame 已被**播放**（drain）且沒有預先載入在等待。只用於自然結束 |
| `Paused` / `Resumed` / `Stopped` | 在對應的指令生效之後 |
| `Error(EngineError)` | 曲目失敗（來源、解碼、輸出、不支援）。引擎然後閒置，準備好接受下一個指令。`TrackEnded` 絕不會在同一首曲目的 `Error` 之後出現 |

曲目結束：

- **有預先載入在等待，輸出格式相同**（取樣率、聲道、取樣格式）：下一首曲目的第一個 frame 緊接在目前曲目的最後一個之後寫入，沒有 drain、沒有重新開啟、沒有插入靜音（無縫播放，AC17）
- **有預先載入在等待，格式不同**：drain，以新格式重新開啟，繼續（預期會有間隙）
- **沒有預先載入**：drain，`TrackEnded`，關閉裝置並釋放保留

`OutputInfo` 包含要求的裝置、實際開啟的裝置、種類、取樣格式、取樣率與聲道數，以及 `bit_perfect`，為假時附帶原因。`SourceFormat` 包含 codec、取樣率、聲道與位元數（有損時為 `None`）；授予的 Tidal 音質來自解析出的串流，由呼叫端併入（audio crate 不認識 Tidal 的等級）。

<a id="acceptance-criteria"></a>
## 驗收條件

串流解析（`tidal-player-api::stream`、`tidal-player-core`）：

- **AC1** — `resolve_stream(track_id, max_quality)` 透過 `Authenticator`（bearer header）送出 `GET {api_base}/tracks/{id}/playbackinfopostpaywall`，查詢參數恰好是 `audioquality`、`playbackmode=STREAM`、`assetpresentation=FULL`，以及來自工作階段的 `countryCode`
- **AC2** — BTS manifest 對應到 `StreamPlan::Single { url, codec }`（表格涵蓋測試資料：FLAC、AAC-LC）。`encryptionType` 不是 `NONE` → `StreamError::Unsupported(Encrypted)`；未知的 `manifestMimeType` → `Unsupported(Manifest(mime))`
- **AC3** — DASH manifest 對應到 `StreamPlan::Segmented { init_url, segments, codec }`，其中 `segments` 依序列出每個媒體 URL，`$Number$` 從 `startNumber` 起填入，且每個分段的開始時間與長度取自 `SegmentTimeline`（`t`、`d`、重複 `r`）與 `timescale`。長度的總和等於時間軸的總長。測試資料：probe 錄製的 MPD 格式（`timescale` 96000、`startNumber` 1、`<S d="380928" r="72"/><S d="129030"/>` 沒有 `t`，所以第一個分段從 0 開始）給出 74 個分段，編號 1–74，總計 27 936 774 ticks = `PT4M51.008S`，也就是 MPD 的 `mediaPresentationDuration`
- **AC4** — `assetPresentation` 不是 `FULL` → `StreamError::PreviewOnly`；`audioMode` 不是 `STEREO` → `Unsupported(AudioMode)`；codec 為 `mp4a.40.5` 或 `mp4a.40.29` → `Unsupported(Codec)`；計畫帶有授予的音質，以及回應中有時的來源位元深度與取樣率
- **AC5** — 音質：`resolve_stream` 每次呼叫恰好送出**一個**請求，使用設定的最高音質，不論結果如何；帶有較低 `audioQuality` 的 `200` 給出帶有該授予音質的計畫；失敗依「音質」下的表格對應（`401`/`4005` → `NotAvailable` 且**沒有**權杖更新請求；`500`/`999` → `NotFound`；`503`、`429`、逾時與 `LoginRequired` 不變）
- **AC6** — `tidal_player_core::AudioQuality` 有 `Low < High < Lossless < HiResLossless`，序列化為 Tidal 的名稱（`LOW`、……、`HI_RES_LOSSLESS`），並解析 CLI 名稱 `high`、`lossless`、`hi-res`（`low` 被拒絕，訊息說明 LOW 串流是 HE-AAC，不受支援）

解碼（`tidal-player-audio`）：

- **AC7** — 解碼這些測試資料所得到的取樣，與解碼由相同 PCM 製作的純 FLAC 測試資料相同，並回報正確的來源格式：FLAC 16 位元 44.1 kHz（原始 FLAC）；分段 MP4 中的 FLAC 24 位元 96 kHz，以 init 分段加媒體分段依序串接，從不可 seek 的 reader 讀取。取樣靠左對齊（`decoded == pcm << (32 - bits)`）
- **AC8** — AAC-LC MP4 能解碼並回報 `bits: None`（有損）；HE-AAC MP4 在輸出裝置被開啟**之前**以 `EngineError::Unsupported` 失敗（假 sink 從未被開啟）
- **AC9** — 單聲道 FLAC 以立體聲輸出，兩個聲道都與來源逐取樣相等

輸出（`tidal-player-audio`，決策邏輯位於在測試中以假實作取代的 `PcmBackend` trait 之後；真正的 ALSA 呼叫是它之上的薄層）：

- **AC10** — `choose_format(source_bits, accepted)` 回傳該來源的偏好清單中裝置接受的第一個格式，或 `None`（表格涵蓋：16 位元搭配四種格式的每個被接受的子集、24 位元同樣、有損、沒有任何被接受）
- **AC11** — `pack(samples, format)` 逐位元精確（表格：16 位元與 24 位元來源封裝成四種格式的每一種，包含負的滿量程值；`S24_3LE` 是每個取樣 3 位元組小端序；`S24_LE` 是 4 位元組，取樣位於低 24 位元，帶號擴展）
- **AC12** — 對假後端的 `open_output`：exclusive → 先保留，再開啟，然後以精確的取樣率、關閉重新取樣與 4 個 period 協商；協商被拒絕 → 在同一張音效卡上以 `plughw:` 重新開啟，種類為 fallback，`bit_perfect: false` 並附帶原因，且對同一裝置的第二次 `open_output` 直接前往 `plughw:`；開啟時的 `EBUSY` 以 100 ms 重試，直到成功或 800 ms 過去，然後 `OutputError::Busy`，且後端從未看到 `plughw:` 的開啟；shared → 沒有保留、開啟重新取樣、8 個 period；`bit_perfect` 只在「輸出種類」下列出的情況為真（表格涵蓋種類 × 來源位元 × 協商格式 × 取樣率是否相符）
- **AC13** — 保留的決定依「保留」下的表格（表格涵蓋假 bus 的各種行為：沒有 bus、沒有擁有者、`true`、`false`、沒有回覆）。時間是注入的；穩定等待為 1 秒（AC30），回覆逾時為 500 ms。名稱在裝置關閉時釋放，在每條路徑上都是如此（結束、停止、錯誤、切換裝置）
- **AC14** — 在寫入時回報 underrun 的假裝置會被復原，並恰好收到每個 frame 一次（underrun 會被計數）；回報 `ENODEV` 的則使曲目以 `Error(Output(Lost))` 失敗，且之後沒有 `TrackEnded`

引擎（`tidal-player-audio`，以測試資料來源、假時鐘，以及擴充成帶有可設定延遲的假裝置的 `MemorySink` 驅動）：

- **AC15** — `Play` → `Started`（帶有測試資料的 `SourceFormat` 與 sink 的 `OutputInfo`）→ 每個解碼的 frame 依序恰好到達 sink 一次 → drain → `TrackEnded` 一次 → 裝置被關閉。正在播放時的第二個 `Play` 會停止第一個：sink 絕不會收到兩者交錯的 frame，且任何時候都只有一個裝置開啟
- **AC16** — 曲目中途失敗的來源（在 AC24 的重試之後）、損毀的 FLAC frame，以及輸出錯誤，各自恰好產生一個 `Error(...)` 且**沒有** `TrackEnded`；引擎然後接受新的 `Play` 並播放它
- **AC17** — 無縫播放：預先載入相同格式的曲目時，sink 收到第一首曲目的 frame，緊接著第二首的（精確串接），兩者之間沒有 drain 或重新開啟，並送出 `Transitioned`。預先載入不同格式時，裝置會在第二首曲目的第一個 frame 之前被 drain、關閉，並以新格式重新開啟
- **AC18** — 暫停／繼續：sink 在暫停 → 繼續期間收到的 frame，恰好是曲目的 frame，沒有遺失或重複；暫停期間不寫入任何東西，且 `Position` 不變；`Resume` 之後立即回報的位置等於 `Pause` 時回報的位置
- **AC19** — 跳轉：在 `Seek(t)` 之後，sink 收到的第一個 frame 是曲目的第 `round(t × rate)` 個 frame（與測試資料的完整解碼比較），且下一個 `Position` 是 `t`。暫停時的跳轉保持暫停。跳過結尾的跳轉給出 `TrackEnded`。在分段來源上，假來源沒有記錄到任何在 `t` 或之前結束的分段被擷取；在單一檔案來源上，跳轉之後沒有讀取跳轉點的位元組偏移量之前的位元組
- **AC20** — `Position` =（已寫入的 frame − 假裝置的延遲）/ 取樣率；播放期間事件最多相隔 250 ms（假時鐘），且除了跳轉之後從不倒退
- **AC21** — 播放中的 `SetDevice` 在開啟新裝置之前會先關閉舊裝置（並釋放它的保留），且新裝置的第一個 frame 是在舊裝置上到達的位置的那個 frame，除了舊裝置尚未 drain 的延遲之外，沒有遺失或重複
- **AC22** — 即使來源卡在一個永不返回的讀取（假），`Stop` 與 `Shutdown` 也在 1 秒內生效、關閉裝置並釋放保留；在 `Shutdown` 之後執行緒已結束（在測試中 join）
- **AC23** — 停滯的來源給出 `Buffering`，然後沒有寫入且位置凍結，接著一旦再次有 2 秒的音訊可用就是 `Buffered` 與正常播放；不會計數任何 underrun

擷取與 CLI（`tidal-player`）：

- **AC24** — `HttpSource`（對 `wiremock`）：以 `Range` 請求讀取單一檔案串流；被截短的本文以 `Range: bytes=<offset>-` 從精確的偏移量繼續，並在 0.5、1 與 2 秒之後重試（注入的 sleeper），然後是 `SourceError::Network`；`403` 會呼叫它的重新解析回呼一次，並在新的 URL 上從相同的偏移量繼續（第二個 `403` 則失敗）；分段計畫先擷取 init 分段，再依序擷取分段；向前讀取絕不會持有超過設定的上限
- **AC25** — `tidal-player devices` 的輸出來自對 `/proc/asound/cards` 與 `/proc/asound/pcm` 內容的純解析器（測試資料：沒有音效卡、一張內建音效卡、內建加 USB DAC、僅錄音的裝置則不列出）：`default` 在前，然後是每個有播放串流的 `hw:C,D` 與它的音效卡名稱；設定的裝置以 `*` 標示
- **AC26** — `play`：設定依序來自旗標、環境變數、預設（表格）；帶有未知值的 `--quality` 與 `TIDAL_PLAYER_QUALITY` 以 2 結束；沒有工作階段時 `play` 以 1 結束並顯示登入提示；「Track」與「Output」行依「指令」下的格式化（表格涵蓋授予的音質、來源格式、輸出種類與 `bit_perfect` 原因）；進度行只在 stdout 是終端機時印出
- **AC27** — `docs/playback.md` 記載 `play`、`devices`、設定、輸出種類（exclusive / fallback / shared）與 `bit-perfect` 的意義、如何找到 DAC 的裝置，以及下列錯誤與該如何處理；本規格連結到它

<a id="edge-cases--errors"></a>
## 邊界情況與錯誤

| 情況 | 使用者看到的（來自 `play`；0004 在 TUI 中顯示相同的訊息） |
|---|---|
| 帳號或曲目低於要求的音質 | Tidal 授予較少（AC5）；「Track」行顯示授予的內容 |
| 曲目對這個帳號／國家只提供試聽 | `Track 123 is only available as a preview for this account`（結束碼 1） |
| 曲目對這個帳號或國家不可播放（`401`/`4005`） | `Track 123 is not available in <country>`（結束碼 1） |
| 未知的曲目 ID，或在該國家無法串流（Tidal 回答 `500`/`999`） | `Track 123 was not found, or cannot be streamed in <country>`（結束碼 1） |
| 加密串流、不支援的 codec（HE-AAC）、Dolby Atmos / 360 模式 | `Track 123 is not playable: <what>`（結束碼 1）。絕不會有一陣噪音 |
| 裝置忙碌（另一個應用程式佔用 `hw:`），或保留被拒絕 | `Output hw:1,0 is busy (used by <app>): close it, or use --device default`（結束碼 1）。絕不會悄悄降級為 `plughw:` 或 shared |
| 裝置不存在（`hw:5,0`、打錯字） | `No such output device hw:5,0: see "tidal-player devices"`（結束碼 1） |
| DAC 在播放時被拔除 | `Output hw:1,0 was lost`（結束碼 1），裝置與保留已釋放，沒有 panic（AC14） |
| 裝置拒絕來源格式 | 透過 `plughw:` 播放，「Output」行說明它為什麼不是位元完美（AC12） |
| 曲目中途網路中斷 | 重試（AC24）；緩衝區涵蓋短暫的間隙（AC23）；重試之後：`Network error while streaming track 123`（結束碼 1）。絕不會被回報為「曲目結束」（AC16） |
| 串流 URL 過期（長時間暫停、過了一陣子之後跳轉） | 重新解析一次，過程透明（AC24） |
| 解析時工作階段過期 | `Authenticator` 的 `LoginRequired`（spec 0002）：`Session expired: run "tidal-player login"`（結束碼 1） |
| 在保留或開啟裝置期間按 Ctrl-C | 在 1 秒內以 130 結束，沒有任何東西還被保留（AC22） |
| 曲目之間取樣率改變 | drain，以新取樣率重新開啟（AC17） |
| AAC 編碼器延遲／補白 | 不修剪：有損曲目不是取樣精確的無縫播放（不在範圍內） |
| 跳轉到恰好結尾或之後 | `TrackEnded`（AC19） |
| 在沒有硬體暫停（不支援 `snd_pcm_pause`）的裝置上暫停 | Pause 之後最多一個緩衝區（約 0.1 秒）仍會繼續播放；在 Resume 時，下一次寫入會復原 underrun，所以沒有東西遺失或重複。於 slice B 驗收時發現；0004/0005 可能改為重新餵入被丟棄的緩衝區 |
| 完全沒有 ALSA（`--no-default-features` 建置） | `play` 以 1 結束：`this build has no ALSA output` |

<a id="test-plan"></a>
## 測試計畫

每個自動化測試都以它的條件命名（`ac5_…`）。Red 是針對存根型別與本體為存根的函式的失敗斷言（沒有 `todo!()`，沒有編譯錯誤），與 spec 0001 及 0002 相同。HTTP 測試使用 `wiremock` 與 `crates/api/tests/fixtures/stream/` 下的測試資料（依 probe 錄製的格式寫成，URL 與 ID 換成假值），絕不碰網路。音訊測試資料是 `crates/audio/tests/fixtures/` 下產生的小檔案（總共不到 200 KiB），由該目錄 `README.md` 中記錄的 `ffmpeg` 指令從正弦波製作；測試從不呼叫 `ffmpeg`。時間與 sleep 都是注入的；沒有測試會真的 sleep 或開啟真正的音訊裝置。

| AC | 測試（file :: name） | 斷言內容 | 預期的紅燈 |
|----|---------------------|-----------------|--------------|
| AC1 | `crates/api/tests/stream.rs` :: `ac1_playbackinfo_request` | mock 預期路徑、恰好四個查詢參數（國家來自工作階段）與 bearer header，`expect(1)` | 存根 `resolve_stream` 不送出就回傳 `Err` |
| AC2 | `crates/api/src/stream.rs` :: `ac2_bts_manifest`（表格涵蓋測試資料） | 每一列的計畫或錯誤 | 存根解析器對一切都回傳 `Unsupported(Manifest)` |
| AC3 | `crates/api/src/stream.rs` :: `ac3_dash_manifest` | 錄製的 MPD 的分段數、第一個／最後一個 URL、開始時間、總長度；一個帶有 `r` 重複的手寫 MPD | 存根回傳空的分段清單 |
| AC4 | `crates/api/src/stream.rs` :: `ac4_validate_playbackinfo`（表格） | 每個無效欄位 → 它的錯誤；帶有授予的音質、位元與取樣率 | 存根接受一切 |
| AC5 | `crates/api/tests/stream.rs` :: `ac5_one_request`（表格：授予相等、授予較低、`401`/`4005`、`500`/`999`、503、429、逾時、`LoginRequired`） | mock 恰好看到一個 `playbackinfopostpaywall` 請求且沒有 `/token` 請求；每一列的結果 | 存根在任何錯誤時走 tidalt 的階梯，所以 503 那一列看到 4 個請求 |
| AC6 | `crates/core/src/quality.rs` :: `ac6_audio_quality` | 排序、serde 名稱、CLI 名稱、`low` 被拒絕 | 存根解析器什麼都不接受 |
| AC7 | `crates/audio/tests/decode.rs` :: `ac7_flac_raw_and_fmp4` | 取樣等於純 FLAC 參考；來源格式；靠左對齊 | 存根解碼器不產生任何取樣 |
| AC8 | `crates/audio/tests/decode.rs` :: `ac8_aac_lc_and_he_aac` | LC 以 `bits: None` 解碼；HE-AAC → `Unsupported`，假 sink 從未開啟 | 存根回報 `bits: Some(16)` 並先開啟 sink |
| AC9 | `crates/audio/tests/decode.rs` :: `ac9_mono_to_stereo` | L == R == 來源 | 存根輸出一個聲道 |
| AC10 | `crates/audio/src/output/format.rs` :: `ac10_choose_format`（表格） | 如 AC10 | 存根總是回傳 `S32_LE` |
| AC11 | `crates/audio/src/output/format.rs` :: `ac11_pack`（表格） | 每一列的位元組 | 存根寫入零 |
| AC12 | `crates/audio/src/output/open.rs` :: `ac12_open_output`（表格，記錄呼叫的假 `PcmBackend`） | 每一列的呼叫順序與最後的 `OutputInfo` | 存根照給定的開啟裝置、不協商，且 `bit_perfect: true` |
| AC13 | `crates/audio/src/output/reserve.rs` :: `ac13_reservation`（表格，假 bus + 假時鐘） | 每一列的結果；穩定與逾時的長度；在每條關閉路徑上釋放 | 存根總是繼續 |
| AC14 | `crates/audio/tests/engine.rs` :: `ac14_underrun_recovered`、`ac14_device_lost` | frame 恰好一次 + 計數；一個 `Error`，沒有 `TrackEnded` | 存根引擎在第一個寫入錯誤時停止 |
| AC15 | `crates/audio/tests/engine.rs` :: `ac15_play_to_end`、`ac15_play_replaces_play` | 事件順序；sink 內容；單一開啟的裝置 | 存根引擎什麼事件都不發出 |
| AC16 | `crates/audio/tests/engine.rs` :: `ac16_errors_never_end_track`（表格：來源錯誤、損毀的 frame、輸出錯誤） | 一個 `Error`，沒有 `TrackEnded`，下一個 `Play` 會播放 | 存根在任何停止之後送出 `TrackEnded`（tidalt 的錯誤） |
| AC17 | `crates/audio/tests/engine.rs` :: `ac17_gapless_same_format`、`ac17_reopen_on_format_change` | 精確串接，假實作的呼叫記錄中沒有 drain/重新開啟；否則 drain + 重新開啟 | 存根在所有曲目之間 drain 並重新開啟 |
| AC18 | `crates/audio/tests/engine.rs` :: `ac18_pause_resume_loses_nothing` | sink 內容 == 曲目；位置 | 存根在暫停時丟棄裝置緩衝區 |
| AC19 | `crates/audio/tests/engine.rs` :: `ac19_seek`（表格：播放中、暫停、超過結尾、分段、單一檔案） | 跳轉後的第一個 frame；位置；擷取記錄 | 存根從 frame 0 重新開始並解碼到目標，從頭擷取 |
| AC20 | `crates/audio/tests/engine.rs` :: `ac20_position_tracks_delay` | 以假時鐘驗證公式與節奏 | 存根回報已寫入的 frame |
| AC21 | `crates/audio/tests/engine.rs` :: `ac21_set_device_moves_playback` | 先關閉再開啟的順序；連續性 | 存根忽略 `SetDevice` 直到下一個 `Play` |
| AC22 | `crates/audio/tests/engine.rs` :: `ac22_stop_with_blocked_source` | `Stopped` 到達，且執行緒在 1 秒的真實時間內被 join（測試中有限時等待，沒有 sleep）；裝置已關閉、保留已釋放 | 存根等待被阻塞的讀取 |
| AC23 | `crates/audio/tests/engine.rs` :: `ac23_buffering` | 事件、沒有寫入、位置凍結、沒有 underrun | 存根持續寫入靜音 |
| AC24 | `crates/app/tests/http_source.rs` :: `ac24_range_resume`、`ac24_retry_then_fail`、`ac24_reresolve_on_403`、`ac24_segmented_order`、`ac24_readahead_cap` | mock 上的 `Range` header、sleeper 記錄、回呼次數 | 存根只讀取第一個回應 |
| AC25 | `crates/audio/src/devices.rs` :: `ac25_parse_devices`（表格涵蓋測試資料）+ `crates/app/tests/cli.rs` :: `ac25_devices_marks_configured`（透過 `TIDAL_PLAYER_ASOUND_DIR` 的測試資料檔案） | 列出的裝置與順序；`*` 標記 | 存根只回傳 `default` |
| AC26 | `crates/app/src/settings.rs` :: `ac26_settings_precedence`（表格）+ `crates/app/src/cli/play.rs` :: `ac26_status_lines`（表格）+ `crates/app/tests/cli.rs` :: `ac26_bad_quality`、`ac26_play_needs_login` | 優先順序；精確的行；結束碼與 stderr | 存根忽略環境變數且什麼都不印 |
| AC27 | — 驗收時審閱 | `docs/playback.md` 存在、與本規格相符，並在此連結 | — |

刻意不由自動化測試涵蓋、而在驗收時以真實帳號在真實硬體上手動檢查的項目（結果寫在 PR 說明中）：

- 真正的 ALSA 後端，位於 `PcmBackend` 接縫（seam）之後：在 USB DAC 上對一首 16/44.1 與一首 24/96（或更高）的曲目執行 `play`，播放期間讀取 `/proc/asound/cardN/pcm0p/sub0/hw_params` 以確認位元完美的輸出（格式與取樣率必須與「Output」行相符，且取樣率必須等於來源的）
- PipeWire 執行時真正的 ReserveDevice1 握手：`play` 執行期間 PipeWire 讓出音效卡，之後取回；對同一個 `hw:` 的第二個 `play` 會回報忙碌
- 同時有其他音訊在播放時的 shared 模式
- `plughw:` 後備方案，如果手邊有固定格式的介面（否則以缺少來源取樣率的裝置強制觸發）
- 憑耳朵檢查專輯中兩首曲目之間的無縫播放（需要 0004 的佇列；在那之前只靠引擎測試）
- 播放時拔除 DAC；保留期間按 Ctrl-C

<a id="crate-placement"></a>
## Crate 配置

- `tidal-player-core`：`AudioQuality`（AC6）。沒有 I/O
- `tidal-player-api::stream`：`resolve_stream`、`StreamPlan`、`StreamError`、BTS 與 MPD 解析器、回應／錯誤的對應。MPD 解析需要 XML 解析器：`quick-xml`（新的 workspace 相依套件，純 Rust）
- `tidal-player-audio`：引擎執行緒、引擎讀取的 `ByteSource`/`TrackSource` trait、解碼（symphonia，新使用：features `flac`、`isomp4`、`aac`）、格式選擇與封裝、`PcmBackend` + 決策邏輯 + ALSA 實作（feature `alsa`）、裝置清單解析器、ReserveDevice1 用的 `Reserver` trait（D-Bus 實作位於執行檔中，所以 audio crate 不會增加 D-Bus 相依）。仍然沒有 `tokio`、沒有 `core`、沒有 `api`（0001 的分層）
- `tidal-player`（執行檔）：`play` 與 `devices` 子指令、`HttpSource`（reqwest，在自己的擷取執行緒上）、以 `zbus` 實作的 `Reserver`（blocking API；新的 workspace 相依套件，已為 MPRIS 規劃）、把 `resolve_stream` 接到來源的重新解析回呼
- `xtask layering`：沒有變更（audio crate 的新相依是允許的；`core` 一個也不會得到）

<a id="facts-vs-assumptions"></a>
## 事實與假設

2026-10-06 在開發容器中驗證（針對 `symphonia` 0.6.1 的暫時 crate 搭配 ffmpeg 產生的檔案；不是針對 Tidal 真正的串流）：

- symphonia 0.6.1 能解碼 FLAC 16 位元/44.1 kHz 與 24 位元/96 kHz，不論是原始 FLAC 或在**分段 MP4**（`frag_keyframe+empty_moov+default_base_moof`）中，與 ffmpeg 的解碼逐位元相符，包含從不可 seek 的 reader（`ReadOnlySource`）。這回答了 spec 0001 的開放假設：probe（見下）確認 Tidal 的 hi-res DASH 分段就是這種格式
- symphonia 在這些檔案上的 `SeekMode::Accurate` 會落在目標處或之前的 frame 邊界（`actual_ts` ≤ `required_ts`）；呼叫端會丟棄直到 `required_ts` 的 frame
- symphonia 能解碼 MP4 中的 AAC-LC，但只在 SBR 以階層方式標示時（object type 5/29：「aac too complex」）才拒絕 HE-AAC；向後相容的標示（`0x2b7` 同步擴充）與隱含的 SBR 只會被解碼為它的 LC 核心。因此引擎自己檢查 AudioSpecificConfig，並把每種形式的 HE-AAC 都回報為 `Unsupported`（於 slice C 驗收時發現）。它不修剪 AAC 編碼器延遲（441 000 frame 的來源解碼成 444 416 frame）
- `moov` 在結尾的**非分段** MP4，無法從不可 seek 的 reader 探測（「missing moov atom」）：單一檔案 MP4 串流需要可 seek（HTTP `Range`）的來源，`HttpSource` 提供之
- tidalt 的 ALSA 偏好清單、緩衝區大小與時間預算如「背景」所引（讀自它的原始碼）

2026-10-07 由 `scripts/tidal-playback-probe.sh` 針對**真實 API** 驗證，由使用者執行（帳號：`PREMIUM`、`highestSoundQuality: HI_RES`、國家 `NG`、client「Android Automotive HiRes」），使用一首 hi-res 曲目（33695188）與一首 CD 音質曲目（3079103）。URL 與權杖已遮蔽：

| 要求 | Hi-res 曲目 | CD 音質曲目 |
|---|---|---|
| `HI_RES_LOSSLESS` | `200`，授予 `HI_RES_LOSSLESS`、`bitDepth` 24、`sampleRate` 96000、DASH、`codecs="flac"` | `200`，授予 **`HIGH`**、BTS、`mp4a.40.2` |
| `LOSSLESS` | `200`，授予 **`HIGH`**、BTS、`mp4a.40.2` | `200`，授予 **`HIGH`**、BTS、`mp4a.40.2` |
| `HIGH` | `HIGH`、`mp4a.40.2`（AAC-LC 44.1 kHz） | 相同 |
| `LOW` | `LOW`、**`mp4a.40.5`**（HE-AAC） | 相同 |

- **device-flow client 從不會得到 16 位元 FLAC。** 要求 `LOSSLESS` 得到的是 AAC `HIGH`，試過的每首曲目都是如此（三次執行，包括曲目 473593668，由使用者確認為 16/44.1）。只有 hi-res 母帶以 FLAC（DASH）提供。Tidal 在 2026 年 4 月限制了該 client ID；修正是在 PKCE client 下更新（見下，以及 spec 0002 AC19）
- `playbackinfopostpaywall` 回應的鍵：`trackId, assetPresentation, audioMode, audioQuality, manifestMimeType, manifestHash, manifest, albumReplayGain, albumPeakAmplitude, trackReplayGain, trackPeakAmplitude`，加上只在 hi-res 授予時才有的 `bitDepth` 與 `sampleRate`
- BTS manifest：`{"mimeType":"audio/mp4","codecs":"mp4a.40.2","encryptionType":"NONE","urls":["https://amz-pr-fa.audio.tidal.com/<id>.mp4?token=…"]}`，一個 URL。`encryptionType` 在每個地方都是 `NONE`
- DASH manifest：`type="static"`、`profiles="urn:mpeg:dash:profile:isoff-main:2011"`、一個 `Period`/`AdaptationSet`/`Representation id="FLAC_HIRES,96000,24" codecs="flac" audioSamplingRate="96000"`、`AudioChannelConfiguration value="2"`、`SegmentTemplate timescale="96000" initialization="…/0.mp4?token=…" media="…/$Number$.mp4?token=…" startNumber="1"`、`SegmentTimeline` `<S d="380928" r="72"/><S d="129030"/>`（3.968 秒的分段，沒有 `t`）。Init 分段 619 位元組，媒體分段約 1.38 MB。init + 第一個分段串接起來是 MP4 中的 FLAC 24 位元 96 kHz 立體聲（ffprobe），也就是上面「事實」下逐位元解碼的格式
- 串流主機（DASH 用 `sp-ad-fa.audio.tidal.com`，BTS 用 `amz-pr-fa.audio.tidal.com`）遵守 `Range`：帶有 `Content-Range` 的 `206`、`Accept-Ranges: bytes`，超過結尾則為 `416`。`Cache-Control: max-age=31536000`
- `HIGH` MP4 的前 256 KiB 就足以讓 ffprobe 使用，所以 `moov` 在最前面（faststart）；`HttpSource` 仍使用 `Range` 來跳轉
- 串流 URL 只帶有一個 `token` 查詢參數：沒有可讀的到期時間。相隔 5 秒的兩個請求給出不同的 manifest（每次都是新的權杖）。權杖的存活時間與過期權杖的狀態**不**清楚（AC24 仍假設是 `403`/`410`）
- 錯誤：未知的曲目 ID → `500 {"status":500,"subStatus":999,"userMessage":"Unexpected error occurred."}`；`assetpresentation=PREVIEW` 以及在 `HI_RES_LOSSLESS` 的 `urlpostpaywall` → `401 {"subStatus":4005,"userMessage":"Asset is not ready for playback"}`；錯誤的 bearer → `401 {"subStatus":11002,…}`
- `GET /v1/users/{id}/subscription` 可行並回報 `highestSoundQuality`（本規格不需要）
- **第二次執行**（2026-10-07T00:22Z，hi-res 曲目 35986245，24/96）：格式相同。`HI_RES_LOSSLESS` → DASH FLAC 24/96（`<S d="380928" r="72"/><S d="256282"/>`、`PT4M52.333S`）；`LOSSLESS` → 又是 `HIGH` AAC；`LOW` → HE-AAC；在 `HI_RES_LOSSLESS` 的 `urlpostpaywall` → 又是 `401`/`4005`。被當作 CD 音質給出的第二個曲目 ID，在**每個**等級從 `playbackinfopostpaywall` 得到的都是 `500`/`999`（`urlpostpaywall` 則是 `401`/`4005`），與未知 ID 的回答相同：所以 `500`/`999` 也表示「這首曲目在這裡無法串流」（錯誤的 ID，或在該國家沒有授權），不只是伺服器故障。該次執行對同一個 ID 的 `PREVIEW` 請求也得到 `500`。相隔 5 秒擷取的兩個 manifest 這次是相同的（第一次執行時不同）：權杖有時會被重用，存活時間仍然未知

- **在 PKCE client 下更新能修正它**（`scripts/tidal-playback-probe2.sh`，2026-10-07T01:00Z，同一個帳號）。以 client `6BDSRdpK9hqEBTgU` 更新 device-flow 的 refresh 權杖：`200`，`clientName` 為「TIDAL_Android_2.87.0」。用那個權杖：
  - CD 音質曲目 473593668 在 `LOSSLESS` **與** `HI_RES_LOSSLESS`：授予 `LOSSLESS`、`bitDepth` 16、`sampleRate` 44100、**DASH**（不是 BTS）：`Representation id="FLAC,44100,16" codecs="flac"`、`timescale="44100"`、`<S d="176128" r="55"/><S d="52930"/>`（3.993 秒的分段）、`PT3M44.854S`。init 623 位元組 + 第一個分段 = MP4 中的 FLAC 16 位元 44.1 kHz（ffprobe），也就是上面「事實」下逐位元解碼的格式
  - Hi-res 曲目 35986245 在 `HI_RES_LOSSLESS`：不變，DASH FLAC 24/96
  - 串流 URL 然後來自帶有 CloudFront 簽署的 `sp-ad-cf.audio.tidal.com`（`Policy`、`Signature`、`Key-Pair-Id`；policy 想必帶有到期時間，未解碼）。那些回應沒有 `Accept-Ranges` header，但對 `Range` 仍回答 `206`
  - device-flow client 之後仍可更新同一個 refresh 權杖（`200`）
- 不論用哪個權杖，header `x-tidal-client-version` 在 `LOSSLESS` 都不會改變任何東西
- `openapi.tidal.com/v2/trackManifests` 對該 CD 曲目列出 `formats: [FLAC, AACLC]`，但搭配 PKCE 權杖時它帶有 `drmData`（`WIDEVINE`）：無法使用。搭配 device-flow 權杖時沒有 `drmData`，但那個權杖是被限制的。本規格維持使用 v1 `playbackinfopostpaywall`

未驗證：

- Tidal 對免費帳號，或在該國家沒有授權的曲目會回答什麼（假設是 `401`/`4005`，即上面見到的格式）
- 44.1 kHz 或 192 kHz 的 hi-res 曲目是否使用相同的 DASH 格式（假設如此；解析器從 MPD 取得 `timescale` 與取樣率）

假設，於手動驗收時檢查：

- `alsa` 0.12 公開後端需要的一切（精確的取樣率、`set_rate_resample`、period/緩衝區大小、`delay`、`pause` 或等價物、`recover`；也包含 `drain`，不過自 AC31 起後端不再呼叫它）
- PipeWire（WirePlumber）仍實作 `org.freedesktop.ReserveDevice1` 並在請求時釋放音效卡
- `zbus` 5 的 blocking API 能在引擎執行緒上運作，而不需要它上面有 tokio runtime

<a id="decisions-answered-by-the-user-2026-10-07"></a>
## 決策（由使用者回答，2026-10-07）

1. **端點**：使用 `playbackinfopostpaywall`，而不是 tidalt 的 `urlpostpaywall`。*由 probe 定案*：`urlpostpaywall` 對這個 client 拒絕 hi-res
2. **預設輸出裝置**：`default`（shared），所以第一次執行絕不會把音效卡從其他應用程式手中奪走；位元完美是選擇性的，用 `--device hw:C,D` / `TIDAL_PLAYER_DEVICE`，而 `devices` 顯示名稱。不自動偵測 DAC
3. **`play` / `devices`**：保留為永久的 CLI 指令，不只是為了驗收（與 TUI 並存的 CLI 是受歡迎的）。0005 在加入其他一次性指令時，可能把它們歸到 `playback …` 之下
4. **`LOW` 是 HE-AAC**（probe）：拒絕 `low` 作為設定，而不是加入帶有 SBR 的 AAC 解碼器（目前純 Rust 沒有；FFmpeg 正是 0001 移除的）。*提議：拒絕它*
5. **其他 tidalt 的錯誤**：播放方面除了「背景」之外沒有；最痛的那些是在常駐程式與客戶端的通訊中。它們會在起草 spec 0005 時從 tidalt 的歷史中收集
6. **CD 音質作為 FLAC**：*由 probe 2 定案*。在 PKCE client 下更新（spec 0002 AC19，與本規格一起實作）會透過 DASH 給出 16/44.1 FLAC 並保留 hi-res。因此 `LOSSLESS` 一律以 DASH 到達；BTS 路徑保留給 `HIGH`（AAC）以及 Tidal 可能仍會送出的任何 BTS FLAC

<a id="out-of-scope"></a>
## 不在範圍內

- 佇列、自動前進策略、下一首／上一首、隨機播放、重複播放、音量（0004）。0004 決定何時送出 `Preload`，並在增益 ≠ 1 時清除 `bit_perfect`
- 暫停時釋放裝置、回應其他應用程式的 `RequestRelease`、常駐程式與客戶端（0005）
- 設定檔（0008）、記住裝置（0009 略過了它：還沒有挑選器）、MPRIS（[0010](0010-mpris.md)，implemented）
- ReplayGain／響度正規化
- 加密串流（包括受 Widevine 保護的 v2 `trackManifests`）、Dolby Atmos / Sony 360、影片
- 有損曲目的無縫播放（AAC 延遲修剪）
- 本機檔案播放、把串流快取到磁碟
- 非 ALSA 的後端（PipeWire 原生、JACK、PulseAudio）：shared 模式透過 ALSA 的 `default` 到達它們
- 在我們自己的程式碼中重新取樣：當裝置無法接受來源取樣率時，由 ALSA 的 plug 層處理

<a id="bugs"></a>
## 錯誤

- **缺少的裝置給出後端錯誤，且 alsa-lib 自己把行寫到 stderr（於 slice D 驗收時發現，2026-10-07）。** 預期（邊界情況）：`--device hw:99,0` 或拼錯的 PCM 名稱以 1 結束，stderr 恰好一行：`No such output device <name>: see "tidal-player devices"`。實際：對不存在的音效卡 `snd_pcm_open` 以 `EINVAL` 失敗，ALSA 後端把它對應為 `SinkError::Backend`（`Output error: audio backend error: … Invalid argument (22)`）；而對任何開啟失敗，alsa-lib 都會先自己把 `ALSA lib …: Unknown PCM …` 印到 stderr，然後才是我們的那一行。根本原因：後端的開啟錯誤對應只把 `ENOENT`/`ENODEV`/`ENXIO` 當作「找不到」，且沒有任何東西重新導向 alsa-lib 的錯誤輸出。新條件：**AC28** — 以 `EINVAL`、`ENOENT`、`ENODEV` 或 `ENXIO` 失敗的開啟是 `SinkError::NotFound`；alsa-lib 自己的診斷絕不會到達 stderr（在引擎執行緒上擷取並改為記錄），所以帶有 `--device hw:99,0` 與 `--device tidal_player_no_such_pcm` 的 `play` 都恰好印出那一行。測試：`crates/audio/src/output/alsa.rs` :: `ac28_open_error_mapping`（表格涵蓋 errno → `PcmError`；紅燈：`EINVAL` 對應到 `Other`），以及 `crates/app/tests/cli.rs` :: `ac26_play_end_to_end_errors` 收緊為兩個裝置名稱並要求精確的 stderr（紅燈：`ALSA lib` 那一行與後端錯誤）
- **在真實硬體上跳轉會失敗，出現 `snd_pcm_pause … Unknown errno (77)`（由使用者回報，2026-10-07，在 0004 讓 TUI 可以做跳轉之後）。** 預期（AC18、AC19、AC23）：播放中或暫停時跳轉，以及跳轉之後緩衝，都會繼續播放；不回報任何東西。實際：`Output error: audio backend error: ALSA function 'snd_pcm_pause' failed with error 'Unknown errno (77)'` 且曲目停止。根本原因：errno 77 是 `EBADFD`：`snd_pcm_pause(1)` 只對 `RUNNING` 的 PCM 有效，`snd_pcm_pause(0)` 只對 `PAUSED` 的有效。跳轉會丟棄裝置緩衝區（`snd_pcm_drop` + `snd_pcm_prepare`），讓 PCM 處於 `PREPARED`；跳轉之後的重新擷取然後回報 `Buffering`，引擎就暫停裝置，而它並未在執行。同樣地，暫停時跳轉再 `Resume`，會對一個已不再是 `PAUSED` 的 PCM 解除暫停；暫停時的 `Play` 也一樣。假的 `PcmBackend` 在任何狀態都接受 `pause`，所以沒有測試看到這個問題。新條件：**AC29** — `PcmSink` 追蹤 PCM 的狀態（開啟、丟棄、drain 與 underrun 復原之後為 `PREPARED`；寫入之後為 `RUNNING`；暫停之後為 `PAUSED`），且只在 `RUNNING` 時呼叫後端的 `pause(true)`、只在 `PAUSED` 時呼叫 `pause(false)`；否則 `set_paused` 不呼叫就成功（`PREPARED` 的 PCM 在被寫入之前什麼都不會播放，而引擎在暫停時從不寫入）。假後端強制執行 ALSA 的狀態規則（在錯誤狀態下的 `pause` 會像 `EBADFD` 一樣失敗）。測試：`crates/audio/src/output/pcm_sink.rs` :: `ac29_pause_follows_pcm_state`（表格：開啟後立刻暫停；暫停再繼續；跳轉（丟棄）然後緩衝暫停；暫停時跳轉再繼續；underrun 復原再暫停；紅燈：後端的 `EBADFD`），且既有的 `pause_discard_drain_delay` 更新為在暫停之前先寫入，因為暫停一個從未執行過的 PCM 正是這個錯誤

- **`ac5_one_request` 在機器負載下失敗（由使用者回報，2026-10-08：在完整的 `cargo test --workspace` 旁邊有另一個大型建置時失敗一次；單獨執行 3/3 為綠）。** 預期（AC5 測試計畫）：不論機器在做什麼，表格都證明每一列只有一個 `playbackinfopostpaywall` 請求。實際：`503: playbackinfo requests`，左 `0`，右 `1`。根本原因：每一列都以 200 ms 的請求逾時執行，它只為了 `timeout` 那一列而存在。reqwest 的逾時也涵蓋連線與送出，所以在被餓死的 CPU 上，與時間無關的某一列會在請求到達 mock 伺服器之前就逾時（0 個請求，得到的是傳輸錯誤，而不是該列的回答）。已重現：把測試執行檔釘在一個核心上以 `nice 19` 執行，旁邊在同一個核心上跑 8 個忙碌迴圈（`taskset -c 0 nice -n 19 <stream test binary> ac5_one_request`），5 次執行有 5 次在第一列失敗。測試的新規則（AC5 不變）：每一列帶有自己的逾時；與時間無關的列使用正式環境的 `RESOLVE_TIMEOUT`（10 秒），而 `timeout` 那一列對延遲 60 秒的回應使用 5 秒，所以一列的結果絕不取決於機器有多快。在相同的重現條件下，修正後的測試 4 次執行有 4 次通過（`timeout` 那一列的 2 秒預算在那裡仍會失敗一次，所以是 5 秒；該列的請求必須在它自己的逾時內到達本機伺服器，這是剩下的唯一時間假設）。測試：`crates/api/tests/stream.rs` :: `ac5_one_request`（紅燈：上述的負載重現）
- **獨占輸出會有規律的嗶聲，shared 輸出則沒有（issue #6，由使用者回報，2026-10-10）。** 預期（Output (ALSA)、AC13）：`hw:` 上的獨占播放是乾淨的，就像透過 PipeWire 的 shared 播放。實際：在 MOONDROP MOONRIVER 3（`35d8:43da`，USB，非同步回饋）上，`hw:3,0` 上的 44.1 kHz 曲目，只要裝置保持開啟，就每隔不到一秒出現一次嗶聲。根本原因：承襲自 tidalt 的 200 ms 穩定等待對這個 DAC 太短。當 PipeWire 剛放開音效卡、而裝置在那個視窗內就被開啟時，DAC 的時脈回饋迴圈永遠鎖不住：它要求的取樣率（`/proc/asound/cardN/stream0` 中的 `Momentary freq`）以鋸齒狀在 43.5 到 44.8 kHz 之間擺盪，約每 0.6 秒一次，而每次回繞就是一聲嗶。ALSA 看不到任何問題：沒有 underrun，緩衝區從未低於 3,373 frame（76 ms），所以 issue 中最先被懷疑的緩衝區大小並沒有關係。在使用者的機器上，以一支像引擎那樣寫入的 C 程式量測（`hw:3,0`、`S32_LE`、44.1 kHz、1024 frame 的 period、4 個 period）：在前面沒有釋放的情況下開啟，20 次中有 0 次出現鋸齒；在 WirePlumber 釋放音效卡之後 200 ms 開啟，12 次中有 11 次；在之後 0.4、0.6、0.8、1、3 與 6 秒開啟，48 次中有 0 次。穩定等待設為 1 秒的播放器：4 次中有 0 次，而被它取代的建置是 4 次中有 4 次。在這個 DAC 上，門檻位於 200 到 400 ms 之間；1 秒為較慢的裝置留有餘裕，而且只花在另一個應用程式佔用過音效卡之後的第一次開啟。新條件：**AC30** — 當音效卡名稱的擁有者對 `RequestRelease` 回覆 `true` 時，引擎在取得該名稱並開啟裝置之前等待 1 秒（`SETTLE`）（AC13 的表格，「擁有者回覆 `true`」那一列）。測試：`crates/audio/src/output/reserve.rs` :: `ac30_settle_after_release`（紅燈：假時鐘記錄到 200 ms 的 sleep），且 `ac13_reservation` 的「owner replies true」那一列依新的長度
- **播放器在 PipeWire 的 ALSA 外掛上換曲目時卡住（issue #84，由使用者回報，2026-11-03）。** 預期（曲目結束、AC15、AC17）：當曲目結束時有不同格式的預先載入在等待，裝置會被 drain 並重新開啟，下一首曲目播放；當最後一首曲目結束時，接著是 `TrackEnded`。`Stop` 與 `Shutdown` 總是生效。實際：在使用 PipeWire 1.6.10（第一次卡住的前一晚才從 1.6.9 升級）的 shared `default` 裝置上，播放在那一點永遠停止：下一首曲目顯示為載入中、位置 0:00，沒有任何指令有作用，且常駐程式忽略 `SIGTERM`，直到 systemd 把它殺掉。根本原因：sink 在引擎執行緒上以 blocking 模式的 `snd_pcm_drain` 做 drain。PipeWire 的 ALSA 外掛在這個版本上從不從它返回：debug 建置停在 `snd_pcm_drain` → `libasound_module_pcm_pipewire.so` → `pthread_cond_wait`，由 `EngineThread::open_output`（格式改變）呼叫，而 `aplay -D default` 在任何輸入結束時都卡在同一個呼叫，不論 period 與緩衝區大小，在耳機的 sink 與 S/PDIF 的上都一樣。在那裡被阻塞的引擎執行緒不接收任何指令，所以下一首曲目與 `Shutdown` 都永遠不會執行。Draining 不得依賴一個可能永不返回的函式庫呼叫。新條件：**AC31** — `PcmSink::drain` 絕不呼叫後端的 `snd_pcm_drain`（`PcmBackend` 接縫（seam）不再有 drain）：它透過注入的時鐘輪詢後端的延遲，直到延遲為 0 或以下，或後端回報 underrun 或 suspend，最多等待在開啟取樣率下該延遲的長度（上限 2 秒）再加 500 ms（最後一次 sleep 會被截短為預算中剩下的部分），然後丟棄並準備（prepare）裝置。自上次 prepare 以來從未被寫入的 PCM 不持有任何東西，會立即返回，不等待。輪詢期間，underrun 或 suspend 以外的裝置錯誤會如前被回傳（裝置消失時為 `Lost`）。測試：`crates/audio/src/output/pcm_sink.rs` :: `ac31_drain_is_bounded`（表格涵蓋假後端的腳本化延遲：播放到 0；結尾的 underrun；永不下降的延遲（卡住的裝置）在預算用完時返回，不早也不晚一個輪詢，包含預算不是輪詢間隔倍數的情況；沒有寫入；等待時裝置消失；紅燈：sink 呼叫後端的 drain，假實作記錄了它，且從不輪詢），且 `pause_discard_drain_delay` 預期接在丟棄之後的 drain 不產生任何呼叫（沒有持有任何東西）
