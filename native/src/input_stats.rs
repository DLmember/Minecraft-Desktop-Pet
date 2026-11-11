//! Opt-in local aggregate input counts. Typed text and click coordinates are never stored.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};
use windows_sys::Win32::{Foundation::SYSTEMTIME, System::SystemInformation::GetLocalTime};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Range { Today, Last5Hours, Week, Month, AllTime }

#[derive(Default)]
pub struct Period {
    pub total: u64,
    pub keys: BTreeMap<String, u64>,
    pub hours: [u64; 24],
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Bucket {
    total: u64,
    keys: BTreeMap<String, u64>,
    hours: [u64; 24],
}

#[derive(Clone, Serialize, Deserialize)]
struct Day {
    ordinal: i64,
    year: u16,
    month: u16,
    bucket: Bucket,
}

#[derive(Clone, Serialize, Deserialize)]
struct Minute {
    utc_minute: u64,
    hour: u16,
    keys: BTreeMap<String, u64>,
}

#[derive(Serialize, Deserialize)]
struct Data {
    version: u32,
    enabled: bool,
    days: BTreeMap<String, Day>,
    minutes: BTreeMap<u64, Minute>,
    all_time: Bucket,
}
impl Default for Data {
    fn default() -> Self {
        Self { version: 1, enabled: false, days: BTreeMap::new(), minutes: BTreeMap::new(), all_time: Bucket::default() }
    }
}

pub struct Store {
    path: PathBuf,
    data: Data,
    pub enabled: bool,
    pub read_error: Option<String>,
    dirty: bool,
    last_pruned_minute: Option<u64>,
}

fn local_time() -> SYSTEMTIME {
    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut local) };
    local
}

// Days since a fixed Gregorian epoch; only relative differences matter.
fn ordinal(year: u16, month: u16, day: u16) -> i64 {
    let mut y = year as i64;
    let m = month as i64;
    if m <= 2 { y -= 1; }
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = m + if m > 2 { -3 } else { 9 };
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + (153 * mp + 2) / 5 + day as i64 - 1
}

fn utc_minute() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() / 60
}

fn increment(bucket: &mut Bucket, id: &str, hour: usize) {
    bucket.total = bucket.total.saturating_add(1);
    let value = bucket.keys.entry(id.to_owned()).or_default();
    *value = value.saturating_add(1);
    bucket.hours[hour] = bucket.hours[hour].saturating_add(1);
}

fn merge(period: &mut Period, bucket: &Bucket) {
    period.total = period.total.saturating_add(bucket.total);
    for (key, count) in &bucket.keys {
        let target = period.keys.entry(key.clone()).or_default();
        *target = target.saturating_add(*count);
    }
    for (hour, count) in bucket.hours.iter().enumerate() {
        period.hours[hour] = period.hours[hour].saturating_add(*count);
    }
}

impl Store {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("input-stats.json");
        let (data, read_error) = match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Data>(&bytes) {
                Ok(data) if data.version == 1 => (data, None),
                _ => (Data::default(), Some("统计文件损坏或版本不受支持，原文件已保留".to_string())),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Data::default(), None),
            Err(e) => (Data::default(), Some(format!("无法读取统计文件：{e}"))),
        };
        let enabled = data.enabled && read_error.is_none();
        Self { path, data, enabled, read_error, dirty: false, last_pruned_minute: None }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.read_error.is_some() { return; }
        self.enabled = enabled;
        self.data.enabled = enabled;
        self.dirty = true;
    }

    pub fn count(&mut self, id: &str) {
        if !self.enabled || self.read_error.is_some() || id.is_empty() || id.len() > 64 { return; }
        let now = local_time();
        if now.wMonth == 0 || now.wMonth > 12 || now.wDay == 0 || now.wDay > 31 || now.wHour >= 24 { return; }
        let date = format!("{:04}-{:02}-{:02}", now.wYear, now.wMonth, now.wDay);
        let today = ordinal(now.wYear, now.wMonth, now.wDay);
        let minute = utc_minute();
        let day = self.data.days.entry(date).or_insert_with(|| Day {
            ordinal: today, year: now.wYear, month: now.wMonth, bucket: Bucket::default(),
        });
        increment(&mut day.bucket, id, now.wHour as usize);
        increment(&mut self.data.all_time, id, now.wHour as usize);
        let bucket = self.data.minutes.entry(minute).or_insert_with(|| Minute {
            utc_minute: minute, hour: now.wHour, keys: BTreeMap::new(),
        });
        let value = bucket.keys.entry(id.to_owned()).or_default();
        *value = value.saturating_add(1);
        if self.last_pruned_minute != Some(minute) {
            self.data.days.retain(|_, day| day.ordinal >= today - 62);
            self.data.minutes.retain(|_, bucket| bucket.utc_minute >= minute.saturating_sub(360));
            self.last_pruned_minute = Some(minute);
        }
        self.dirty = true;
    }

    pub fn period(&self, range: Range) -> Period {
        let now = local_time();
        let today = ordinal(now.wYear, now.wMonth, now.wDay);
        let mut result = Period::default();
        if range == Range::AllTime {
            merge(&mut result, &self.data.all_time);
        } else if range == Range::Last5Hours {
            let minute = utc_minute();
            for bucket in self.data.minutes.values().filter(|b| b.utc_minute >= minute.saturating_sub(300) && b.utc_minute <= minute && b.hour < 24) {
                for (key, count) in &bucket.keys {
                    let target = result.keys.entry(key.clone()).or_default();
                    *target = target.saturating_add(*count);
                    result.total = result.total.saturating_add(*count);
                    result.hours[bucket.hour as usize] = result.hours[bucket.hour as usize].saturating_add(*count);
                }
            }
        } else {
            let week_start = today - ((now.wDayOfWeek as i64 + 6) % 7);
            for day in self.data.days.values() {
                let include = match range {
                    Range::Today => day.ordinal == today,
                    Range::Week => day.ordinal >= week_start && day.ordinal <= today,
                    Range::Month => day.year == now.wYear && day.month == now.wMonth && day.ordinal <= today,
                    _ => false,
                };
                if include { merge(&mut result, &day.bucket); }
            }
        }
        result
    }

    pub fn clear(&mut self) {
        if self.read_error.is_some() { return; }
        self.data.days.clear();
        self.data.minutes.clear();
        self.data.all_time = Bucket::default();
        self.dirty = true;
    }

    pub fn save(&mut self) -> Result<(), String> {
        if let Some(error) = &self.read_error { return Err(error.clone()); }
        if !self.dirty { return Ok(()); }
        let bytes = serde_json::to_vec(&self.data).map_err(|e| e.to_string())?;
        crate::config::atomic(&self.path, &bytes)?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_ranges_and_preserves_damaged_files() {
        let dir = std::env::temp_dir().join(format!("pet-input-stats-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("input-stats.json");
        let _ = fs::remove_file(&path);
        let mut store = Store::load(&dir);
        assert!(!store.enabled);
        store.count("A");
        assert_eq!(store.period(Range::AllTime).total, 0);
        store.set_enabled(true);
        store.count("A");
        store.count("MouseWheelUp");
        store.save().unwrap();
        let mut loaded = Store::load(&dir);
        assert!(loaded.enabled);
        for range in [Range::Today, Range::Last5Hours, Range::Week, Range::Month, Range::AllTime] {
            let period = loaded.period(range);
            assert_eq!(period.total, 2, "{range:?}");
            assert_eq!(period.keys.get("A"), Some(&1));
            assert_eq!(period.keys.get("MouseWheelUp"), Some(&1));
        }
        loaded.clear();
        loaded.save().unwrap();
        assert_eq!(Store::load(&dir).period(Range::AllTime).total, 0);
        fs::write(&path, b"invalid json").unwrap();
        let mut damaged = Store::load(&dir);
        assert!(damaged.read_error.is_some());
        damaged.set_enabled(true);
        damaged.count("A");
        assert!(damaged.save().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"invalid json");
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn calendar_ordinal_handles_leap_years() {
        assert_eq!(ordinal(2024, 3, 1) - ordinal(2024, 2, 28), 2);
        assert_eq!(ordinal(2025, 3, 1) - ordinal(2025, 2, 28), 1);
        assert_eq!(ordinal(2026, 1, 1) - ordinal(2025, 12, 31), 1);
    }
}
