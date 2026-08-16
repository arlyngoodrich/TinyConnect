# TinyConnect

TinyConnect is a small Windows terminal controller for a local Spotify Connect
device powered directly by librespot
(https://github.com/librespot-org/librespot). It displays the current track and
controls the active player without using the Spotify Web API, client IDs, or an
OAuth application of its own.

TinyConnect advertises itself as TinyConnect on the local network. Select it
from a Spotify client, then use the terminal controller to play, pause, skip,
and switch the Windows default playback device.

## Controls

| Key | Action |
| --- | --- |
| Left arrow | Previous track (or restart the current track) |
| Space | Play/pause |
| Right arrow | Next track |
| H | Switch to Headset Earphone (CORSAIR HS55 WIRELESS Gaming Headset) |
| S | Switch to Speaker (Realtek(R) Audio) |
| Q | Shut down cleanly |

The H and S routes change the Windows default playback endpoint and then
restart the local librespot player. This restart is intentional: the Rodio
backend binds WASAPI when the player is created. The current Connect
credentials remain in memory for the rebind and are discarded when TinyConnect
exits.

## Requirements

- Windows 10 or newer
- Rust 1.85 or newer
- A Spotify Premium account for Spotify Connect playback
- AudioDeviceCmdlets (https://github.com/frgnca/AudioDeviceCmdlets) installed
  for H/S switching:

  ~~~powershell
  Install-Module -Name AudioDeviceCmdlets -Scope CurrentUser
  ~~~

No credential, audio, or application cache is enabled by TinyConnect. The
application does not include or use CLIAMP.

## Run

~~~powershell
cargo run --release
~~~

Or use the small helper:

~~~powershell
.\scripts\Start-TinyConnect.ps1
~~~

The first run opens a terminal UI and waits for a Spotify client to select
TinyConnect. The current Windows playback endpoint is shown in the UI.

You can verify the two named endpoints without starting the player:

~~~powershell
.\scripts\Set-TinyConnectAudioDevice.ps1 -Name 'Speaker (Realtek(R) Audio)'
.\scripts\Set-TinyConnectAudioDevice.ps1 -Name 'Headset Earphone (CORSAIR HS55 WIRELESS Gaming Headset)'
~~~

Changing the default endpoint affects other Windows applications using the
default device. Use Q before closing the terminal window so librespot and the
Connect advertisement can shut down cleanly.

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
