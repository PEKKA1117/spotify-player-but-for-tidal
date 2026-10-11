[English](README.md) | 正體中文

<a id="tidal-player"></a>
# tidal-player

[![crates.io](https://img.shields.io/crates/v/tidal-player.svg)](https://crates.io/crates/tidal-player)
[![CI](https://github.com/PEKKA1117/spotify-player-but-for-tidal/actions/workflows/ci.yml/badge.svg)](https://github.com/PEKKA1117/spotify-player-but-for-tidal/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Linux 上的 [Tidal](https://tidal.com) 終端機播放器，以 [spotify-player](https://github.com/aome510/spotify-player) 為範本：用鍵盤操作的 TUI 瀏覽你的收藏庫、搜尋與 mix，並以位元完美（bit-perfect）的方式直接播放到你的 DAC。

![在 tidal-player 中播放專輯](https://raw.githubusercontent.com/PEKKA1117/spotify-player-but-for-tidal/master/docs/assets/tui-playing-song.png)

<a id="features"></a>
## 功能

- **位元完美播放**：串流 Tidal 提供的最佳音質（最高 `HI_RES_LOSSLESS` FLAC），以曲目原本的取樣率播放到 ALSA `hw:` 裝置，播放期間向 PipeWire 取得音效卡。無縫播放（gapless），會預先載入下一首。[播放曲目](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Playback-zh-TW)
- **你的收藏庫**：收藏的曲目、播放清單、專輯與藝人，以及專輯、播放清單與藝人頁面；可從動作選單編輯收藏與你的播放清單。[TUI](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/TUI-zh-TW)
- **搜尋、mix 與電台**：搜尋曲目、專輯、藝人與播放清單（`g s`），開啟你的每日 mix（`g m`）或曲目、藝人的電台（`r`），並可在輸入時篩選任何清單（`/`）
- **完整的佇列**：隨機播放、重複播放，佇列播完時以 Tidal 的推薦自動播放，再次啟動時還原上次的工作階段（佇列、位置、模式與音量）
- **常駐程式與用戶端**：以 systemd 使用者服務在背景執行播放器，可連上任意數量的 TUI，並以單次的 `tidal-player playback …` 指令控制。暫停時會釋放音訊裝置。[常駐程式](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Daemon-zh-TW)
- **桌面控制**：透過 MPRIS 支援多媒體鍵、藍牙耳機、GNOME/KDE 媒體控制與 `playerctl`，含專輯封面。[桌面控制與多媒體鍵](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/MPRIS-zh-TW)
- **輸出裝置**：播放中即可切換輸出裝置（TUI 中的 `D`，或在 shell 中用 `tidal-player playback device`），不會中斷播放位置
- **自訂按鍵**：與 spotify-player 相容的 `keymap.toml`；按鍵說明（`?`）列出目前所在位置可用的按鍵並可直接執行；按下 `g` 稍候，就會提示接下來可以按的鍵。[設定](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Configuration-zh-TW)

<a id="screenshots"></a>
## 截圖

*（字型是 Ink Free。如果每次有人抱怨它我就能拿到一分錢，我現在早就是億萬富翁了。）*

按下 `g` 稍候：提示會列出接下來可以按的鍵。

![按下 g 後的按鍵提示](https://raw.githubusercontent.com/PEKKA1117/spotify-player-but-for-tidal/master/docs/assets/tui-library-with-goto-hints.png)

`?` 開啟按鍵說明：先列出目前所在位置的按鍵，可篩選，按 `enter` 直接執行。

![按鍵說明](https://raw.githubusercontent.com/PEKKA1117/spotify-player-but-for-tidal/master/docs/assets/tui-with-help.png)

`D` 列出輸出裝置；按 `enter` 就會在同一位置改由該裝置播放。

![選擇輸出裝置](https://raw.githubusercontent.com/PEKKA1117/spotify-player-but-for-tidal/master/docs/assets/tui-device-choosing.png)

<a id="requirements"></a>
## 需求

- 有 ALSA 的 Linux。MPRIS 需要 D-Bus 工作階段匯流排；有系統金鑰圈（keyring）時登入工作階段存放在其中，否則存放在加密檔案
- 付費的 Tidal 訂閱；hi-res 串流需要包含它的方案
- 建置需要：Rust 1.97 以上、`pkg-config` 與 ALSA 標頭檔

| 發行版 | 建置相依套件 |
|---|---|
| Debian、Ubuntu | `sudo apt install pkg-config libasound2-dev` |
| Arch | `sudo pacman -S pkgconf alsa-lib` |
| Fedora | `sudo dnf install pkgconf-pkg-config alsa-lib-devel` |

<a id="installation"></a>
## 安裝

```sh
cargo install tidal-player
```

或從本儲存庫的 checkout 安裝：

```sh
cargo install --path crates/app
```

<a id="quick-start"></a>
## 快速上手

```sh
tidal-player login   # 開啟連結，輸入代碼
tidal-player         # 開啟 TUI
```

在 TUI 中，`g l` 開啟收藏庫、`g s` 開啟搜尋、`g m` 開啟你的 mix；`enter` 播放、`space` 暫停、`n`/`p` 切換曲目、`?` 列出所有按鍵、`q` 離開。詳見 [TUI](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/TUI-zh-TW)。

要位元完美輸出，先用 `tidal-player devices` 找出你的 DAC，再於 TUI 中按 `D` 選擇它，或在 `app.toml` 設定 `output_device`（見[找出 DAC 的裝置](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Playback-zh-TW#finding-your-dacs-device)）。

要讓播放器在背景持續執行，把它安裝為 systemd 使用者服務：

```sh
mkdir -p ~/.config/systemd/user
tidal-player daemon unit > ~/.config/systemd/user/tidal-player.service
systemctl --user daemon-reload
systemctl --user enable --now tidal-player
```

之後 `tidal-player` 會以 TUI 連上它，`tidal-player playback next`（以及其他 `playback` 指令）可從 shell 或快捷鍵控制它。

<a id="configuration"></a>
## 設定

設定存放在 `~/.config/tidal-player/app.toml`，按鍵存放在 `~/.config/tidal-player/keymap.toml`。[範例 `app.toml`](examples/app.toml) 與[範例 `keymap.toml`](examples/keymap.toml) 寫出了每一項預設值：複製後修改即可。環境變數與命令列旗標依然有效且優先。所有設定與指令見[設定](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Configuration-zh-TW)。

<a id="documentation"></a>
## 文件

使用說明在 [wiki](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Home-zh-TW)：
[TUI](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/TUI-zh-TW) ·
[設定](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Configuration-zh-TW) ·
[播放曲目](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Playback-zh-TW) ·
[登入](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Login-zh-TW) ·
[常駐程式](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/Daemon-zh-TW) ·
[桌面控制與多媒體鍵](https://github.com/PEKKA1117/spotify-player-but-for-tidal/wiki/MPRIS-zh-TW)

每項功能的設計規格（英文）在 [`docs/specs`](docs/specs)。

<a id="acknowledgements"></a>
## 致謝

- aome510 的 [spotify-player](https://github.com/aome510/spotify-player)：本播放器的介面、按鍵與 `keymap.toml` 格式都以它為範本
- Benehiko 的 [tidalt](https://github.com/Benehiko/tidalt)：本專案由這個較早的 Tidal TUI 重寫而來，它在 Tidal API 與位元完美 ALSA 輸出上累積的經驗造就了本專案

<a id="disclaimer"></a>
## 免責聲明

tidal-player 是非官方用戶端，與 TIDAL 沒有關聯，也未獲其背書或贊助。你需要自己的 Tidal 訂閱，並依 Tidal 的條款使用。

<a id="license"></a>
## 授權

依你的選擇，採用以下任一授權：

- Apache License, Version 2.0（[LICENSE-APACHE](LICENSE-APACHE)）
- MIT license（[LICENSE-MIT](LICENSE-MIT)）

除非你明確表示不同意，你有意提交以納入本專案的任何貢獻（依 Apache-2.0 授權的定義），皆依上述方式雙重授權，不附加任何其他條款或條件。
