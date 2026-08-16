# TinyConnect

TinyConnect is a small Windows terminal controller for a local Spotify Connect
device powered directly by librespot
(https://github.com/librespot-org/librespot). It displays the current track and
controls the active player without using the Spotify Web API, client IDs, or an
OAuth application of its own.

TinyConnect advertises itself as TinyConnect on the local network. Select it
from a Spotify client, then use the terminal controller to play, pause, and
skip.

## Controls

| Key | Action |
| --- | --- |
| Left arrow | Previous track (or restart the current track) |
| Space | Play/pause |
| Right arrow | Next track |
| Q | Shut down cleanly |

TinyConnect plays through the Windows default playback device selected when
TinyConnect starts. To change outputs, quit TinyConnect, select another
Windows default playback device, and start TinyConnect again.

## Requirements

- Windows 10 or newer
- Rust 1.85 or newer
- A Spotify Premium account for Spotify Connect playback

No credential, audio, or application cache is enabled by TinyConnect. The
application does not include or use CLIAMP.
TinyConnect starts the Spotify Connect volume at 100% while leaving remote
Connect volume control enabled. Windows volume is the intended normal volume
control.

## Run

~~~powershell
cargo run --release
~~~

Or use the small helper:

~~~powershell
.\scripts\Start-TinyConnect.ps1
~~~

The helper runs target\release\tinyconnect.exe directly when that release build
exists, so Rust and Cargo are not required for ordinary use. If the executable
is absent and Cargo is available, it falls back to cargo run --release. If
neither is available, it reports the exact missing executable and the Rust build
command needed to create it.

The first run opens a terminal UI and waits for a Spotify client to select
TinyConnect. The UI reports that it uses the Windows default playback device
selected at startup. Use Q before closing the terminal window so librespot and
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
