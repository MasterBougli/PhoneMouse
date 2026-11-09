#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
mod window;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::response::Html;
use axum::routing::get;
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Mouse, Settings};
use serde::Deserialize;

const DEFAULT_PORT: u16 = 8787;
const MAX_ATTEMPTS: u8 = 8;
const BAN_DURATION: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
struct AppState {
    pin: String,
    mouse_tx: Sender<MouseCmd>,
    security: std::sync::Arc<Mutex<Security>>,
}

struct Security {
    failures: HashMap<IpAddr, u8>,
    bans: HashMap<IpAddr, Instant>,
}

enum MouseCmd {
    Move(i32, i32),
    Button(Button, Direction),
    Scroll(i32, Axis),
}

#[derive(Default)]
struct Accum {
    x: f32,
    y: f32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ClientMsg {
    Auth { pin: String },
    Move { x: f32, y: f32 },
    Down { btn: Btn },
    Up { btn: Btn },
    Click { btn: Btn },
    Scroll { x: i32, y: i32 },
    Ping,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Btn {
    Left,
    Right,
    Middle,
}

struct Opts {
    port: u16,
    new_pin: bool,
}

pub(crate) struct LanLink {
    name: String,
    url: String,
}

fn main() {
    #[cfg(windows)]
    {
        window::enable_dpi();
        window::install_panic_hook();
    }

    let opts = parse_args();
    let pin = load_or_create_pin(opts.new_pin);
    let links = lan_links(opts.port);

    let (mouse_tx, mouse_rx) = mpsc::channel();
    let (mouse_ready_tx, mouse_ready_rx) = mpsc::channel();
    let mouse_thread = thread::Builder::new()
        .name("souris".into())
        .spawn(move || mouse_loop(mouse_rx, mouse_ready_tx))
        .unwrap_or_else(|error| {
            tell(&format!("Impossible de démarrer.\n\n{error}"));
            std::process::exit(1);
        });

    let state = AppState {
        pin: pin.clone(),
        mouse_tx,
        security: std::sync::Arc::new(Mutex::new(Security {
            failures: HashMap::new(),
            bans: HashMap::new(),
        })),
    };

    let (server_ready_tx, server_ready_rx) = mpsc::channel();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let port = opts.port;
    let server_thread = thread::Builder::new()
        .name("serveur".into())
        .spawn(move || serve(state, port, server_ready_tx, shutdown_rx))
        .unwrap_or_else(|error| {
            tell(&format!("Impossible de démarrer.\n\n{error}"));
            std::process::exit(1);
        });

    if let Err(error) = mouse_ready_rx
        .recv()
        .unwrap_or_else(|_| Err("Le contrôle de la souris s'est arrêté.".into()))
    {
        tell(&format!(
            "Impossible de prendre le contrôle de la souris.\n\n{error}"
        ));
        let _ = shutdown_tx.send(());
        let _ = server_thread.join();
        let _ = mouse_thread.join();
        std::process::exit(1);
    }

    if let Err(error) = server_ready_rx
        .recv()
        .unwrap_or_else(|_| Err("Le serveur s'est arrêté.".into()))
    {
        tell(&error);
        let _ = shutdown_tx.send(());
        let _ = server_thread.join();
        let _ = mouse_thread.join();
        std::process::exit(1);
    }

    #[cfg(windows)]
    window::run(&pin, &links);
    #[cfg(not(windows))]
    {
        println!("PhoneMouse");
        println!("Code : {pin}");
        for link in &links {
            println!("{}  {}", link.name, link.url);
        }
        println!("Entrée pour quitter.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }

    let _ = shutdown_tx.send(());
    let _ = server_thread.join();
    let _ = mouse_thread.join();
}

fn serve(
    state: AppState,
    port: u16,
    ready: Sender<Result<(), String>>,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready.send(Err(format!("Impossible de démarrer.\n\n{error}")));
            return;
        }
    };

    runtime.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(listener) => listener,
            Err(error) => {
                let _ = ready.send(Err(format!(
                    "Impossible d'écouter sur le port {port}.\n\n{error}\n\nFerme l'autre PhoneMouse, ou lance PhoneMouse.exe {}.",
                    port.saturating_add(1)
                )));
                return;
            }
        };
        let _ = ready.send(Ok(()));

        let app = Router::new()
            .route("/", get(index))
            .route("/ws", get(ws_handler))
            .with_state(state);
        let server = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = shutdown.await;
        });
        let _ = server.await;
    });
}

fn parse_args() -> Opts {
    let mut opts = Opts {
        port: DEFAULT_PORT,
        new_pin: false,
    };

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--new-pin" => opts.new_pin = true,
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => match other.parse::<u16>() {
                Ok(port) if port != 0 => opts.port = port,
                _ => {
                    tell(&format!("Argument inconnu : {other}"));
                    print_help();
                    std::process::exit(1);
                }
            },
        }
    }

    opts
}

fn print_help() {
    tell(
        "\
PhoneMouse — trackpad du téléphone vers la souris du PC, sur le réseau local.

Usage :
  PhoneMouse.exe [port] [--new-pin]

  port       Port d'écoute (défaut 8787)
  --new-pin  Génère un nouveau code à 4 chiffres

Ferme la fenêtre pour quitter.",
    );
}

fn tell(text: &str) {
    #[cfg(windows)]
    window::message_box(text, true);
    #[cfg(not(windows))]
    eprintln!("{text}");
}

async fn index() -> Html<&'static str> {
    Html(include_str!("trackpad.html"))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| handle_socket(socket, addr.ip(), state))
}

async fn handle_socket(mut socket: WebSocket, ip: IpAddr, state: AppState) {
    if let Some(wait) = ban_remaining(&state, ip) {
        let secs = wait.as_secs().max(1);
        let _ = socket
            .send(Message::Text(
                format!(r#"{{"ok":false,"retry":{secs}}}"#).into(),
            ))
            .await;
        return;
    }

    let mut authed = false;
    let mut announced = false;
    let mut held = [false; 3];
    let mut acc = Accum::default();

    while let Some(Ok(message)) = socket.recv().await {
        let Message::Text(text) = message else {
            continue;
        };
        if text.len() > 1024 {
            continue;
        }

        let Ok(command) = serde_json::from_str::<ClientMsg>(&text) else {
            continue;
        };

        if !authed {
            let ClientMsg::Auth { pin } = command else {
                continue;
            };
            if pins_match(&pin, &state.pin) {
                clear_failures(&state, ip);
                authed = true;
                if socket
                    .send(Message::Text(r#"{"ok":true}"#.into()))
                    .await
                    .is_err()
                {
                    break;
                }
                phone_connected(1);
                announced = true;
            } else {
                let banned = register_failure(&state, ip);
                let payload = if banned {
                    format!(r#"{{"ok":false,"retry":{}}}"#, BAN_DURATION.as_secs())
                } else {
                    r#"{"ok":false}"#.to_string()
                };
                let _ = socket.send(Message::Text(payload.into())).await;
                tokio::time::sleep(Duration::from_millis(400)).await;
                if banned {
                    break;
                }
            }
            continue;
        }

        match command {
            ClientMsg::Move { x, y } => {
                if let Some((px, py)) = queue_move(&mut acc, x, y) {
                    let _ = state.mouse_tx.send(MouseCmd::Move(px, py));
                }
            }
            ClientMsg::Down { btn } => {
                held[btn.index()] = true;
                let _ = state
                    .mouse_tx
                    .send(MouseCmd::Button(btn.enigo(), Direction::Press));
            }
            ClientMsg::Up { btn } => {
                held[btn.index()] = false;
                let _ = state
                    .mouse_tx
                    .send(MouseCmd::Button(btn.enigo(), Direction::Release));
            }
            ClientMsg::Click { btn } => {
                held[btn.index()] = false;
                let _ = state
                    .mouse_tx
                    .send(MouseCmd::Button(btn.enigo(), Direction::Click));
            }
            ClientMsg::Scroll { x, y } => {
                let x = x.clamp(-8, 8);
                let y = y.clamp(-8, 8);
                if x != 0 {
                    let _ = state.mouse_tx.send(MouseCmd::Scroll(x, Axis::Horizontal));
                }
                if y != 0 {
                    let _ = state.mouse_tx.send(MouseCmd::Scroll(y, Axis::Vertical));
                }
            }
            ClientMsg::Auth { .. } | ClientMsg::Ping => {}
        }
    }

    for (index, down) in held.into_iter().enumerate() {
        if down {
            let _ = state.mouse_tx.send(MouseCmd::Button(
                Btn::from_index(index).enigo(),
                Direction::Release,
            ));
        }
    }
    if announced {
        phone_connected(-1);
    }
}

fn phone_connected(delta: i32) {
    #[cfg(windows)]
    window::phone_connected(delta);
    #[cfg(not(windows))]
    let _ = delta;
}

fn mouse_loop(rx: Receiver<MouseCmd>, ready: Sender<Result<(), String>>) {
    let enigo = match Enigo::new(&Settings::default()) {
        Ok(enigo) => enigo,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let mut guard = MouseGuard {
        enigo,
        held: [false; 3],
    };

    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while let Ok(next) = rx.try_recv() {
            batch.push(next);
        }

        let mut move_x = 0i32;
        let mut move_y = 0i32;
        for command in batch {
            match command {
                MouseCmd::Move(x, y) => {
                    move_x = move_x.saturating_add(x);
                    move_y = move_y.saturating_add(y);
                }
                other => {
                    flush_move(&mut guard, &mut move_x, &mut move_y);
                    match other {
                        MouseCmd::Button(button, direction) => {
                            apply_button(&mut guard, button, direction);
                        }
                        MouseCmd::Scroll(length, axis) => {
                            let length = length.clamp(-20, 20);
                            if length != 0 {
                                let _ = guard.enigo.scroll(length, axis);
                            }
                        }
                        MouseCmd::Move(_, _) => {}
                    }
                }
            }
        }
        flush_move(&mut guard, &mut move_x, &mut move_y);
    }
}

struct MouseGuard {
    enigo: Enigo,
    held: [bool; 3],
}

impl Drop for MouseGuard {
    fn drop(&mut self) {
        for index in 0..3 {
            if self.held[index] {
                let _ = self
                    .enigo
                    .button(Btn::from_index(index).enigo(), Direction::Release);
            }
        }
    }
}

fn flush_move(guard: &mut MouseGuard, x: &mut i32, y: &mut i32) {
    if *x == 0 && *y == 0 {
        return;
    }
    let _ = guard.enigo.move_mouse(*x, *y, Coordinate::Rel);
    *x = 0;
    *y = 0;
}

fn apply_button(guard: &mut MouseGuard, button: Button, direction: Direction) {
    let Some(index) = button_slot(button) else {
        let _ = guard.enigo.button(button, direction);
        return;
    };
    let held = &mut guard.held[index];
    match direction {
        Direction::Press => {
            if !*held && guard.enigo.button(button, Direction::Press).is_ok() {
                *held = true;
            }
        }
        Direction::Release => {
            if *held {
                let _ = guard.enigo.button(button, Direction::Release);
                *held = false;
            }
        }
        Direction::Click => {
            if *held {
                let _ = guard.enigo.button(button, Direction::Release);
                *held = false;
            }
            let _ = guard.enigo.button(button, Direction::Click);
        }
    }
}

fn button_slot(button: Button) -> Option<usize> {
    match button {
        Button::Left => Some(0),
        Button::Right => Some(1),
        Button::Middle => Some(2),
        _ => None,
    }
}

impl Btn {
    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Middle => 2,
        }
    }

    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Right,
            2 => Self::Middle,
            _ => Self::Left,
        }
    }

    fn enigo(self) -> Button {
        match self {
            Self::Left => Button::Left,
            Self::Right => Button::Right,
            Self::Middle => Button::Middle,
        }
    }
}

fn queue_move(acc: &mut Accum, x: f32, y: f32) -> Option<(i32, i32)> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    acc.x += x.clamp(-1200.0, 1200.0);
    acc.y += y.clamp(-1200.0, 1200.0);
    let px = acc.x.trunc() as i32;
    let py = acc.y.trunc() as i32;
    if px == 0 && py == 0 {
        return None;
    }
    acc.x -= px as f32;
    acc.y -= py as f32;
    Some((px, py))
}

fn pins_match(given: &str, expected: &str) -> bool {
    let given = given.as_bytes();
    let expected = expected.as_bytes();
    if given.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in given.iter().zip(expected) {
        diff |= left ^ right;
    }
    diff == 0
}

fn ban_remaining(state: &AppState, ip: IpAddr) -> Option<Duration> {
    let mut security = state.security.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    security.bans.retain(|_, until| *until > now);
    security
        .bans
        .get(&ip)
        .map(|until| until.saturating_duration_since(now))
}

fn register_failure(state: &AppState, ip: IpAddr) -> bool {
    let mut security = state.security.lock().unwrap_or_else(|e| e.into_inner());
    let count = security.failures.entry(ip).or_insert(0);
    *count = count.saturating_add(1);
    if *count >= MAX_ATTEMPTS {
        security.failures.remove(&ip);
        security.bans.insert(ip, Instant::now() + BAN_DURATION);
        true
    } else {
        false
    }
}

fn clear_failures(state: &AppState, ip: IpAddr) {
    let mut security = state.security.lock().unwrap_or_else(|e| e.into_inner());
    security.failures.remove(&ip);
}

fn load_or_create_pin(force_new: bool) -> String {
    let path = pin_path();
    if !force_new {
        if let Ok(saved) = std::fs::read_to_string(&path) {
            let saved = saved.trim();
            if saved.len() == 4 && saved.chars().all(|c| c.is_ascii_digit()) {
                return saved.to_string();
            }
        }
    }

    let pin = random_pin();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(&path, &pin) {
        tell(&format!(
            "Le code n'a pas pu être enregistré dans {}.\n\n{error}",
            path.display()
        ));
    }
    pin
}

fn pin_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join("phonemouse-pin.txt")))
        .unwrap_or_else(|| std::path::PathBuf::from("phonemouse-pin.txt"))
}

fn random_pin() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut hasher = RandomState::new().build_hasher();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    hasher.write_u64(nanos);
    format!("{:04}", hasher.finish() % 10000)
}

fn lan_links(port: u16) -> Vec<LanLink> {
    let mut physical = Vec::new();
    let mut virtual_ifaces = Vec::new();

    if let Ok(interfaces) = local_ip_address::list_afinet_netifas() {
        for (name, ip) in interfaces {
            let IpAddr::V4(ipv4) = ip else {
                continue;
            };
            if !useful_ipv4(ipv4) {
                continue;
            }
            let link = LanLink {
                name,
                url: format!("http://{ipv4}:{port}"),
            };
            if looks_virtual(&link.name) {
                virtual_ifaces.push(link);
            } else {
                physical.push(link);
            }
        }
    }

    if physical.is_empty() {
        physical.append(&mut virtual_ifaces);
    }
    physical.sort_by(|left, right| left.url.cmp(&right.url));
    physical.dedup_by(|left, right| left.url == right.url);
    physical
}

fn useful_ipv4(ip: Ipv4Addr) -> bool {
    !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_unspecified()
        && !ip.is_broadcast()
        && !ip.is_multicast()
}

fn looks_virtual(name: &str) -> bool {
    let name = name.to_lowercase();
    [
        "virtual",
        "vethernet",
        "vmware",
        "hyper-v",
        "wsl",
        "loopback",
        "bluetooth",
        "tunnel",
        "vpn",
        "tap",
        "tun",
        "docker",
        "vbox",
        "tailscale",
        "zerotier",
    ]
    .iter()
    .any(|needle| name.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_move_accumulates() {
        let mut acc = Accum::default();
        assert_eq!(queue_move(&mut acc, 0.6, -0.4), None);
        assert_eq!(queue_move(&mut acc, 0.6, -0.9), Some((1, -1)));
        assert!((acc.x - 0.2).abs() < 0.001);
        assert!((acc.y + 0.3).abs() < 0.001);
    }

    #[test]
    fn rejects_non_finite_move() {
        let mut acc = Accum::default();
        assert_eq!(queue_move(&mut acc, f32::NAN, 1.0), None);
        assert_eq!(queue_move(&mut acc, 1.0, f32::INFINITY), None);
        assert_eq!(acc.x, 0.0);
        assert_eq!(acc.y, 0.0);
    }

    #[test]
    fn pin_compare_requires_exact_match() {
        assert!(pins_match("1234", "1234"));
        assert!(!pins_match("1235", "1234"));
        assert!(!pins_match("123", "1234"));
        assert!(!pins_match("12345", "1234"));
    }
}
