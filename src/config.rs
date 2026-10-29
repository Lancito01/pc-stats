use std::path::PathBuf;

use crate::util::exe_dir;

/// pc-stats.ini next to the exe. It's read again before every update, so edits apply within a minute.
pub struct Config {
    pub endpoint: String,
    pub interval: u32,
    pub share_uptime: bool,
    pub share_load: bool,
    pub share_input: bool,
    pub share_mouse: bool,
    pub share_apps: bool,
    pub mouse_dpi: f64,
    /// (exe name in lowercase, label, what it is)
    pub apps: Vec<(String, String, String)>,
}

const DEFAULT_APPS: &[(&str, &str, &str)] = &[
    ("code.exe", "VS Code", "code editor"),
    ("devenv.exe", "Visual Studio", "IDE for C# and C++"),
    ("obs64.exe", "OBS", "recording or streaming"),
    ("blender.exe", "Blender", "3D modelling and animation"),
];

impl Default for Config {
    fn default() -> Self {
        Config {
            // Nothing is sent until this is set
            endpoint: String::new(),
            interval: 60,
            share_uptime: true,
            share_load: true,
            share_input: true,
            share_mouse: true,
            share_apps: true,
            mouse_dpi: 800.0,
            apps: DEFAULT_APPS
                .iter()
                .map(|(exe, label, info)| (exe.to_string(), label.to_string(), info.to_string()))
                .collect(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        exe_dir().join("pc-stats.ini")
    }

    pub fn load() -> Config {
        let Ok(text) = std::fs::read_to_string(Self::path()) else {
            let config = Config::default();
            config.save();
            return config;
        };

        let mut c = Config { apps: Vec::new(), ..Config::default() };
        let mut section = String::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                section = line.trim_matches(|c| c == '[' || c == ']').to_ascii_lowercase();
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());

            if section == "apps" {
                let (label, info) = value.split_once('|').unwrap_or((value, ""));
                let (label, info) = (clip(label.trim(), 40), clip(info.trim(), 80));
                if !label.is_empty() {
                    c.apps.push((key, label, info));
                }
                continue;
            }
            match key.as_str() {
                "endpoint" if value.starts_with("https://") => c.endpoint = value.to_string(),
                "intervalseconds" => c.interval = value.parse::<u32>().unwrap_or(c.interval).clamp(30, 600),
                "uptime" => c.share_uptime = flag(value, c.share_uptime),
                "load" => c.share_load = flag(value, c.share_load),
                "input" => c.share_input = flag(value, c.share_input),
                "mousedistance" => c.share_mouse = flag(value, c.share_mouse),
                "apps" => c.share_apps = flag(value, c.share_apps),
                "mousedpi" => c.mouse_dpi = value.parse::<f64>().unwrap_or(c.mouse_dpi).clamp(100.0, 50_000.0),
                _ => {}
            }
        }
        c
    }

    fn save(&self) {
        let on = |b: bool| if b { 1 } else { 0 };
        let mut lines = vec![
            "; PC Stats settings. Changes apply with the next update (within a minute).".to_string(),
            "[Sharing]".into(),
            "; Where updates go: your site's ingest address, e.g. https://example.com/api/pc-stats/ingest".into(),
            "; Nothing is sent while this is empty. Only https:// addresses are accepted.".into(),
            format!("Endpoint={}", self.endpoint),
            "; Seconds between updates (30 to 600)".into(),
            format!("IntervalSeconds={}", self.interval),
            "; 1 = share it, 0 = never send it".into(),
            "; How long the PC has been on, and whether I've used it in the last 5 minutes".into(),
            format!("Uptime={}", on(self.share_uptime)),
            "; CPU, GPU and RAM use".into(),
            format!("Load={}", on(self.share_load)),
            "; Clicks, key presses and scrolls today. Counts only: which keys is never recorded.".into(),
            format!("Input={}", on(self.share_input)),
            "; How far the mouse moved today".into(),
            format!("MouseDistance={}", on(self.share_mouse)),
            "; The app I'm in, but only if it's listed under [Apps] below".into(),
            format!("Apps={}", on(self.share_apps)),
            "; The mouse's DPI (CPI), used to turn movement into distance".into(),
            format!("MouseDpi={}", self.mouse_dpi),
            String::new(),
            "[Apps]".into(),
            "; exe name = name to show | what it is. Only these are ever reported, and only while".into(),
            "; focused. Anything not listed here (browsers, chats...) is never mentioned.".into(),
        ];
        lines.extend(self.apps.iter().map(|(exe, label, info)| format!("{exe}={label} | {info}")));
        let _ = std::fs::write(Self::path(), lines.join("\r\n") + "\r\n");
    }

    /// The label and description for a focused exe, if it's on the list.
    pub fn app_for(&self, exe: &str) -> Option<(String, String)> {
        self.apps
            .iter()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(exe))
            .map(|(_, label, info)| (label.clone(), info.clone()))
    }
}

fn flag(value: &str, fallback: bool) -> bool {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => fallback,
    }
}

/// Keeps labels within what the site accepts (it rejects longer ones outright).
fn clip(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}
