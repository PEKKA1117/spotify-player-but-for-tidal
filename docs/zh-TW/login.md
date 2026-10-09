[English](../login.md) | 繁體中文

<a id="logging-in"></a>
# 登入

`tidal-player` 以 OAuth2 裝置流程（device flow）登入 Tidal：你在 Tidal 網頁上核准一組短代碼，播放器從不會看到你的密碼。設計：[spec 0002](../specs/0002-auth.md)。

<a id="commands"></a>
## 指令

<a id="tidal-player-login"></a>
### `tidal-player login`

印出一個連結與一組代碼，並嘗試在你的瀏覽器中開啟該連結：

```
To log in, open this link and approve the code:

  https://link.tidal.com

Code: ABCDE   (expires in 5 min)
Waiting for approval… (Ctrl-C to cancel)
```

開啟連結（若瀏覽器沒有自行開啟），輸入代碼並核准。播放器接著印出 `Logged in as user <id> (<country>)` 並以結束代碼 0 退出。新的登入會取代任何已儲存的工作階段。

- 代碼會在幾分鐘後過期：`Login code expired, run "tidal-player login" again`（結束代碼 1）
- 你在 Tidal 頁面上拒絕：`Login was denied`（結束代碼 1）
- Ctrl-C 取消：不會儲存任何東西，結束代碼 130
- `Warning: Could not switch this session to the PKCE client: CD-quality tracks will stream as AAC instead of FLAC`：登入成功，但把工作階段切換到能取得 16-bit FLAC 的用戶端失敗了（Tidal 拒絕，或無法連線），因此保留登入本身的權杖；Hi-Res 曲目不受影響。重新登入可能會解決

在沒有已儲存工作階段的情況下直接執行 `tidal-player`，會印出 `Not logged in.`，並在 TUI 啟動前執行相同的流程。

<a id="tidal-player-logout"></a>
### `tidal-player logout`

從這台機器上的每個儲存位置刪除已儲存的工作階段，並印出 `Logged out`（若原本沒有則印出 `Not logged in`）。無論哪種情況都以結束代碼 0 退出，且從不詢問密語，所以也能清除你忘了密語的工作階段。

`logout` 也會刪除記住的播放狀態（`playback.json`，見[接續上次的工作階段](playback.md#resuming-the-last-session)），因此下一個帳號不會沿用上一個帳號的佇列。正在執行的播放器會保留它的佇列，並在下次儲存時重新寫入該檔案。

`logout` 無法在 Tidal 端撤銷登入：Tidal 拒絕為此用戶端撤銷。登入在伺服器端仍然有效，直到 Tidal 讓它過期，但這台機器上不會留下任何副本。

<a id="tidal-player-daemon"></a>
### `tidal-player daemon`

從不啟動登入。沒有已儲存的工作階段時，以結束代碼 1 退出並顯示 `Not logged in: run "tidal-player login"`。如何執行它，以及作為 systemd 使用者服務執行：見[常駐程式與用戶端](daemon.md)。執行中播放器的用戶端（`tidal-player playback …`、連接上的 TUI）不需要自己登入。

<a id="where-the-session-is-stored"></a>
## 工作階段儲存在哪裡

1. 系統金鑰圈（keyring）（Secret Service），服務 `tidal-player`，帳號 `session`。
2. 若沒有 Secret Service（沒有 D-Bus 工作階段匯流排、沒有提供者在執行、已鎖定），則使用以 age 加密的檔案：`$XDG_STATE_HOME/tidal-player/session.age`，預設為 `~/.local/state/tidal-player/session.age`。它以 `0600` 權限建立在 `0700` 的資料夾中。

讀取時先找金鑰圈，再找檔案；`logout` 會從兩者刪除。

<a id="the-passphrase"></a>
## 密語

加密檔案需要密語。播放器每個行程只詢問一次，並保存在記憶體中。它依下列順序尋找密語：

1. `TIDAL_PLAYER_PASSPHRASE_FILE`：一個檔案的路徑，該檔案的第一行就是密語（若其他使用者能讀取該檔案，會印出警告）
2. 名為 `tidal-player-passphrase` 的 systemd credential，即 `$CREDENTIALS_DIRECTORY/tidal-player-passphrase`
3. 互動式提示（`Passphrase for the tidal-player session:`），僅在 stdin 與 stderr 都是終端機時。新檔案會詢問兩次；密語錯誤時最多重新提示 3 次

刻意不提供存放密語本身的環境變數：環境變數會經由 `/proc/<pid>/environ` 外洩，也會傳入子行程。

<a id="giving-the-passphrase-to-the-daemon"></a>
### 把密語交給常駐程式

常駐程式沒有終端機，所以請使用檔案或 systemd credential。兩者都沒有時，它以結束代碼 1 退出並顯示 `Session file is encrypted and no passphrase is available: set TIDAL_PLAYER_PASSPHRASE_FILE or a systemd credential`。密語過時時，它以結束代碼 1 退出並顯示 `Wrong passphrase for the session file`（它從不覆寫該檔案）。

```sh
install -m 600 /dev/null ~/.config/tidal-player-passphrase
printf '%s\n' 'my passphrase' > ~/.config/tidal-player-passphrase
TIDAL_PLAYER_PASSPHRASE_FILE=~/.config/tidal-player-passphrase tidal-player daemon
```

在 systemd 使用者單元中，請改用 credential：

```ini
[Service]
LoadCredential=tidal-player-passphrase:%h/.config/tidal-player-passphrase
```

（`LoadCredentialEncrypted=` 的用法相同。`tidal-player daemon unit` 會印出完整的單元，其中這一行已備妥、只需取消註解：見[在 systemd 下執行常駐程式](daemon.md#running-the-daemon-under-systemd)。）

<a id="environment-variables"></a>
## 環境變數

| 變數 | 效果 |
|---|---|
| `TIDAL_PLAYER_PASSPHRASE_FILE` | 加密工作階段檔案的密語檔案（見上文） |
| `TIDAL_PLAYER_STATE_DIR` | 取代狀態資料夾；工作階段檔案為 `<dir>/session.age` |
| `TIDAL_PLAYER_NO_KEYRING=1` | 完全略過金鑰圈，只使用加密檔案（適用於沒有 Secret Service 的機器） |

<a id="session-expired"></a>
## 「Session expired」（工作階段已過期）

Tidal 可能拒絕已儲存的更新權杖（refresh token）（例如長時間未使用後，或登入在 Tidal 端被作廢時）。此時播放器無法更新，會顯示 `Session expired — run "tidal-player login" in another terminal`；在你重新登入之前，API 呼叫都會失敗。

要恢復，請在任何終端機執行 `tidal-player login`，即使 TUI 或常駐程式正在執行也可以。幾秒內，執行中的行程會在儲存位置中發現新的工作階段，清除訊息並繼續運作；你不需要重新啟動它。

短暫的網路故障或 Tidal 伺服器錯誤不算「session expired」：播放器會保留工作階段並重試。
