<p align="center">
  <img src="assets/brand/tinyconnect-logo.png" alt="TinyConnect logo" width="180">
</p>

<h1 align="center">TinyConnect</h1>

<p align="center">
  A compact Windows terminal controller for a local Spotify Connect device.
</p>

<p align="center">
  <img src="assets/demo/tinyconnect-demo.gif" alt="TinyConnect compact terminal demo" width="720">
</p>

TinyConnect v0.1.2 is a small Windows terminal controller for a local Spotify
Connect device powered directly by librespot
(https://github.com/librespot-org/librespot). It displays the current track and
controls the active player without using the Spotify Web API, client IDs, or an
OAuth application of its own.

TinyConnect automatically includes the local Windows computer name in its
advertised device name, such as `TinyConnect (LIVINGROOM-PC)`. Multiple PCs can
therefore run TinyConnect on the same LAN without sharing the same visible
Connect name. Select the host you want from a Spotify client, then use the
terminal controller to play, pause, skip, and adjust the Windows playback
volume.

## Controls

| Key | Action |
| --- | --- |
| Left | Previous track (or restart the current track) |
| Space | Play/pause |
| Right | Next track |
| Up | Increase Windows playback volume by approximately 5% |
| Down | Decrease Windows playback volume by approximately 5% |
| Q | Shut down cleanly |

TinyConnect resolves the Windows default render endpoint when it starts and
uses that same endpoint for its lifetime. The Runtime area displays the
endpoint friendly name and current Windows master volume. If the Windows
default output changes while TinyConnect is running, quit and restart
TinyConnect to use the newly selected endpoint.

## Quick start

For the normal user path, download
`tinyconnect-v0.1.2-windows-x86_64.exe` from the [GitHub
Releases](https://github.com/arlyngoodrich/TinyConnect/releases) page and
double-click it. Then open Spotify on an authenticated device and select
TinyConnect under devices.

TinyConnect targets Windows 10/11 and requires Spotify Premium for Connect
playback. Rust, Cargo, PowerShell, and a repository checkout are not required
for release users. When Windows Terminal is available, the executable opens
one dedicated 80-column by 27-row window; otherwise it runs in the current
console. The executable carries the TinyConnect icon and is otherwise a normal
console application.

The release executable is unsigned, so Windows SmartScreen may warn that an
uncommon download was blocked. No signing infrastructure is included in this
release. The live dedicated window is hosted by Windows Terminal, so its
taskbar identity may remain Windows Terminal rather than TinyConnect.

The repository helper at `scripts/Start-TinyConnect.ps1` remains available for
development and backward compatibility. It locates a release build first and
otherwise falls back to `cargo run --release`; it is not needed for the
released executable.

## Build from source

Requirements:

- Windows 10 or newer
- Rust 1.85 or newer
- A Spotify Premium account for Spotify Connect playback

No credential, audio, or application cache is enabled by TinyConnect. The
application does not include or use CLIAMP.
TinyConnect starts the Spotify Connect volume at 100% while leaving remote
Connect volume control enabled. Windows volume is the intended normal volume
control; Up and Down change the Windows endpoint volume without changing the
Spotify Connect volume.

~~~powershell
cargo run --release
~~~

The first run opens the same terminal UI and waits for a Spotify client to
select TinyConnect. Use Q before closing the terminal window so librespot and
the Connect advertisement can shut down cleanly.

## Project boundaries and attribution

TinyConnect is an unofficial independent project and is not affiliated with,
endorsed by, or sponsored by Spotify AB. Spotify and the Spotify logo are
trademarks of Spotify AB.

TinyConnect uses the open-source librespot library for Spotify Connect session,
discovery, and playback behavior. See the upstream project and its license at
https://github.com/librespot-org/librespot.

Spotify account access, availability, and service behavior remain controlled by
Spotify. TinyConnect does not bypass account requirements or DRM and makes no
guarantee about service availability.

## License

TinyConnect is released under the MIT License. See LICENSE.
