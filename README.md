# Audio Extractor

A desktop application built with Tauri and Rust that extracts high-quality audio from YouTube and YouTube Music and saves it directly to your NAS with clean ID3 tags and folder organization.

## Browser Extension (Bridge)

A Chromium Manifest V3 browser extension is provided under `extension/` so you can trigger downloads directly from YouTube or YouTube Music with one click.

### Installation / Loading Unpacked

1. Open Google Chrome (or Brave / Edge).
2. Navigate to `chrome://extensions` in the address bar.
3. Toggle on **Developer mode** in the top right corner.
4. Click **Load unpacked** in the top left.
5. Select the `extension/` folder inside this repository:
   ```
   /Users/jonathan/Chats/Ytdlm/extension
   ```
6. The **Audio Extractor Bridge** icon will appear in your extension toolbar.

### First-Time Configuration

1. Launch the **Audio Extractor** desktop app.
2. In the app's top menu bar, click **Browser Bridge**.
3. This displays your local port (default: `47821`) and copies your secret bridge token to your clipboard.
4. Click the extension icon in your browser and choose **Settings ⚙️** (or right click and open Options).
5. Paste the token into the **Bridge Token** field and click **Save Settings**.

### Usage

1. Browse to any YouTube or YouTube Music song, album, or playlist.
2. Click the Audio Extractor extension icon in your browser toolbar.
   - It automatically classifies songs, albums (`OLAK5uy_...`), and playlists.
   - For links with both a song and a playlist (`watch?v=...&list=...`), you can choose whether to download just the single track or the entire playlist.
3. Click **Send to Audio Extractor**.
4. The download progress will stream live both in your browser popup and inside the desktop app window.
