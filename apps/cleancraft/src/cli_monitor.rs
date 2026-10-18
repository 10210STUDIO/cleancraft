//! `cleancraft status` (a Live Monitor reading) and `cleancraft sweep` (Quick Sweep's scan,
//! saving each module's plan for `cleancraft clean <plan> --yes`). Both only look.

use std::sync::atomic::AtomicBool;

use cleancraft_core::fsutil::{human_size, memory_size};
use cleancraft_core::monitor::{
    self, BatteryReading, Sampler, Snapshot, UNKNOWN, pct, rate_text, uptime_text,
};
use cleancraft_core::sweep::{self, Sources, TileData};
use cleancraft_core::{Env, Settings, plans};

fn or_unknown(v: Option<String>) -> String {
    v.unwrap_or_else(|| UNKNOWN.into())
}

/// The reading as text, one line per value.
pub fn status_text(s: &Snapshot) -> String {
    let mut o = Vec::new();
    o.push(format!(
        "CPU        {}",
        or_unknown(s.cpu.as_ref().map(|c| pct(c.load)))
    ));
    o.push(format!(
        "Uptime     {}",
        or_unknown(s.uptime_secs.map(uptime_text))
    ));
    o.push(format!(
        "Memory     {}",
        or_unknown(s.memory.as_ref().map(|m| format!(
            "{} of {} used, pressure {}",
            memory_size(m.used),
            memory_size(m.total),
            m.pressure.map(|p| p.label()).unwrap_or(UNKNOWN)
        )))
    ));
    o.push(format!(
        "Free space {}",
        or_unknown(s.storage.free.map(human_size))
    ));
    o.push(format!(
        "Trash      {}",
        or_unknown(s.storage.trash.map(human_size))
    ));
    o.push(format!(
        "Battery    {}",
        match &s.battery {
            Some(BatteryReading::Present(b)) => format!("{}%, {}", b.percent, b.state),
            Some(BatteryReading::None) => "none".into(),
            None => UNKNOWN.into(),
        }
    ));
    o.push(format!(
        "Network    {}",
        or_unknown(s.network.map(|n| format!(
            "down {}, up {}",
            rate_text(n.down),
            rate_text(n.up)
        )))
    ));
    for a in monitor::alerts(s) {
        o.push(format!("alert: {}", a.text));
    }
    for p in &s.problems {
        o.push(format!("not read: {p}"));
    }
    o.join("\n")
}

/// Two readings one second apart, so the network speed is known.
pub fn status(env: &Env, json: bool, pause: std::time::Duration) -> anyhow::Result<String> {
    let mut s = Sampler::new();
    s.sample(env, monitor::now_secs());
    std::thread::sleep(pause);
    let snap = s.sample(env, monitor::now_secs());
    Ok(if json {
        serde_json::to_string_pretty(&snap)?
    } else {
        status_text(&snap)
    })
}

pub fn sweep_text(env: &Env, settings: &Settings, sources: &Sources) -> anyhow::Result<String> {
    let res = sweep::scan(env, settings, sources, &AtomicBool::new(false), &|_| {});
    let mut o = Vec::new();
    for t in &res.tiles {
        let skipped = settings.sweep_skip.contains(&t.id);
        let mut line = sweep::describe(t);
        if skipped {
            line.push_str(" (skipped in Settings)");
        }
        if let TileData::Plan(p) = &t.data {
            plans::save(env, p)?;
            line.push_str(&format!("\n    plan {}", p.id));
        }
        o.push(line);
    }
    o.push(format!(
        "\n{} ready to clean. Review a plan with `cleancraft clean <plan>` (prints the list), then add --yes to move it to the Trash.",
        human_size(res.ready_bytes(&settings.sweep_skip))
    ));
    Ok(o.join("\n"))
}

pub fn sweep(env: &Env, settings: &Settings) -> anyhow::Result<String> {
    sweep_text(env, settings, &Sources::registered())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cleancraft_core::testkit::FakeMac;
    use cleancraft_core::testkit_monitor::*;

    #[test]
    fn status_lines() {
        let m = FakeMac::new();
        if let Some(f) = m.env.fake_sys() {
            answer_all(f);
        }
        let out = status(&m.env, false, std::time::Duration::ZERO).unwrap_or_default();
        assert!(out.contains("CPU        17%"), "{out}");
        assert!(out.contains("Battery    64%, discharging"), "{out}");
        assert!(out.contains("Free space 81.9 GB"), "{out}");
        let json = status(&m.env, true, std::time::Duration::ZERO).unwrap_or_default();
        assert!(json.contains("\"percent\": 64"));
    }

    #[test]
    fn status_unknown() {
        let m = FakeMac::new();
        let out = status(&m.env, false, std::time::Duration::ZERO).unwrap_or_default();
        assert!(out.contains("CPU        Unknown"), "{out}");
        assert!(out.contains("not read:"), "{out}");
    }

    #[test]
    fn sweep_lines_and_saved_plans() {
        let m = sweep_mac();
        let out = sweep_text(&m.env, &Settings::default(), &sweep_sources()).unwrap_or_default();
        assert!(out.contains("App Updates: 3 updates"), "{out}");
        let ids: Vec<&str> = out
            .lines()
            .filter_map(|l| l.trim().strip_prefix("plan "))
            .collect();
        assert_eq!(ids.len(), 3, "{out}");
        for id in ids {
            assert!(plans::load(&m.env, id).is_ok());
        }
        assert!(m.env.home_path("Library/Mail Downloads/b.zip").exists());
    }
}
