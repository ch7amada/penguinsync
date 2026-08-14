//! `ratatui` TUI: device list, pairing QR display, confirm/revoke
//! (docs/design.md §4.3).
//!
//! Deliberately simple for M0: poll the device list on a timer rather than
//! subscribing to `PropertiesChanged` per object. A handful of devices at a
//! couple of updates per second is not a performance problem, and it's a lot
//! less code than a fully reactive subscription model — room to grow into a
//! dashboard with transfer progress at M4, not before the protocol works.

use std::io;
use std::time::Duration;

use crossterm::event::{Event as CEvent, KeyCode, KeyEventKind};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use tokio::sync::mpsc;

use crate::dbus_client::{self, ClientError, DeviceInfo};

struct PendingConfirmation {
    device_id: String,
    fingerprint: String,
    name: String,
}

struct PairingDisplay {
    qr: String,
    fingerprint: String,
}

struct App {
    devices: Vec<DeviceInfo>,
    selected: usize,
    status: String,
    pairing_display: Option<PairingDisplay>,
    confirmation: Option<PendingConfirmation>,
}

impl App {
    fn new() -> Self {
        Self {
            devices: Vec::new(),
            selected: 0,
            status: "[p]air  [u]npair  [\u{2191}/\u{2193}] select  [q]uit".to_string(),
            pairing_display: None,
            confirmation: None,
        }
    }

    fn selected_device(&self) -> Option<&DeviceInfo> {
        self.devices.get(self.selected)
    }
}

pub async fn run(connection: zbus::Connection) -> Result<(), ClientError> {
    let daemon = dbus_client::daemon_proxy(&connection).await?;
    let mut pairing_requests = daemon.receive_pairing_requested().await?;

    let mut terminal =
        setup_terminal().map_err(|e| ClientError::Zbus(zbus::Error::InputOutput(e.into())))?;
    let input_rx = spawn_input_thread();
    let mut input_rx = input_rx;
    let mut refresh = tokio::time::interval(Duration::from_millis(500));

    let mut app = App::new();
    app.devices = dbus_client::list_devices(&connection)
        .await
        .unwrap_or_default();

    let result = loop {
        terminal.draw(|f| draw(f, &app)).ok();

        tokio::select! {
            Some(event) = input_rx.recv() => {
                if let CEvent::Key(key) = event
                    && key.kind == KeyEventKind::Press
                    && !handle_key(&mut app, key.code, &daemon).await
                {
                    break Ok(());
                }
            }
            _ = refresh.tick() => {
                match dbus_client::list_devices(&connection).await {
                    Ok(devices) => app.devices = devices,
                    Err(e) => app.status = format!("refresh failed: {e}"),
                }
            }
            Some(signal) = pairing_requests.next() => {
                if let Ok(args) = signal.args() {
                    app.confirmation = Some(PendingConfirmation {
                        device_id: args.device_id().clone(),
                        fingerprint: args.fingerprint().clone(),
                        name: args.name().clone(),
                    });
                }
            }
        }
    };

    restore_terminal(&mut terminal).ok();
    result
}

/// `true` to keep running, `false` to quit.
async fn handle_key(app: &mut App, code: KeyCode, daemon: &dbus_client::Daemon1Proxy<'_>) -> bool {
    // A pending human confirmation takes over the keyboard until answered —
    // it's a security prompt, not routine navigation (docs/design.md §7).
    if let Some(confirmation) = &app.confirmation {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let _ = daemon.confirm_pairing(&confirmation.device_id, true).await;
                app.status = format!("paired with {}", confirmation.name);
                app.confirmation = None;
                app.pairing_display = None;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                let _ = daemon.confirm_pairing(&confirmation.device_id, false).await;
                app.status = "pairing rejected".to_string();
                app.confirmation = None;
            }
            _ => {}
        }
        return true;
    }

    match code {
        KeyCode::Char('q') | KeyCode::Esc => return false,
        KeyCode::Char('p') => match daemon.start_pairing().await {
            Ok((qr_uri, fingerprint)) => match crate::qr::render(&qr_uri) {
                Ok(qr) => {
                    app.pairing_display = Some(PairingDisplay { qr, fingerprint });
                    app.status =
                        "scan the QR on your phone, then confirm the fingerprint".to_string();
                }
                Err(e) => app.status = format!("failed to render QR: {e}"),
            },
            Err(e) => app.status = format!("StartPairing failed: {e}"),
        },
        KeyCode::Char('u') => {
            if let Some(device) = app.selected_device().cloned() {
                match daemon.unpair(&device.device_id).await {
                    Ok(()) => app.status = format!("unpaired {}", device.name),
                    Err(e) => app.status = format!("Unpair failed: {e}"),
                }
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.selected = app.selected.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') if app.selected + 1 < app.devices.len() => {
            app.selected += 1;
        }
        _ => {}
    }
    true
}

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    f.render_widget(
        Paragraph::new("PenguinSync").block(Block::default().borders(Borders::ALL)),
        chunks[0],
    );

    let items: Vec<ListItem> = if app.devices.is_empty() {
        vec![ListItem::new(
            "No paired devices yet — press 'p' to pair one.",
        )]
    } else {
        app.devices
            .iter()
            .map(|d| {
                let dot = if d.connected {
                    Span::styled("\u{25cf}", Style::default().fg(Color::Green))
                } else {
                    Span::styled("\u{25cb}", Style::default().fg(Color::DarkGray))
                };
                ListItem::new(Line::from(vec![
                    dot,
                    Span::raw(format!(" {}  ", d.name)),
                    Span::styled(
                        &d.device_id[..16.min(d.device_id.len())],
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect()
    };
    let mut list_state = ListState::default();
    if !app.devices.is_empty() {
        list_state.select(Some(app.selected));
    }
    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Devices"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        chunks[1],
        &mut list_state,
    );

    f.render_widget(Paragraph::new(app.status.as_str()), chunks[2]);

    if let Some(confirmation) = &app.confirmation {
        draw_popup(
            f,
            area,
            "Confirm pairing",
            &format!(
                "Device: {}\nFingerprint: {}\n\nDoes this match the phone's screen?\n\n[y] confirm    [n] reject",
                confirmation.name, confirmation.fingerprint
            ),
        );
    } else if let Some(pairing) = &app.pairing_display {
        draw_popup(
            f,
            area,
            "Scan to pair",
            &format!(
                "{}\nFingerprint: {}\n\n[Esc] dismiss",
                pairing.qr, pairing.fingerprint
            ),
        );
    }
}

fn draw_popup(f: &mut Frame, area: Rect, title: &str, body: &str) {
    let width = (area.width * 3 / 4).clamp(20, area.width);
    let height = (area.height * 3 / 4).clamp(10, area.height);
    let popup = Rect {
        x: (area.width.saturating_sub(width)) / 2,
        y: (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(body).block(Block::default().borders(Borders::ALL).title(title)),
        popup,
    );
}

fn spawn_input_thread() -> mpsc::UnboundedReceiver<CEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(event) = crossterm::event::read() {
            if tx.send(event).is_err() {
                break;
            }
        }
    });
    rx
}

fn setup_terminal() -> io::Result<ratatui::DefaultTerminal> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(io::stdout(), EnterAlternateScreen)?;
    Ok(ratatui::init())
}

fn restore_terminal(_terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen)?;
    ratatui::restore();
    Ok(())
}
