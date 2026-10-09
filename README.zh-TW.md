[English](README.md) | 繁體中文

仿照 [spotify-player](https://github.com/aome510/spotify-player) 的 Tidal 終端機播放器，邊做邊試（vibe）的開發中專案。

目前可用的功能：以 Tidal 的裝置流程（device flow）登入；位元完美（bit-perfect）播放到 ALSA `hw:` 裝置；支援隨機播放、重複播放與自動播放的佇列；終端機介面，包含你的收藏庫（收藏、播放清單、專輯、藝人）與搜尋；無介面的常駐程式（daemon，以 systemd 使用者服務執行），可讓多個 TUI 連上，並以單次的 `tidal-player playback …` 指令控制；重新啟動後接續上次的工作階段（佇列、位置、模式與音量），停止在原本的位置；透過 MPRIS 支援桌面媒體控制、多媒體鍵與 `playerctl`（含專輯封面）；以及設定檔：`~/.config/tidal-player/app.toml` 存放設定（環境變數與命令列旗標依然有效且優先），與 spotify-player 相容的 `keymap.toml` 存放按鍵，另有按鍵說明視窗（`?`），列出目前所在位置可用的按鍵，可篩選並直接執行。

可複製後修改的預設設定檔：[`examples/app.toml`](examples/app.toml)、[`examples/keymap.toml`](examples/keymap.toml)（檔案寫出了每一項預設值，所以原樣使用不會改變任何行為）。

文件：[TUI](docs/zh-TW/tui.md)、[設定](docs/zh-TW/config.md)（`app.toml`、`keymap.toml`、所有指令）、[播放曲目](docs/zh-TW/playback.md)、[登入](docs/zh-TW/login.md)、[常駐程式](docs/zh-TW/daemon.md)、[桌面控制與多媒體鍵](docs/zh-TW/mpris.md)。

設計規格（英文）：[docs/specs](docs/specs)。
