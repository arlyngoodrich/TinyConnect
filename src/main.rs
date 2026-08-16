use std::{
    io::{self, stdout},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle as ThreadJoinHandle},
    time::Duration,
};

#[cfg(windows)]
use std::process::Command;

use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
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

const SPEAKER_NAME: &str = "Speaker (Realtek(R) Audio)";
const HEADSET_NAME: &str = "Headset Earphone (CORSAIR HS55 WIRELESS Gaming Headset)";
const DEVICE_NAME: &str = "TinyConnect";

type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputRoute {
    Speakers,
    Headset,
}

impl OutputRoute {
    fn device_name(self) -> &'static str {
        match self {
            Self::Speakers => SPEAKER_NAME,
            Self::Headset => HEADSET_NAME,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Speakers => "speakers",
            Self::Headset => "headset",
        }
    }
}

#[derive(Debug)]
struct UiState {
    track: String,
    artist: String,
    position_ms: u32,
    duration_ms: u32,
    playing: bool,
    connection: String,
    output: String,
    status: String,
    bars_phase: usize,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            track: "Waiting for a Spotify Connect selection".to_owned(),
            artist: "Select TinyConnect from a Spotify client".to_owned(),
            position_ms: 0,
            duration_ms: 0,
            playing: false,
            connection: "Advertising".to_owned(),
            output: "Checking Windows output...".to_owned(),
            status: "No credentials or audio cache are stored".to_owned(),
            bars_phase: 0,
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
    let player = Player::new(
        player_config,
        session.clone(),
        mixer.get_soft_volume(),
        move || sink_builder(None, audio_format),
    );
    let player_events = player.get_player_event_channel();

    let (spirc, spirc_task) = Spirc::new(
        ConnectConfig::default(),
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

async fn switch_output(
    route: OutputRoute,
    ui: &mut UiState,
    session_config: &SessionConfig,
    credentials: &Option<Credentials>,
    connected: &mut Option<Connection>,
    player_events: &mut Option<PlayerEventChannel>,
    current_route: &mut Option<OutputRoute>,
) {
    ui.status = format!("Switching to {}...", route.label());
    match set_default_output(route.device_name()) {
        Ok(selected) => {
            ui.output = selected;
            *current_route = Some(route);
            player_events.take();
            if let Some(connection) = connected.take() {
                connection.shutdown().await;
            }

            if let Some(credentials) = credentials {
                match establish_connection(credentials.clone(), session_config).await {
                    Ok((connection, events)) => {
                        *connected = Some(connection);
                        *player_events = Some(events);
                        ui.connection = "Connected".to_owned();
                        ui.status = format!("Rebound to {}", route.label());
                    }
                    Err(error) => {
                        ui.connection = "Disconnected".to_owned();
                        ui.status = format!("Rebind failed: {error}");
                    }
                }
            } else {
                ui.status = format!("{} selected; waiting for Connect", route.label());
            }
        }
        Err(error) => {
            ui.status = format!("Audio switch failed: {error}");
        }
    }
}

async fn handle_key(
    key: KeyEvent,
    ui: &mut UiState,
    session_config: &SessionConfig,
    credentials: &Option<Credentials>,
    connected: &mut Option<Connection>,
    player_events: &mut Option<PlayerEventChannel>,
    current_route: &mut Option<OutputRoute>,
) -> bool {
    if !is_actionable_key_event(&key) {
        return false;
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => true,
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
        KeyCode::Char('h') | KeyCode::Char('H') => {
            switch_output(
                OutputRoute::Headset,
                ui,
                session_config,
                credentials,
                connected,
                player_events,
                current_route,
            )
            .await;
            false
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            switch_output(
                OutputRoute::Speakers,
                ui,
                session_config,
                credentials,
                connected,
                player_events,
                current_route,
            )
            .await;
            false
        }
        _ => false,
    }
}

fn is_actionable_key_event(key: &KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
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
            Constraint::Length(3),
        ])
        .split(area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            " TinyConnect ",
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
        Line::from(format!("Status: {}", ui.status)),
    ])
    .block(Block::default().borders(Borders::ALL).title("Runtime"))
    .wrap(Wrap { trim: true });
    frame.render_widget(status, chunks[4]);

    let controls = Paragraph::new(
        "Left previous   Space play/pause   Right next   H headset   S speakers   Q quit",
    )
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

#[cfg(windows)]
fn run_powershell(script: &str, target: Option<&str>) -> AppResult<String> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    if let Some(target) = target {
        command.env("TINYCONNECT_AUDIO_NAME", target);
    }

    let output = command.output()?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(io::Error::other(if message.is_empty() {
            "PowerShell audio command failed".to_owned()
        } else {
            message
        })
        .into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(windows)]
fn current_default_output() -> AppResult<String> {
    run_powershell(
        "$ErrorActionPreference = 'Stop'; Import-Module AudioDeviceCmdlets -ErrorAction Stop; (Get-AudioDevice -Playback).Name",
        None,
    )
}

#[cfg(windows)]
fn set_default_output(name: &str) -> AppResult<String> {
    let selected = run_powershell(
        "$ErrorActionPreference = 'Stop'; Import-Module AudioDeviceCmdlets -ErrorAction Stop; $target = $env:TINYCONNECT_AUDIO_NAME; $device = Get-AudioDevice -List | Where-Object { $_.Type -eq 'Playback' -and $_.Name -eq $target } | Select-Object -First 1; if ($null -eq $device) { throw \"Playback endpoint not found: $target\" }; Set-AudioDevice -Index $device.Index | Out-Null; (Get-AudioDevice -Playback).Name",
        Some(name),
    )?;
    if selected != name {
        return Err(
            io::Error::other(format!("Windows selected '{selected}' instead of '{name}'")).into(),
        );
    }
    Ok(selected)
}

#[cfg(not(windows))]
fn current_default_output() -> AppResult<String> {
    Err(io::Error::other("TinyConnect audio routing requires Windows").into())
}

#[cfg(not(windows))]
fn set_default_output(_name: &str) -> AppResult<String> {
    Err(io::Error::other("TinyConnect audio routing requires Windows").into())
}

async fn run_app() -> AppResult<()> {
    let mut terminal = TerminalGuard::new()?;
    let mut ui = UiState {
        output: current_default_output()
            .unwrap_or_else(|_| "Unavailable (AudioDeviceCmdlets required)".to_owned()),
        ..UiState::default()
    };
    let (input_sender, mut input_receiver) = mpsc::unbounded_channel();
    let input = InputGuard::spawn(input_sender);

    let session_config = SessionConfig::default();
    let zeroconf_backend = discovery::find(Some("libmdns"))?;
    let mut discovery: Discovery = Discovery::builder(
        session_config.device_id.clone(),
        session_config.client_id.clone(),
    )
    .name(DEVICE_NAME)
    .device_type(DeviceType::Speaker)
    .port(0)
    .zeroconf_backend(zeroconf_backend)
    .launch()?;

    let mut connected: Option<Connection> = None;
    let mut player_events: Option<PlayerEventChannel> = None;
    let mut credentials: Option<Credentials> = None;
    let mut current_route: Option<OutputRoute> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        terminal.terminal.draw(|frame| draw(frame, &ui))?;
        tokio::select! {
            _ = tick.tick() => {
                ui.bars_phase = (ui.bars_phase + 1) % 8;
            }
            Some(key) = input_receiver.recv() => {
                if handle_key(
                    key,
                    &mut ui,
                    &session_config,
                    &credentials,
                    &mut connected,
                    &mut player_events,
                    &mut current_route,
                ).await {
                    break;
                }
            }
            Some(event) = next_player_event(&mut player_events) => {
                apply_player_event(&mut ui, event);
            }
            Some(new_credentials) = discovery.next() => {
                credentials = Some(new_credentials.clone());
                if connected.is_none() {
                    ui.status = "Connect selected; establishing session".to_owned();
                    match establish_connection(new_credentials, &session_config).await {
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
    run_app().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

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
}
