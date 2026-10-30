// PC Stats: shares a few live stats from this PC with your website, from the system tray.
//
// Every update is signed with a key Windows creates on this PC and won't let leave it, so
// the site only accepts updates that really come from here. What gets sent is listed in
// pc-stats.ini, and "Show what gets sent" in the tray menu opens the last update as JSON.
// The site's side is in server/, and PROTOCOL.md describes what goes over the wire.
//
// Build: cargo build --release

#![windows_subsystem = "windows"]

mod config;
mod input;
mod metrics;
mod net;
mod signer;
mod state;
mod tray;
mod util;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer, MessageBoxW,
    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetTimer, TranslateMessage, MB_ICONERROR,
    MSG, PBT_APMRESUMEAUTOMATIC, SW_SHOWNORMAL, WM_APP, WM_DESTROY, WM_ENDSESSION, WM_INPUT, WM_LBUTTONUP,
    WM_POWERBROADCAST, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};

use config::Config;
use metrics::Load;
use signer::Signer;
use state::Counters;
use tray::{Item, Tray, WM_TRAY};
use util::{json_string, local_day, log, unix_seconds, wide};

const WM_SENT: u32 = WM_APP + 2;
const SAMPLE_TIMER: usize = 1;
const SEND_TIMER: usize = 2;
const SOON_TIMER: usize = 3;
// Often enough that every update has a fresh CPU/GPU reading, even at the 5 s minimum
const SAMPLE_MS: u32 = 2_000;
// While away, one update a minute is plenty: the site shows "away" either way
const AWAY_INTERVAL_SECONDS: u64 = 60;
const SAVE_EVERY_SECONDS: u64 = 60;
// Matches the site: no input for this long counts as away, and apps aren't reported then
const AWAY_SECONDS: u64 = 300;

// Tray menu ids
const PAUSE: u32 = 1;
const STARTUP: u32 = 2;
const COPY_KEY: u32 = 3;
const SHOW_SENT: u32 = 4;
const SETTINGS: u32 = 5;
const QUIT: u32 = 6;

struct App {
    hwnd: HWND,
    config: Config,
    counters: Counters,
    keys_down: [bool; 256],
    load: Load,
    /// Samples per listed app (label, what it is) focused since the last update; unlisted apps are never counted
    app_samples: HashMap<(String, String), u32>,
    signer: Signer,
    tray: Tray,
    sending: Arc<AtomicBool>,
    status: String,
    last_body: Option<String>,
    interval: u32,
    last_sent: Option<Instant>,
    last_saved: Instant,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Runs `f` with the app state, unless it's already in use further up the stack (a modal
/// loop like the tray menu can deliver messages while a handler is still running).
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut app| app.as_mut().map(f)))
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--self-test") => return self_test(std::env::args().nth(2)),
        Some("--public-key") => return print_public_key(),
        _ => {}
    }

    unsafe {
        CreateMutexW(std::ptr::null(), 1, wide(r"Local\PcStats.SingleInstance").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
    }
    log::init();

    let signer = match Signer::open_or_create() {
        Ok(signer) => signer,
        Err(error) => return fail(&format!("PC Stats {error}.")),
    };
    if let Ok(key) = signer.public_jwk() {
        let _ = std::fs::write(util::exe_dir().join("public-key.txt"), &key);
    }

    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("PcStatsWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        // Never shown: it's only here to receive input, timer and tray messages
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("PC Stats").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return fail("PC Stats couldn't start.");
        }
        if !input::register(hwnd) {
            log::write("couldn't register for raw input; clicks and keys won't be counted");
        }

        let first_run = !Config::path().exists();
        let config = Config::load();
        let interval = config.interval;
        // A fresh install has nowhere to send updates yet, so start by showing the settings
        if first_run && config.endpoint.is_empty() {
            open(&Config::path().to_string_lossy());
        }
        let counters = Counters::load();
        let mut app = App {
            hwnd,
            config,
            counters,
            keys_down: [false; 256],
            load: Load::new(),
            app_samples: HashMap::new(),
            signer,
            tray: Tray::add(hwnd),
            sending: Arc::new(AtomicBool::new(false)),
            status: "Starting up".into(),
            last_body: None,
            interval,
            last_sent: None,
            last_saved: Instant::now(),
        };
        refresh_tray(&mut app);
        APP.with(|cell| *cell.borrow_mut() = Some(app));

        SetTimer(hwnd, SAMPLE_TIMER, SAMPLE_MS, None);
        SetTimer(hwnd, SEND_TIMER, interval * 1000, None);
        // The first update waits for a couple of CPU samples
        SetTimer(hwnd, SOON_TIMER, 12_000, None);
        log::write("started");

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    static TASKBAR_CREATED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    let taskbar_created = *TASKBAR_CREATED.get_or_init(|| RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));

    match msg {
        WM_INPUT => {
            with_app(|app| input::handle(lparam, &mut app.counters, &mut app.keys_down));
            // Raw input still has to go through DefWindowProc so Windows can clean up
        }
        WM_TIMER => {
            match wparam {
                SAMPLE_TIMER => with_app(sample),
                SEND_TIMER => with_app(send),
                SOON_TIMER => {
                    KillTimer(hwnd, SOON_TIMER);
                    with_app(send)
                }
                _ => None,
            };
            return 0;
        }
        WM_TRAY => {
            if matches!(lparam as u32, WM_LBUTTONUP | WM_RBUTTONUP) {
                show_menu(hwnd);
            }
            return 0;
        }
        WM_SENT => {
            with_app(|app| sent(app, wparam as u32));
            return 0;
        }
        WM_POWERBROADCAST if wparam as u32 == PBT_APMRESUMEAUTOMATIC => {
            // Back from sleep: say so soon rather than at the next interval
            SetTimer(hwnd, SOON_TIMER, 10_000, None);
        }
        WM_ENDSESSION if wparam != 0 => {
            with_app(|app| app.counters.save());
        }
        WM_DESTROY => {
            with_app(|app| {
                app.counters.save();
                app.tray.remove();
            });
            PostQuitMessage(0);
            return 0;
        }
        _ if msg == taskbar_created => {
            with_app(|app| app.tray.show());
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn sample(app: &mut App) {
    app.counters.roll_over();
    app.load.sample();
    if app.config.share_apps && metrics::idle_seconds() < 60 {
        if let Some(found) = metrics::focused_window().and_then(|(exe, title)| app.config.app_for(&exe, &title)) {
            *app.app_samples.entry(found).or_default() += 1;
        }
    }
}

/// The JSON body for one update. Only what pc-stats.ini allows goes in.
fn snapshot(config: &Config, counters: &Counters, load: (Option<u8>, Option<u8>), app: Option<(String, String)>, ts: u64) -> String {
    let idle = metrics::idle_seconds();
    // Seconds until the next update, so the site can glide between the two. While away the
    // send timer still ticks every interval, but only one tick a minute actually sends.
    let interval = u64::from(config.interval);
    let next = if idle >= AWAY_SECONDS { AWAY_INTERVAL_SECONDS.div_ceil(interval) * interval } else { interval };
    let mut fields = vec!["\"v\":1".to_string(), format!("\"ts\":{ts}"), format!("\"interval\":{}", next.min(600))];
    if config.share_uptime {
        fields.push(format!("\"uptime\":{}", metrics::uptime_seconds()));
        fields.push(format!("\"idle\":{idle}"));
    }
    if config.share_load {
        if let Some(cpu) = load.0 {
            fields.push(format!("\"cpu\":{cpu}"));
        }
        if let Some(gpu) = load.1 {
            fields.push(format!("\"gpu\":{gpu}"));
        }
        if let Some((used, total)) = metrics::memory() {
            fields.push(format!("\"ramUsed\":{used:.1},\"ramTotal\":{total:.1}"));
        }
    }
    if config.share_input || config.share_mouse {
        fields.push(format!("\"day\":{}", json_string(&counters.day)));
    }
    if config.share_input {
        fields.push(format!("\"clicks\":{},\"keys\":{},\"scrolls\":{}", counters.clicks, counters.keys, counters.scrolls));
    }
    if config.share_mouse {
        let metres = counters.mickeys / config.mouse_dpi * 0.0254;
        fields.push(format!("\"mouse\":{metres:.2}"));
    }
    if config.share_apps && idle < AWAY_SECONDS {
        if let Some((label, info)) = app {
            fields.push(format!("\"app\":{}", json_string(&label)));
            if !info.is_empty() {
                fields.push(format!("\"appInfo\":{}", json_string(&info)));
            }
        }
    }
    format!("{{{}}}", fields.join(","))
}

fn send(app: &mut App) {
    // Settings are re-read every time, so edits to pc-stats.ini apply from the next update
    app.config = Config::load();
    if app.config.interval != app.interval {
        app.interval = app.config.interval;
        unsafe { SetTimer(app.hwnd, SEND_TIMER, app.interval * 1000, None) };
    }
    app.counters.roll_over();
    if app.last_saved.elapsed().as_secs() >= SAVE_EVERY_SECONDS {
        app.counters.save();
        app.last_saved = Instant::now();
    }
    let away = metrics::idle_seconds() >= AWAY_SECONDS;
    if away && app.last_sent.is_some_and(|sent| sent.elapsed().as_secs() < AWAY_INTERVAL_SECONDS) {
        return;
    }

    let load = app.load.take();
    let top_app = app.app_samples.drain().max_by_key(|(_, n)| *n).map(|(found, _)| found);
    if app.counters.paused || app.sending.load(Ordering::SeqCst) {
        return;
    }
    let Some((host, _, _)) = net::parse(&app.config.endpoint) else {
        app.status = "Not sharing yet: set Endpoint in pc-stats.ini".into();
        return refresh_tray(app);
    };

    let ts = unix_seconds();
    let body = snapshot(&app.config, &app.counters, load, top_app, ts);
    let signature = match app.signer.sign(&host, ts, &body) {
        Ok(signature) => signature,
        Err(error) => {
            log::write(&error);
            app.status = format!("Couldn't sign the update: {error}");
            return refresh_tray(app);
        }
    };
    app.last_body = Some(body.clone());

    let endpoint = app.config.endpoint.clone();
    let sending = app.sending.clone();
    let hwnd = app.hwnd as isize;
    app.last_sent = Some(Instant::now());
    sending.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        let status = net::post(&endpoint, ts, &body, &signature).unwrap_or_else(|error| {
            log::write(&error);
            0
        });
        sending.store(false, Ordering::SeqCst);
        unsafe { PostMessageW(hwnd as HWND, WM_SENT, status as usize, 0) };
    });
}

fn sent(app: &mut App, status: u32) {
    let t = util::local_time();
    app.status = match status {
        204 => format!("Last update sent at {:02}:{:02}", t.wHour, t.wMinute),
        409 => return, // too soon after the previous one; the next update will go through
        401 => "The site rejected this PC's key. Is the public key set on Vercel?".into(),
        503 => "The site isn't set up for stats yet".into(),
        0 => "Couldn't reach the site".into(),
        other => format!("The site answered {other}"),
    };
    if status != 204 {
        log::write(&format!("update not accepted: {} ({status})", app.status));
    }
    refresh_tray(app);
}

fn refresh_tray(app: &mut App) {
    let tip = if app.counters.paused { "PC Stats: paused".to_string() } else { format!("PC Stats: {}", app.status) };
    let sharing = !app.counters.paused;
    app.tray.update(sharing, &tip);
}

fn show_menu(hwnd: HWND) {
    // Read what the menu needs, then let go of the app state while the menu is open
    let Some((status, paused)) = with_app(|app| {
        let status = if app.counters.paused { "Paused: nothing is being sent".to_string() } else { app.status.clone() };
        (status, app.counters.paused)
    }) else {
        return;
    };
    let starts_with_windows = startup::enabled();

    let chosen = tray::menu(
        hwnd,
        &[
            Item::Text(&status),
            Item::Separator,
            Item::Action { id: PAUSE, label: "Pause sharing", checked: paused },
            Item::Action { id: STARTUP, label: "Start with Windows", checked: starts_with_windows },
            Item::Separator,
            Item::Action { id: SHOW_SENT, label: "Show what gets sent", checked: false },
            Item::Action { id: COPY_KEY, label: "Copy public key", checked: false },
            Item::Action { id: SETTINGS, label: "Open settings", checked: false },
            Item::Separator,
            Item::Action { id: QUIT, label: "Quit", checked: false },
        ],
    );

    match chosen {
        PAUSE => {
            with_app(|app| {
                app.counters.paused = !app.counters.paused;
                app.counters.save();
                refresh_tray(app);
            });
        }
        STARTUP => startup::set(!starts_with_windows),
        COPY_KEY => {
            if let Some(Ok(key)) = with_app(|app| app.signer.public_jwk()) {
                copy_to_clipboard(hwnd, &key);
            }
        }
        SHOW_SENT => {
            let body = with_app(|app| app.last_body.clone()).flatten();
            let text = body.unwrap_or_else(|| "{\"note\":\"Nothing has been sent yet. The first update goes out a few seconds after start.\"}".into());
            let path = util::exe_dir().join("last-sent.json");
            if std::fs::write(&path, text).is_ok() {
                open(&path.to_string_lossy());
            }
        }
        SETTINGS => open(&Config::path().to_string_lossy()),
        QUIT => unsafe {
            DestroyWindow(hwnd);
        },
        _ => {}
    }
}

fn open(path: &str) {
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide(path).as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

fn copy_to_clipboard(hwnd: HWND, text: &str) {
    const CF_UNICODETEXT: u32 = 13;
    let data = wide(text);
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return;
        }
        EmptyClipboard();
        let memory = GlobalAlloc(GMEM_MOVEABLE, data.len() * 2);
        if !memory.is_null() {
            let target = GlobalLock(memory) as *mut u16;
            std::ptr::copy_nonoverlapping(data.as_ptr(), target, data.len());
            GlobalUnlock(memory);
            SetClipboardData(CF_UNICODETEXT, memory);
        }
        CloseClipboard();
    }
}

fn fail(message: &str) {
    log::write(message);
    unsafe { MessageBoxW(std::ptr::null_mut(), wide(message).as_ptr(), wide("PC Stats").as_ptr(), MB_ICONERROR) };
}

/// Lets `pc-stats.exe --flag` print to the terminal it was started from.
fn attach_console() {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

fn print_public_key() {
    attach_console();
    match Signer::open_or_create().and_then(|signer| signer.public_jwk()) {
        Ok(key) => println!("{key}"),
        Err(error) => eprintln!("PC Stats {error}."),
    }
}

/// Builds and signs one update with a throwaway key and prints it, without sending anything
/// or touching this PC's real key. Handy for checking what the app would send, and for
/// testing a server: it signs for the given endpoint's host (example.com by default).
fn self_test(endpoint: Option<String>) {
    attach_console();
    let endpoint = endpoint.unwrap_or_else(|| "https://example.com/api/pc-stats/ingest".into());
    let Some((host, _, _)) = net::parse(&endpoint) else {
        return eprintln!("PC Stats: {endpoint} isn't a valid https:// address.");
    };
    let mut load = Load::new();
    for _ in 0..2 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        load.sample();
    }
    let config = Config::default();
    let counters = Counters { day: local_day(), clicks: 0, keys: 0, scrolls: 0, mickeys: 0.0, paused: false };
    let app = metrics::focused_window().and_then(|(exe, title)| config.app_for(&exe, &title));
    let ts = unix_seconds();
    let body = snapshot(&config, &counters, load.take(), app, ts);

    let result = Signer::ephemeral().and_then(|signer| Ok((signer.sign(&host, ts, &body)?, signer.public_jwk()?)));
    match result {
        Ok((signature, key)) => println!(
            "{{\"host\":{},\"timestamp\":{ts},\"body\":{},\"signature\":\"{signature}\",\"publicKey\":{key}}}",
            json_string(&host),
            json_string(&body)
        ),
        Err(error) => eprintln!("PC Stats {error}."),
    }
}

mod startup {
    use super::*;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "PC Stats";

    fn open_run_key(access: u32) -> Option<HKEY> {
        let mut key: HKEY = std::ptr::null_mut();
        let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, access, &mut key) };
        (status == 0).then_some(key)
    }

    pub fn enabled() -> bool {
        let Some(key) = open_run_key(KEY_QUERY_VALUE) else { return false };
        let mut size = 0u32;
        let status = unsafe {
            RegQueryValueExW(key, wide(VALUE).as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), &mut size)
        };
        unsafe { RegCloseKey(key) };
        status == 0
    }

    pub fn set(enable: bool) {
        let Some(key) = open_run_key(KEY_SET_VALUE) else { return };
        unsafe {
            if enable {
                let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                let command = wide(&format!("\"{exe}\""));
                RegSetValueExW(key, wide(VALUE).as_ptr(), 0, REG_SZ, command.as_ptr() as *const u8, (command.len() * 2) as u32);
            } else {
                RegDeleteValueW(key, wide(VALUE).as_ptr());
            }
            RegCloseKey(key);
        }
    }
}
