# 規格中文版用語表

`docs/specs/zh-TW/` 的規格中文版、使用者文件的中文版、發行說明與 PR 說明的中文段落，一律使用本表的譯名與臺灣用語（臺式用法）。英文規格是原文；中文版供審閱用，與英文有出入時以英文為準。

## 不翻譯

程式碼、型別與函式名稱、按鍵（`g s`、`Enter`）、指令、路徑、設定名稱（`app.toml`、`cover_img`）、環境變數、AC 編號（`AC1`）、測試名稱、crate 名稱、Tidal API 端點與欄位、畫面上顯示的英文字串（`Nothing playing`）、狀態值（`draft`、`approved`、`implemented`）、規格編號（`spec 0004`）、`tidalt`、`spotify-player`、Tidal、MPRIS、PipeWire、ALSA、D-Bus、TUI、mix、hook。專有名詞第一次出現時可加中文說明，例如「位元完美（bit-perfect）」。

## 規格的固定標題

| English | 正體中文 |
|---|---|
| Status / Owner / Issue / Depends on / User docs | 狀態／負責人／Issue／相依於／使用者文件 |
| Context | 背景 |
| Behaviour | 行為 |
| Acceptance criteria | 驗收條件 |
| Edge cases & errors | 邊界情況與錯誤 |
| Test plan | 測試計畫 |
| Crate placement | Crate 配置 |
| Facts vs. assumptions | 事實與假設 |
| Decisions | 決策 |
| Out of scope | 不在範圍內 |
| Implementation notes | 實作備註 |
| Bugs | 錯誤 |
| Expected / Actual / Root cause | 預期／實際／根本原因 |
| What tidalt did | tidalt 的做法 |

## 譯名

| English | 正體中文 |
|---|---|
| acceptance criterion | 驗收條件 |
| action / actions popup | 動作／動作彈出視窗 |
| album / artist / track / playlist | 專輯／藝人／曲目／播放清單 |
| autoplay / shuffle / repeat | 自動播放／隨機播放／重複播放 |
| bit-perfect | 位元完美 |
| browse | 瀏覽 |
| cache | 快取 |
| client | 客戶端 |
| collection, favorites, Collected Tracks | 收藏（`Collected Tracks` 不譯） |
| config / settings / config file | 設定／設定檔 |
| cover / album cover | 封面／專輯封面 |
| cursor | 游標 |
| daemon | 常駐程式 |
| device / output device | 裝置／輸出裝置 |
| edge case | 邊界情況 |
| event | 事件 |
| fake (test double) | 假實作（fake） |
| filter | 篩選 |
| fixture | 測試資料（fixture） |
| gapless | 無縫播放 |
| issue (GitHub) | issue |
| key hints / keys help / keymap | 按鍵提示／按鍵說明／按鍵對應 |
| keyring | 金鑰圈 |
| library | 音樂庫 |
| link | 連結 |
| log / logs page | 記錄檔／記錄頁 |
| login / log in | 登入 |
| lyrics | 歌詞 |
| message row | 訊息列 |
| page / window / popup / tab | 頁面／視窗／彈出視窗／分頁 |
| player | 播放器 |
| preload | 預先載入 |
| process | 行程 |
| protocol | 協定 |
| quality | 音質 |
| queue | 佇列 |
| radio | 電台 |
| refresh (token) | 更新（權杖） |
| release (version) | 發行版 |
| row / column | 列／欄 |
| search | 搜尋 |
| seam | 接縫（seam） |
| selection / select / mark | 選取／選取／標記 |
| session (login) | 工作階段 |
| snapshot test | 快照測試 |
| socket | socket |
| spec | 規格 |
| standalone | 獨立模式 |
| state directory | 狀態目錄 |
| stream | 串流 |
| test plan | 測試計畫 |
| theme | 佈景主題 |
| thread | 執行緒 |
| token | 權杖 |
| implement / implementation | 實作 |
| verify / verified | 驗證 |
| attach / detach (a client) | 連上／中斷連線 |
| batch | 批次 |
| bus / session bus | 匯流排／工作階段匯流排 |
| codec | 編解碼器 |
| fan-out | 扇出 |
| follow (lyrics, logs) | 跟隨 |
| harness (test) | 測試架構 |
| instance | 執行個體 |
| manual check / hands-on check | 手動檢查 |
| one-shot command | 一次性指令 |
| outbox | 發送佇列 |
| passphrase | 通關密語 |
| prefix / nested prefix | 前綴／巢狀前綴 |
| prompt (input) | 輸入提示 |
| red / green (TDD) | 紅燈／綠燈 |
| render | 繪製 |
| reply / response / request | 回覆／回應／請求 |
| reserve / release (device) | 保留／釋放 |
| rotation (log) | 輪替 |
| runtime / runtime directory | 執行期／執行期目錄 |
| slice | 切片 |
| snapshot (state) | 快照 |
| stub | 存根 |
| subscribe / subscriber | 訂閱／訂閱者 |
| synced / plain lyrics | 同步歌詞／純文字歌詞 |
| top hit | 最佳結果（視窗標題 `Top hit` 不譯） |
| unit (systemd) | 單元 |
| worktree / branch / commit | worktree／分支／commit |

## 臺式用法（不要用左欄）

| 不用 | 用 |
|---|---|
| 默認 | 預設 |
| 信息 | 資訊、訊息 |
| 用戶 | 使用者 |
| 客戶端 | 客戶端 |
| 軟件／硬件 | 軟體／硬體 |
| 文件（指 file） | 檔案 |
| 文檔 | 文件 |
| 程序（指 program） | 程式 |
| 進程 | 行程 |
| 線程 | 執行緒 |
| 網絡 | 網路 |
| 鏈接 | 連結 |
| 服務器 | 伺服器 |
| 數據 | 資料 |
| 屏幕 | 螢幕 |
| 菜單 | 選單 |
| 光標 | 游標 |
| 緩存 | 快取 |
| 隊列 | 佇列 |
| 歌單 | 播放清單 |
| 歌手 | 藝人 |
| 接口 | 介面 |
| 函數 | 函式 |
| 變量 | 變數 |
| 對象（指 object） | 物件 |
| 字符串 | 字串 |
| 調試 | 除錯 |
| 日誌 | 記錄 |
| 運行 | 執行 |
| 創建 | 建立 |
| 實現 | 實作 |
| 支持 | 支援 |
| 質量 | 品質（音訊為「音質」） |
| 視頻／音頻 | 影片／音訊 |
| 設備 | 裝置 |
| 模塊 | 模組 |
| 異步／同步調用 | 非同步／同步呼叫 |
| 回調 | 回呼 |
| 搜索 | 搜尋 |
| 過濾 | 篩選 |
| 標籤頁 | 分頁 |
| 彈窗 | 彈出視窗 |
| 算繪 | 繪製 |
| 樁（stub） | 存根 |
| 快捷鍵 | 快速鍵（本專案多用「按鍵」） |
| 計劃 | 計畫 |
| 刷新 | 更新、重新整理 |
| 優化 | 最佳化 |
| 集成 | 整合 |
| 測試用例 | 測試案例 |

標點用全形（，。：；「」（）），引號用「」與『』；中文與英文單字、行內程式碼之間留一個半形空格（全形標點旁不留），與既有 `docs/zh-TW/` 一致。
