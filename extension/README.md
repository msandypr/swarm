# Swarm Browser Extension (Manifest V3)

Companion extension for **Swarm Download Manager** (supports Chromium and Firefox-based browsers).

## Features
- **Auto-Interception**: Automatically captures downloads and passes them to the Swarm high-performance engine.
- **Fail-Safe**: If Swarm daemon is not running, the browser completes the download natively without error.
- **Cookie & Referrer Forwarding**: Forwards session cookies and request headers for authenticated downloads.
- **Context Menus**: Right-click any link, image, or video to "Download with Swarm".
- **Live Status Indicator**: Check connection to daemon directly from the extension popup.

## How to Install (Chromium: Edge, Chrome, Brave)
1. Open your browser's extensions page:
   - Microsoft Edge: `edge://extensions`
   - Google Chrome: `chrome://extensions`
   - Brave: `brave://extensions`
2. Enable **Developer mode** (toggle in top right or left sidebar).
3. Click **Load unpacked** (*Muat yang belum dibongkar*).
4. Select this `extension` directory.

## Pairing with Swarm Daemon
1. Click the Swarm icon in your browser toolbar to open the popup.
2. In the Swarm Desktop App, open **Settings** (gear icon) and copy the **Auth Token**.
3. Paste the token into the extension popup and click **Save & Connect**.
4. The status pill will turn green: **Online**.
