use std::{
    ffi::OsStr,
    io::{self, stdout},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle as ThreadJoinHandle},
    time::Duration,
};

#[cfg(windows)]
use std::{env, path::PathBuf, process::Command};

use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures_util::StreamExt;
use librespot::{
    connect::{ConnectConfig, Spirc},
    core::{authentication::Credentials, config::SessionConfig, session::Session},
    discovery::{self, DeviceType, Discovery},
    metadata::audio::{AudioItem, UniqueFields},
    playback::{
        audio_backend,
        config::{AudioFormat, Bitrate, PlayerConfig},
        mixer::{self, MixerConfig},
        player::{Player, PlayerEvent, PlayerEventChannel},
    },
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
};
use tokio::{sync::mpsc, task::JoinHandle};

#[cfg(windows)]
mod windows_audio;

#[cfg(windows)]
use windows_audio::{AudioEndpoint, VOLUME_STEP_PERCENT};

const DEVICE_NAME_PREFIX: &str = "TinyConnect";
const INNER_LAUNCH_ARGUMENT: &str = "--inner";

type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
struct UiState {
    device_name: String,
    track: String,
    artist: String,
    position_ms: u32,
    duration_ms: u32,
    playing: bool,
    connection: String,
    output: String,
    volume_percent: u8,
    status: String,
    bars_phase: usize,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            device_name: DEVICE_NAME_PREFIX.to_owned(),
            track: "Waiting for a Spotify Connect selection".to_owned(),
            artist: "Select TinyConnect from a Spotify client".to_owned(),
            position_ms: 0,
            duration_ms: 0,
            playing: false,
            connection: "Advertising".to_owned(),
            output: "Resolving Windows output...".to_owned(),
            volume_percent: 0,
            status: "No credentials or audio cache are stored".to_owned(),
            bars_phase: 0,
        }
    }
}

impl UiState {
    fn with_audio(device_name: String, output: String, volume_percent: u8) -> Self {
        Self {
            device_name,
            output,
            volume_percent,
            ..Self::default()
        }
    }
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut output = stdout();
        if let Err(error) = execute!(output, EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(error);
        }

        let terminal = match Terminal::new(CrosstermBackend::new(output)) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = disable_raw_mode();
                return Err(error);
            }
        };

        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen, Show);
        let _ = self.terminal.show_cursor();
    }
}

struct InputGuard {
    stop: Arc<AtomicBool>,
    handle: Option<ThreadJoinHandle<()>>,
}

impl InputGuard {
    fn spawn(sender: mpsc::UnboundedSender<KeyEvent>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !stop_for_thread.load(Ordering::Relaxed) {
                if !event::poll(Duration::from_millis(100)).unwrap_or(false) {
                    continue;
                }

                match event::read() {
                    Ok(Event::Key(key)) => {
                        if sender.send(key).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for InputGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct Connection {
    session: Session,
    player: Arc<Player>,
    spirc: Spirc,
    task: JoinHandle<()>,
}

impl Connection {
    async fn shutdown(self) {
        let _ = self.spirc.shutdown();
        let _ = self.task.await;
        self.player.stop();
        drop(self.player);
        self.session.shutdown();
    }
}

async fn establish_connection(
    credentials: Credentials,
    session_config: &SessionConfig,
    audio_device_name: &str,
) -> AppResult<(Connection, PlayerEventChannel)> {
    let session = Session::new(session_config.clone(), None);
    let sink_builder = audio_backend::find(Some("rodio".to_owned()))
        .ok_or_else(|| io::Error::other("librespot Rodio backend is unavailable"))?;
    let mixer_builder = mixer::find(None)
        .ok_or_else(|| io::Error::other("librespot mixer backend is unavailable"))?;
    let mixer = mixer_builder(MixerConfig::default())?;

    let player_config = PlayerConfig {
        bitrate: Bitrate::Bitrate160,
        position_update_interval: Some(Duration::from_millis(500)),
        ..PlayerConfig::default()
    };
    let audio_format = AudioFormat::default();
    let audio_device_name = audio_device_name.to_owned();
    let player = Player::new(
        player_config,
        session.clone(),
        mixer.get_soft_volume(),
        move || sink_builder(Some(audio_device_name.clone()), audio_format),
    );
    let player_events = player.get_player_event_channel();

    let (spirc, spirc_task) = Spirc::new(
        tinyconnect_connect_config(),
        session.clone(),
        credentials,
        Arc::clone(&player),
        mixer,
    )
    .await?;
    let task = tokio::spawn(spirc_task);

    Ok((
        Connection {
            session,
            player,
            spirc,
            task,
        },
        player_events,
    ))
}

async fn next_player_event(player_events: &mut Option<PlayerEventChannel>) -> Option<PlayerEvent> {
    match player_events.as_mut() {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

fn apply_player_event(ui: &mut UiState, event: PlayerEvent) {
    match event {
        PlayerEvent::TrackChanged { audio_item } => {
            ui.track = audio_item.name.clone();
            ui.artist = artist_name(&audio_item);
            ui.duration_ms = audio_item.duration_ms;
            ui.position_ms = 0;
            ui.status = "Track metadata received".to_owned();
        }
        PlayerEvent::Playing { position_ms, .. } => {
            ui.playing = true;
            ui.position_ms = position_ms;
            ui.connection = "Connected".to_owned();
            ui.status = "Playing".to_owned();
        }
        PlayerEvent::Paused { position_ms, .. } => {
            ui.playing = false;
            ui.position_ms = position_ms;
            ui.status = "Paused".to_owned();
        }
        PlayerEvent::PositionChanged { position_ms, .. }
        | PlayerEvent::PositionCorrection { position_ms, .. }
        | PlayerEvent::Seeked { position_ms, .. } => {
            ui.position_ms = position_ms;
        }
        PlayerEvent::Loading { .. } => {
            ui.status = "Loading track".to_owned();
        }
        PlayerEvent::Stopped { .. } => {
            ui.playing = false;
            ui.status = "Stopped".to_owned();
        }
        PlayerEvent::SessionConnected { .. } => {
            ui.connection = "Connected".to_owned();
        }
        PlayerEvent::SessionDisconnected { .. } => {
            ui.connection = "Reconnecting".to_owned();
            ui.playing = false;
        }
        _ => {}
    }
}

fn artist_name(audio_item: &AudioItem) -> String {
    match &audio_item.unique_fields {
        UniqueFields::Track { artists, .. } => artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        UniqueFields::Episode { show_name, .. } => show_name.clone(),
        UniqueFields::Local { artists, .. } => {
            artists.clone().unwrap_or_else(|| "Local audio".to_owned())
        }
    }
}

fn advertised_device_name(computer_name: Option<&str>) -> String {
    match computer_name.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) if !name.eq_ignore_ascii_case(DEVICE_NAME_PREFIX) => {
            format!("{DEVICE_NAME_PREFIX} ({name})")
        }
        _ => DEVICE_NAME_PREFIX.to_owned(),
    }
}

#[cfg(windows)]
fn resolved_computer_name() -> Option<String> {
    use windows::{Win32::System::WindowsProgramming::GetComputerNameW, core::PWSTR};

    let mut buffer = [0u16; 256];
    let mut length = buffer.len() as u32;
    unsafe { GetComputerNameW(PWSTR(buffer.as_mut_ptr()), &mut length).ok()? };
    let length = usize::try_from(length).ok()?;
    if length == 0 || length > buffer.len() {
        return None;
    }

    let name = String::from_utf16(&buffer[..length]).ok()?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(not(windows))]
fn resolved_computer_name() -> Option<String> {
    None
}

fn runtime_device_name() -> String {
    advertised_device_name(resolved_computer_name().as_deref())
}

fn tinyconnect_connect_config() -> ConnectConfig {
    ConnectConfig {
        initial_volume: u16::MAX,
        ..ConnectConfig::default()
    }
}

fn is_quit_key(key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
        _ => false,
    }
}

fn handle_key(
    key: KeyEvent,
    ui: &mut UiState,
    connected: &mut Option<Connection>,
    audio_endpoint: &AudioEndpoint,
) -> bool {
    if !is_actionable_key_event(&key) {
        return false;
    }

    if is_quit_key(&key) {
        return true;
    }

    let code = key.code;
    match code {
        KeyCode::Left => {
            if let Some(connection) = connected.as_ref() {
                let _ = connection.spirc.prev();
                ui.status = "Previous requested".to_owned();
            }
            false
        }
        KeyCode::Char(' ') => {
            if let Some(connection) = connected.as_ref() {
                let _ = connection.spirc.play_pause();
                ui.status = "Play/pause requested".to_owned();
            }
            false
        }
        KeyCode::Right => {
            if let Some(connection) = connected.as_ref() {
                let _ = connection.spirc.next();
                ui.status = "Next requested".to_owned();
            }
            false
        }
        KeyCode::Up | KeyCode::Down => {
            let delta = volume_delta_for_key(code).expect("volume key must have a delta");
            match audio_endpoint.adjust_volume(delta) {
                Ok(volume_percent) => {
                    ui.volume_percent = volume_percent;
                    ui.status = if delta > 0 {
                        "Windows volume increased".to_owned()
                    } else {
                        "Windows volume decreased".to_owned()
                    };
                }
                Err(error) => {
                    ui.status = format!("Windows volume change failed: {error}");
                }
            }
            false
        }
        _ => false,
    }
}

fn is_actionable_key_event(key: &KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
}

fn volume_delta_for_key(code: KeyCode) -> Option<i8> {
    match code {
        KeyCode::Up => Some(VOLUME_STEP_PERCENT),
        KeyCode::Down => Some(-VOLUME_STEP_PERCENT),
        _ => None,
    }
}

fn has_inner_launch_argument<I, S>(arguments: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    arguments
        .into_iter()
        .any(|argument| argument.as_ref() == OsStr::new(INNER_LAUNCH_ARGUMENT))
}

#[cfg(windows)]
fn executable_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(windows)]
fn launch_in_windows_terminal() -> AppResult<bool> {
    if has_inner_launch_argument(env::args_os()) {
        return Ok(false);
    }

    let Some(windows_terminal) = executable_on_path("wt.exe") else {
        return Ok(false);
    };

    let executable = env::current_exe()?;
    let executable = executable.canonicalize().unwrap_or(executable);
    let status = Command::new(windows_terminal)
        .args([
            "--window",
            "new",
            "--size",
            "80,27",
            "new-tab",
            "--title",
            "TinyConnect",
        ])
        .arg(executable)
        .arg(INNER_LAUNCH_ARGUMENT)
        .status();

    Ok(matches!(status, Ok(status) if status.success()))
}

fn draw(frame: &mut Frame, ui: &UiState) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(6),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(4),
        ])
        .split(area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {} ", ui.device_name),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  librespot Windows controller",
            Style::default().fg(Color::Gray),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Device"));
    frame.render_widget(title, chunks[0]);

    let track = Paragraph::new(vec![
        Line::from(Span::styled(
            ui.track.as_str(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            ui.artist.as_str(),
            Style::default().fg(Color::LightCyan),
        )),
        Line::from(format!(
            "{}  {} / {}",
            if ui.playing { "Playing" } else { "Paused" },
            format_time(ui.position_ms),
            format_time(ui.duration_ms)
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title("Now playing"))
    .wrap(Wrap { trim: true });
    frame.render_widget(track, chunks[1]);

    let progress = if ui.duration_ms == 0 {
        0.0
    } else {
        (ui.position_ms as f64 / ui.duration_ms as f64).clamp(0.0, 1.0)
    };
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("Progress"))
        .gauge_style(Style::default().fg(Color::Green))
        .ratio(progress);
    frame.render_widget(gauge, chunks[2]);

    let bars = animated_bars(ui.bars_phase);
    let visualization = Paragraph::new(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(bars, Style::default().fg(Color::Magenta)),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Signal"));
    frame.render_widget(visualization, chunks[3]);

    let status = Paragraph::new(vec![
        Line::from(format!("Connection: {}", ui.connection)),
        Line::from(format!("Output: {}", ui.output)),
        Line::from(format!("Volume: {}%", ui.volume_percent)),
        Line::from(format!("Status: {}", ui.status)),
    ])
    .block(Block::default().borders(Borders::ALL).title("Runtime"))
    .wrap(Wrap { trim: true });
    frame.render_widget(status, chunks[4]);

    let controls = Paragraph::new(vec![
        Line::from("[Left] Previous   [Space] Play/Pause   [Right] Next"),
        Line::from("[Up] Volume +     [Down] Volume -      [Q] Quit"),
    ])
    .block(Block::default().borders(Borders::ALL).title("Controls"))
    .wrap(Wrap { trim: true });
    frame.render_widget(controls, chunks[5]);
}

fn animated_bars(phase: usize) -> String {
    const GLYPHS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    (0..34)
        .map(|index| GLYPHS[(index + phase) % GLYPHS.len()])
        .collect()
}

fn format_time(milliseconds: u32) -> String {
    let seconds = milliseconds / 1000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

async fn run_app() -> AppResult<()> {
    let (audio_endpoint, volume_percent) = AudioEndpoint::open_default()?;
    let device_name = runtime_device_name();
    let mut terminal = TerminalGuard::new()?;
    let mut ui = UiState::with_audio(
        device_name.clone(),
        audio_endpoint.name().to_owned(),
        volume_percent,
    );
    let (input_sender, mut input_receiver) = mpsc::unbounded_channel();
    let input = InputGuard::spawn(input_sender);

    let session_config = SessionConfig::default();
    let zeroconf_backend = discovery::find(Some("libmdns"))?;
    let mut discovery: Discovery = Discovery::builder(
        session_config.device_id.clone(),
        session_config.client_id.clone(),
    )
    .name(device_name)
    .device_type(DeviceType::Speaker)
    .port(0)
    .zeroconf_backend(zeroconf_backend)
    .launch()?;

    let mut connected: Option<Connection> = None;
    let mut player_events: Option<PlayerEventChannel> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        terminal.terminal.draw(|frame| draw(frame, &ui))?;
        tokio::select! {
            _ = tick.tick() => {
                ui.bars_phase = (ui.bars_phase + 1) % 8;
                if let Ok(volume_percent) = audio_endpoint.current_volume_percent() {
                    ui.volume_percent = volume_percent;
                }
            }
            Some(key) = input_receiver.recv() => {
                if handle_key(key, &mut ui, &mut connected, &audio_endpoint) {
                    break;
                }
            }
            Some(event) = next_player_event(&mut player_events) => {
                apply_player_event(&mut ui, event);
            }
            Some(new_credentials) = discovery.next() => {
                if connected.is_none() {
                    ui.status = "Connect selected; establishing session".to_owned();
                    match establish_connection(
                        new_credentials,
                        &session_config,
                        audio_endpoint.name(),
                    )
                    .await
                    {
                        Ok((connection, events)) => {
                            connected = Some(connection);
                            player_events = Some(events);
                            ui.connection = "Connected".to_owned();
                            ui.status = "Session ready".to_owned();
                        }
                        Err(error) => {
                            ui.connection = "Disconnected".to_owned();
                            ui.status = format!("Connect failed: {error}");
                        }
                    }
                }
            }
            else => break,
        }
    }

    drop(input);
    player_events.take();
    if let Some(connection) = connected.take() {
        connection.shutdown().await;
    }
    discovery.shutdown().await;
    Ok(())
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> AppResult<()> {
    #[cfg(windows)]
    if launch_in_windows_terminal()? {
        return Ok(());
    }

    run_app().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use std::ffi::OsString;

    #[test]
    fn controls_accept_only_initial_key_presses() {
        let press =
            KeyEvent::new_with_kind(KeyCode::Char(' '), KeyModifiers::NONE, KeyEventKind::Press);
        let repeat =
            KeyEvent::new_with_kind(KeyCode::Char(' '), KeyModifiers::NONE, KeyEventKind::Repeat);
        let release = KeyEvent::new_with_kind(
            KeyCode::Char(' '),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );

        assert!(is_actionable_key_event(&press));
        assert!(!is_actionable_key_event(&repeat));
        assert!(!is_actionable_key_event(&release));
    }

    #[test]
    fn advertised_name_uses_the_host_without_duplicate_product_prefix() {
        let name = advertised_device_name(Some("HOST-A"));

        assert_eq!(name, "TinyConnect (HOST-A)");
        assert_eq!(name.matches(DEVICE_NAME_PREFIX).count(), 1);
    }

    #[test]
    fn advertised_name_falls_back_for_missing_or_product_only_host_names() {
        assert_eq!(advertised_device_name(None), DEVICE_NAME_PREFIX);
        assert_eq!(advertised_device_name(Some("")), DEVICE_NAME_PREFIX);
        assert_eq!(advertised_device_name(Some("  ")), DEVICE_NAME_PREFIX);
        assert_eq!(
            advertised_device_name(Some(" TinyConnect ")),
            DEVICE_NAME_PREFIX
        );
    }

    #[test]
    fn only_the_private_inner_marker_changes_launch_mode() {
        assert!(!has_inner_launch_argument([OsString::from(
            "tinyconnect.exe"
        )]));
        assert!(has_inner_launch_argument([
            OsString::from("tinyconnect.exe"),
            OsString::from(INNER_LAUNCH_ARGUMENT),
        ]));
        assert!(!has_inner_launch_argument([
            OsString::from("tinyconnect.exe"),
            OsString::from("--inner-mostly"),
        ]));
    }

    #[test]
    fn connect_starts_at_max_volume_with_remote_control_enabled() {
        let config = tinyconnect_connect_config();

        assert_eq!(config.initial_volume, u16::MAX);
        assert!(!config.disable_volume);
    }


    #[test]
    fn ctrl_c_quits_cleanly() {
        let ctrl_c = KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        );
        assert!(is_actionable_key_event(&ctrl_c));
        assert!(is_quit_key(&ctrl_c));

        let plain_c = KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        );
        assert!(is_actionable_key_event(&plain_c));
        assert!(!is_quit_key(&plain_c));
    }
    #[test]
    fn volume_keys_map_to_single_five_percent_steps() {
        assert_eq!(volume_delta_for_key(KeyCode::Up), Some(5));
        assert_eq!(volume_delta_for_key(KeyCode::Down), Some(-5));
        assert_eq!(volume_delta_for_key(KeyCode::Left), None);
    }
}
