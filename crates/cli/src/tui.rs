//! `ratatui` TUI: device list, pairing QR display, confirm/revoke
//! (docs/design.md §4.3).
//!
//! Deliberately simple for M0: poll the device list on a timer rather than
//! subscribing to `PropertiesChanged` per object. A handful of devices at a
//! couple of updates per second is not a performance problem, and it's a lot
//! less code than a fully reactive subscription model — room to grow into a
//! dashboard with transfer progress at M4, not before the protocol works.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{Event as CEvent, KeyCode, KeyEventKind};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::StreamExt;
use penguinsync_protocol::pairing::TOKEN_TTL;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};
use tokio::sync::mpsc;

use crate::dbus_client::{self, ClientError, DeviceInfo};

/// One accent, used for anything the eye should land on first: the app name,
/// the key letters in the hint line, popup titles. Previously every widget
/// picked its own colour ad hoc, which is how a four-widget screen ends up
/// with five colours and no hierarchy.
const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const OK: Color = Color::Green;
const WARN: Color = Color::Yellow;
const ERR: Color = Color::Red;

/// How long a transient status message stays on screen. Long enough to read,
/// short enough that "unpaired desk-fedora" isn't still sitting there five
/// minutes later looking like it just happened.
const STATUS_TTL: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, PartialEq, Eq)]
enum StatusKind {
    Hint,
    Ok,
    Err,
}

struct Status {
    text: String,
    kind: StatusKind,
    /// `None` for the permanent key-hint line, which never expires.
    set_at: Option<Instant>,
}

impl Status {
    fn hint() -> Self {
        Self {
            text: String::new(),
            kind: StatusKind::Hint,
            set_at: None,
        }
    }

    fn message(text: String, kind: StatusKind) -> Self {
        Self {
            text,
            kind,
            set_at: Some(Instant::now()),
        }
    }

    fn expired(&self, now: Instant) -> bool {
        self.set_at
            .is_some_and(|at| now.duration_since(at) >= STATUS_TTL)
    }
}

struct PendingConfirmation {
    device_id: String,
    fingerprint: String,
    name: String,
}

struct PairingDisplay {
    qr: crate::qr::Rendered,
    /// Kept alongside the rendered code so a terminal too small to show the
    /// QR can still offer the URI for the phone's manual-entry field, rather
    /// than dead-ending.
    uri: String,
    fingerprint: String,
    /// When the daemon minted the token behind this QR. Drives both the
    /// countdown and the automatic refresh — see [`should_regenerate`].
    issued_at: Instant,
}

struct App {
    devices: Vec<DeviceInfo>,
    selected: usize,
    status: Status,
    pairing_display: Option<PairingDisplay>,
    confirmation: Option<PendingConfirmation>,
}

impl App {
    fn new() -> Self {
        Self {
            devices: Vec::new(),
            selected: 0,
            status: Status::hint(),
            pairing_display: None,
            confirmation: None,
        }
    }

    fn selected_device(&self) -> Option<&DeviceInfo> {
        self.devices.get(self.selected)
    }

    fn set_status(&mut self, text: impl Into<String>, kind: StatusKind) {
        self.status = Status::message(text.into(), kind);
    }
}

/// Whether the on-screen pairing QR has gone stale and should be replaced.
///
/// Split out as a plain function because the decision has two edges worth
/// testing without a terminal or a running daemon attached.
///
/// The refresh happens *at* expiry, never before it. A phone that scanned the
/// code a moment ago may be mid-handshake, and the daemon holds exactly one
/// token (`Shared::current_token`) — issuing a new one throws the old one
/// away, so an early refresh would break a pairing that was about to succeed.
/// Waiting for the full TTL means the token being discarded is one the daemon
/// would have rejected anyway, which leaves the race window exactly as wide
/// as it already was before any of this.
fn should_regenerate(issued_at: Instant, now: Instant, confirmation_open: bool) -> bool {
    // A confirmation on screen means a phone already redeemed the token and
    // is waiting on a human. Minting a new one underneath that would be
    // pointless at best.
    !confirmation_open && now.duration_since(issued_at) >= TOKEN_TTL
}

/// Seconds left on the displayed token, saturating at zero.
fn remaining_secs(issued_at: Instant, now: Instant) -> u64 {
    TOKEN_TTL
        .saturating_sub(now.duration_since(issued_at))
        .as_secs()
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
                let now = Instant::now();
                if app.status.expired(now) {
                    app.status = Status::hint();
                }
                match dbus_client::list_devices(&connection).await {
                    Ok(devices) => app.devices = devices,
                    Err(e) => app.set_status(format!("refresh failed: {e}"), StatusKind::Err),
                }
                // A pairing code that silently stops working after a minute
                // makes the user re-press 'p' to fix something they had no
                // way to know was broken. Re-mint it instead.
                let stale = app
                    .pairing_display
                    .as_ref()
                    .is_some_and(|p| should_regenerate(p.issued_at, now, app.confirmation.is_some()));
                if stale {
                    match start_pairing(&daemon).await {
                        Ok(display) => app.pairing_display = Some(display),
                        Err(e) => {
                            app.pairing_display = None;
                            app.set_status(format!("QR refresh failed: {e}"), StatusKind::Err);
                        }
                    }
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

/// Ask the daemon for a fresh token and render it. Shared by the `p` key and
/// the expiry refresh so both paths produce an identically-built display.
async fn start_pairing(
    daemon: &dbus_client::Daemon1Proxy<'_>,
) -> Result<PairingDisplay, ClientError> {
    let (qr_uri, fingerprint) = daemon.start_pairing().await?;
    let qr = crate::qr::render(&qr_uri).map_err(|e| {
        ClientError::Zbus(zbus::Error::Failure(format!("failed to render QR: {e}")))
    })?;
    Ok(PairingDisplay {
        qr,
        uri: qr_uri,
        fingerprint,
        issued_at: Instant::now(),
    })
}

/// `true` to keep running, `false` to quit.
async fn handle_key(app: &mut App, code: KeyCode, daemon: &dbus_client::Daemon1Proxy<'_>) -> bool {
    // A pending human confirmation takes over the keyboard until answered —
    // it's a security prompt, not routine navigation (docs/design.md §7).
    if let Some(confirmation) = &app.confirmation {
        match code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let _ = daemon.confirm_pairing(&confirmation.device_id, true).await;
                let name = confirmation.name.clone();
                app.confirmation = None;
                app.pairing_display = None;
                app.set_status(format!("paired with {name}"), StatusKind::Ok);
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                let _ = daemon.confirm_pairing(&confirmation.device_id, false).await;
                app.confirmation = None;
                app.set_status("pairing rejected", StatusKind::Err);
            }
            _ => {}
        }
        return true;
    }

    // With the QR up, Esc dismisses it rather than quitting the whole TUI —
    // the footer says so, and quitting out of a pairing screen by reflex is
    // the kind of thing you only do once before it annoys you.
    if app.pairing_display.is_some() && code == KeyCode::Esc {
        app.pairing_display = None;
        return true;
    }

    match code {
        KeyCode::Char('q') | KeyCode::Esc => return false,
        KeyCode::Char('p') => match start_pairing(daemon).await {
            Ok(display) => {
                app.pairing_display = Some(display);
                app.set_status(
                    "scan the QR on your phone, then confirm the fingerprint",
                    StatusKind::Hint,
                );
            }
            Err(e) => app.set_status(format!("StartPairing failed: {e}"), StatusKind::Err),
        },
        KeyCode::Char('u') => {
            if let Some(device) = app.selected_device().cloned() {
                match daemon.unpair(&device.device_id).await {
                    Ok(()) => app.set_status(format!("unpaired {}", device.name), StatusKind::Ok),
                    Err(e) => app.set_status(format!("Unpair failed: {e}"), StatusKind::Err),
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
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    draw_header(f, chunks[0], app);
    draw_devices(f, chunks[1], app);
    draw_footer(f, chunks[2], app);

    if let Some(confirmation) = &app.confirmation {
        draw_confirmation(f, area, confirmation);
    } else if let Some(pairing) = &app.pairing_display {
        draw_pairing(f, area, pairing);
    }
}

/// One row, not a three-row bordered box around a single word. The name is
/// the only thing here that doesn't change, so it gets the least space.
fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let connected = app.devices.iter().filter(|d| d.connected).count();
    let summary = match (app.devices.len(), connected) {
        (0, _) => "no devices".to_string(),
        (total, 0) => format!("{total} paired · none connected"),
        (total, live) => format!("{total} paired · {live} connected"),
    };

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " PenguinSync",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(summary, Style::default().fg(MUTED)),
        ])),
        area,
    );
}

fn draw_devices(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .title(Span::styled(" Devices ", Style::default().fg(ACCENT)));

    if app.devices.is_empty() {
        let inner = block.inner(area);
        f.render_widget(block, area);
        f.render_widget(
            Paragraph::new(vec![
                Line::raw(""),
                Line::styled("No paired devices yet.", Style::default().fg(MUTED)),
                Line::from(vec![
                    Span::styled("Press ", Style::default().fg(MUTED)),
                    Span::styled(
                        "p",
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" to show a pairing code.", Style::default().fg(MUTED)),
                ]),
            ])
            .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .devices
        .iter()
        .map(|d| {
            let (dot, label, colour) = if d.connected {
                ("\u{25cf}", "connected", OK)
            } else {
                ("\u{25cb}", "offline", MUTED)
            };
            ListItem::new(Line::from(vec![
                Span::styled(dot, Style::default().fg(colour)),
                Span::raw(format!(" {}  ", d.name)),
                Span::styled(
                    &d.device_id[..16.min(d.device_id.len())],
                    Style::default().fg(MUTED),
                ),
                Span::raw("  "),
                Span::styled(label, Style::default().fg(colour)),
            ]))
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(app.selected));
    f.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
        &mut list_state,
    );
}

/// The bottom line: either a transient message or the key hints for whatever
/// is currently on screen. Context-sensitive because a global hint list is
/// wrong most of the time — `[u]npair` means nothing while a QR is up.
fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    if app.status.set_at.is_some() {
        let colour = match app.status.kind {
            StatusKind::Ok => OK,
            StatusKind::Err => ERR,
            StatusKind::Hint => MUTED,
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {}", app.status.text),
                Style::default().fg(colour),
            ))),
            area,
        );
        return;
    }

    let keys: &[(&str, &str)] = if app.confirmation.is_some() {
        &[("y", "confirm"), ("n", "reject")]
    } else if app.pairing_display.is_some() {
        &[("Esc", "dismiss")]
    } else {
        &[
            ("p", "pair"),
            ("u", "unpair"),
            ("\u{2191}/\u{2193}", "select"),
            ("q", "quit"),
        ]
    };

    let mut spans = vec![Span::raw(" ")];
    for (key, label) in keys {
        spans.push(Span::styled(
            *key,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}   "),
            Style::default().fg(MUTED),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Rows the pairing view needs under the code itself: fingerprint, the
/// countdown, a blank, and the dismiss hint.
const PAIRING_FOOTER_ROWS: u16 = 4;

/// The pairing QR, drawn over the whole frame rather than in a centred popup.
///
/// A popup sized as a fraction of the terminal is what broke this: a
/// `Paragraph` truncates lines it cannot fit, and a truncated QR code is not
/// a slightly-worse QR code — it is not a QR code. Cutting the right-hand
/// columns takes the top-right finder pattern with them, and a decoder needs
/// all three to locate the symbol at all. Diagnosed off a raw camera frame
/// pulled from the phone: the code sat flush against this popup's right
/// border with two finder patterns instead of three, and nothing could read
/// it — not the app, not Google Lens, not the stock camera. Taking the full
/// area buys the ~20 columns and ~10 rows that were missing.
///
/// If even the full frame is too small, the code is not drawn at all. A QR
/// nobody can scan is worse than no QR: it sends you hunting for better
/// light and a steadier hand instead of telling you to resize the window.
fn draw_pairing(f: &mut Frame, area: Rect, pairing: &PairingDisplay) {
    let needed_width = pairing.qr.width;
    let needed_height = pairing.qr.height + PAIRING_FOOTER_ROWS;

    f.render_widget(Clear, area);
    if needed_width > area.width || needed_height > area.height {
        f.render_widget(
            Paragraph::new(format!(
                "This terminal is {}x{}, too small to show the pairing QR \
                 uncut (it needs {}x{}). A clipped QR cannot be scanned, so \
                 it isn't drawn.\n\n\
                 Resize the window and press 'p' again — or type this into \
                 the phone's Pair screen by hand:\n\n\
                 {}\n\n\
                 Fingerprint: {}\n\n[Esc] dismiss",
                area.width,
                area.height,
                needed_width,
                needed_height,
                pairing.uri,
                pairing.fingerprint,
            ))
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    // Centred horizontally, top-aligned: the quiet zone the renderer already
    // includes is what separates the code from whatever is beside it.
    let x = area.x + (area.width - needed_width) / 2;
    let qr_area = Rect {
        x,
        y: area.y,
        width: needed_width,
        height: pairing.qr.height,
    };
    let footer_area = Rect {
        x,
        y: area.y + pairing.qr.height,
        width: needed_width,
        height: PAIRING_FOOTER_ROWS,
    };
    f.render_widget(Paragraph::new(pairing.qr.text.as_str()), qr_area);

    // The countdown is the honest version of what was always happening: the
    // code expires after TOKEN_TTL whether or not anything says so. It just
    // used to expire invisibly, and the next thing the user saw was a
    // scan that did nothing.
    let left = remaining_secs(pairing.issued_at, Instant::now());
    let countdown_colour = if left <= 10 { WARN } else { MUTED };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Fingerprint  ", Style::default().fg(MUTED)),
                Span::styled(
                    pairing.fingerprint.as_str(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                format!("Refreshes in {left}s"),
                Style::default().fg(countdown_colour),
            )),
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    "Esc",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" dismiss", Style::default().fg(MUTED)),
            ]),
        ]),
        footer_area,
    );
}

fn draw_confirmation(f: &mut Frame, area: Rect, confirmation: &PendingConfirmation) {
    let width = (area.width * 3 / 4).clamp(20, area.width);
    let height = (area.height * 3 / 4).clamp(10, area.height);
    let popup = Rect {
        x: (area.width.saturating_sub(width)) / 2,
        y: (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(WARN))
        .title(Span::styled(
            " Confirm pairing ",
            Style::default().fg(WARN).add_modifier(Modifier::BOLD),
        ));

    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("Device       ", Style::default().fg(MUTED)),
                Span::raw(confirmation.name.clone()).bold(),
            ]),
            Line::from(vec![
                Span::styled("Fingerprint  ", Style::default().fg(MUTED)),
                Span::raw(confirmation.fingerprint.clone()).bold(),
            ]),
            Line::raw(""),
            Line::styled(
                "Does this match what the phone is showing?",
                Style::default().fg(MUTED),
            ),
            Line::raw(""),
            Line::from(vec![
                Span::styled("y", Style::default().fg(OK).add_modifier(Modifier::BOLD)),
                Span::styled(" confirm    ", Style::default().fg(MUTED)),
                Span::styled("n", Style::default().fg(ERR).add_modifier(Modifier::BOLD)),
                Span::styled(" reject", Style::default().fg(MUTED)),
            ]),
        ])
        .wrap(Wrap { trim: false })
        .block(block),
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

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    /// Everything the rendered frame contains, as one string. Cell-by-cell,
    /// because a `Paragraph` that overflows its area is silently truncated —
    /// asserting on what was *passed in* would pass while the user sees half
    /// a fingerprint.
    fn rendered(width: u16, height: u16, f: impl FnOnce(&mut Frame)) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| f(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    /// The security prompt (docs/design.md §7) is worth a rendering test, not
    /// just a "does it compile": if the fingerprint or either answer key is
    /// clipped off, the human is being asked to approve something they can't
    /// read, and nothing else in the system catches that.
    #[test]
    fn the_confirmation_prompt_shows_what_the_human_has_to_check() {
        let confirmation = PendingConfirmation {
            device_id: "ff".repeat(32),
            fingerprint: "c0b2-6dd9-4c30-f345".to_string(),
            name: "SM-S937B".to_string(),
        };
        let area = Rect::new(0, 0, 80, 24);
        let text = rendered(80, 24, |f| draw_confirmation(f, area, &confirmation));

        assert!(text.contains("SM-S937B"), "device name missing:\n{text}");
        assert!(
            text.contains("c0b2-6dd9-4c30-f345"),
            "fingerprint missing or clipped:\n{text}",
        );
        assert!(text.contains("confirm"), "confirm key missing:\n{text}");
        assert!(text.contains("reject"), "reject key missing:\n{text}");
    }

    /// The countdown is the user's only warning that the code on screen is
    /// about to be replaced. Rendered, for the same truncation reason.
    #[test]
    fn the_pairing_view_shows_the_countdown_under_the_code() {
        let display = PairingDisplay {
            qr: crate::qr::render("penguinsync://pair?v=0&id=ff&token=00").unwrap(),
            uri: "penguinsync://pair?v=0&id=ff&token=00".to_string(),
            fingerprint: "c0b2-6dd9-4c30-f345".to_string(),
            issued_at: Instant::now(),
        };
        let area = Rect::new(0, 0, 80, 40);
        let text = rendered(80, 40, |f| draw_pairing(f, area, &display));

        assert!(
            text.contains("c0b2-6dd9-4c30-f345"),
            "fingerprint missing:\n{text}",
        );
        // Whole seconds, truncated — a token minted microseconds ago already
        // reads as one second gone, so both values are correct here.
        let full = TOKEN_TTL.as_secs();
        assert!(
            text.contains(&format!("Refreshes in {full}s"))
                || text.contains(&format!("Refreshes in {}s", full - 1)),
            "countdown missing:\n{text}",
        );
    }

    /// The other half of the size contract in [`crate::qr`]: when the frame
    /// can't hold the code, the user gets the URI to type instead of a
    /// half-drawn QR.
    #[test]
    fn a_frame_too_small_for_the_code_offers_the_uri_instead() {
        let uri = "penguinsync://pair?v=0&id=ff&token=00";
        let display = PairingDisplay {
            qr: crate::qr::render(uri).unwrap(),
            uri: uri.to_string(),
            fingerprint: "c0b2-6dd9-4c30-f345".to_string(),
            issued_at: Instant::now(),
        };
        let area = Rect::new(0, 0, 40, 12);
        let text = rendered(40, 12, |f| draw_pairing(f, area, &display));

        assert!(text.contains("too small"), "no explanation:\n{text}");
        assert!(text.contains("penguinsync://pair"), "no URI:\n{text}");
    }

    #[test]
    fn a_fresh_qr_is_not_regenerated() {
        let issued = Instant::now();
        assert!(!should_regenerate(issued, issued, false));
        assert!(!should_regenerate(
            issued,
            issued + TOKEN_TTL - Duration::from_millis(1),
            false
        ));
    }

    #[test]
    fn an_expired_qr_is_regenerated() {
        let issued = Instant::now();
        assert!(should_regenerate(issued, issued + TOKEN_TTL, false));
    }

    /// A phone that already redeemed the token is waiting on the human at
    /// the keyboard. Minting a new token underneath that answer helps nobody.
    #[test]
    fn a_pending_confirmation_suppresses_regeneration() {
        let issued = Instant::now();
        assert!(!should_regenerate(issued, issued + TOKEN_TTL * 3, true));
    }

    #[test]
    fn the_countdown_runs_down_and_stops_at_zero() {
        let issued = Instant::now();
        assert_eq!(remaining_secs(issued, issued), TOKEN_TTL.as_secs());
        assert_eq!(
            remaining_secs(issued, issued + Duration::from_secs(20)),
            TOKEN_TTL.as_secs() - 20
        );
        assert_eq!(remaining_secs(issued, issued + TOKEN_TTL * 2), 0);
    }
}
