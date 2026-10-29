use crate::util::{exe_dir, local_day};

/// Today's input counts. Saved with every update so a restart doesn't lose them.
pub struct Counters {
    pub day: String,
    pub clicks: u64,
    pub keys: u64,
    pub scrolls: u64,
    /// Raw mouse movement in device counts; divided by the DPI to get inches
    pub mickeys: f64,
    pub paused: bool,
}

fn path() -> std::path::PathBuf {
    exe_dir().join("pc-stats.state")
}

impl Counters {
    pub fn load() -> Counters {
        let mut c = Counters { day: local_day(), clicks: 0, keys: 0, scrolls: 0, mickeys: 0.0, paused: false };
        let Ok(text) = std::fs::read_to_string(path()) else { return c };
        let fields: Vec<&str> = text.split_whitespace().collect();
        // "day clicks keys scrolls mickeys paused"
        if let [day, clicks, keys, scrolls, mickeys, paused] = fields[..] {
            c.paused = paused == "1";
            if day == c.day {
                c.clicks = clicks.parse().unwrap_or(0);
                c.keys = keys.parse().unwrap_or(0);
                c.scrolls = scrolls.parse().unwrap_or(0);
                c.mickeys = mickeys.parse().unwrap_or(0.0);
            }
        }
        c
    }

    pub fn save(&self) {
        let text = format!(
            "{} {} {} {} {} {}",
            self.day,
            self.clicks,
            self.keys,
            self.scrolls,
            self.mickeys.round(),
            if self.paused { 1 } else { 0 }
        );
        let _ = std::fs::write(path(), text);
    }

    /// Starts the counts over when the date changes.
    pub fn roll_over(&mut self) {
        let today = local_day();
        if today != self.day {
            *self = Counters { day: today, clicks: 0, keys: 0, scrolls: 0, mickeys: 0.0, paused: self.paused };
        }
    }
}
