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
    pub apps: Vec<App>,
}

/// One [Apps] entry: an exe, what to show for it, and the `-` title rules listed under it.
pub struct App {
    /// Lowercase exe name
    pub exe: String,
    /// Empty means the exe is only reported when one of its title rules matches
    pub label: String,
    pub info: String,
    /// (text found anywhere in the window title, in lowercase, label, what it is), checked in
    /// order. An empty label keeps the app's.
    pub titles: Vec<(String, String, String)>,
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
            interval: 5,
            share_uptime: true,
            share_load: true,
            share_input: true,
            share_mouse: true,
            share_apps: true,
            mouse_dpi: 800.0,
            apps: DEFAULT_APPS
                .iter()
                .map(|(exe, label, info)| App {
                    exe: exe.to_string(),
                    label: label.to_string(),
                    info: info.to_string(),
                    titles: Vec::new(),
                })
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
            // A line starting with - is a title rule for the app above it
            let (rule, line) = match line.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim().to_lowercase(), value.trim());

            if section == "apps" {
                let (label, info) = value.split_once('|').unwrap_or((value, ""));
                let (label, info) = (clip(label.trim(), 40), clip(info.trim(), 80));
                if key.is_empty() {
                    continue;
                }
                if !rule {
                    c.apps.push(App { exe: key, label, info, titles: Vec::new() });
                } else if let Some(app) = c.apps.last_mut() {
                    app.titles.push((key, label, info));
                }
                continue;
            }
            match key.as_str() {
                "endpoint" if value.starts_with("https://") => c.endpoint = value.to_string(),
                "intervalseconds" => c.interval = value.parse::<u32>().unwrap_or(c.interval).clamp(5, 600),
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
            "; Seconds between updates while you're at the PC (5 to 600). After 5 minutes away it".into(),
            "; drops to one a minute until you're back.".into(),
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
            ";".into(),
            "; Lines starting with - under an app change what's shown when its window title contains".into(),
            "; that text (anywhere, ignoring case). They're checked top to bottom and the first match".into(),
            "; wins; with no match the app's own line is used. Leave a name out to keep the app's:".into(),
            ";   zen.exe=Zen Browser | browsing".into(),
            ";   - youtube = YouTube | watching videos".into(),
            ";   - netflix = | watching a show".into(),
            "; Leave the app's own name empty (zen.exe=) to only report it when a rule matches.".into(),
            "; The title itself is never sent, only the names and descriptions written here.".into(),
        ];
        for app in &self.apps {
            lines.push(format!("{}={} | {}", app.exe, app.label, app.info));
            lines.extend(app.titles.iter().map(|(text, label, info)| format!("- {text} = {label} | {info}")));
        }
        let _ = std::fs::write(Self::path(), lines.join("\r\n") + "\r\n");
    }

    /// The label and description for the focused window, if its exe is on the list: the first of
    /// the app's title rules found in the window title, or else the app's own line.
    pub fn app_for(&self, exe: &str, title: &str) -> Option<(String, String)> {
        let app = self.apps.iter().find(|app| app.exe.eq_ignore_ascii_case(exe))?;
        let title = title.to_lowercase();
        let (label, info) = match app.titles.iter().find(|(text, _, _)| title.contains(text.as_str())) {
            Some((_, label, info)) => (if label.is_empty() { &app.label } else { label }, info),
            None => (&app.label, &app.info),
        };
        (!label.is_empty()).then(|| (label.clone(), info.clone()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn found(label: &str, info: &str) -> Option<(String, String)> {
        Some((label.to_string(), info.to_string()))
    }

    fn browser(label: &str) -> App {
        App {
            exe: "zen.exe".into(),
            label: label.into(),
            info: "browsing".into(),
            titles: vec![
                ("youtube".into(), "YouTube".into(), "watching videos".into()),
                ("netflix".into(), String::new(), "watching a show".into()),
                ("tube".into(), "Tube".into(), "only reached when youtube isn't".into()),
            ],
        }
    }

    #[test]
    fn title_rules_apply_in_order_then_fall_back_to_the_app() {
        let c = Config { apps: vec![browser("Zen Browser")], ..Config::default() };
        assert_eq!(c.app_for("Zen.exe", "YouTube — Zen Browser"), found("YouTube", "watching videos"));
        assert_eq!(c.app_for("zen.exe", "netflix — Zen Browser"), found("Zen Browser", "watching a show"));
        assert_eq!(c.app_for("zen.exe", "Gmail — Zen Browser"), found("Zen Browser", "browsing"));
        assert_eq!(c.app_for("chrome.exe", "YouTube - Google Chrome"), None);
    }

    #[test]
    fn an_app_without_a_name_only_shows_on_a_rule() {
        let c = Config { apps: vec![browser("")], ..Config::default() };
        assert_eq!(c.app_for("zen.exe", "YouTube — Zen Browser"), found("YouTube", "watching videos"));
        assert_eq!(c.app_for("zen.exe", "Netflix — Zen Browser"), None);
        assert_eq!(c.app_for("zen.exe", "Gmail — Zen Browser"), None);
    }
}
