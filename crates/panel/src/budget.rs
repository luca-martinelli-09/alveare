//! Budget di memoria: il massimo teorico di ogni pool (pm.max_children × memory_limit) più i
//! servizi fissi, confrontato con la RAM. Avvisa sopra il 100%, chiede conferma sopra il 150%.
use crate::store::PhpSettingsRow;

/// Stima fissa per MariaDB, Redis, Caddy e pannello.
pub const RESERVED_MB: u64 = 1024;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Budget {
    pub pools_mb: u64,
    /// Worker long-running (processi × memoria).
    pub workers_mb: u64,
    pub reserved_mb: u64,
    pub total_mb: u64,
    pub ram_mb: u64,
    /// Totale rispetto alla RAM, in percentuale.
    pub percent: u64,
}

impl Budget {
    pub fn over_ram(&self) -> bool {
        self.total_mb > self.ram_mb
    }
    /// Oltre il 150% serve una conferma esplicita.
    pub fn needs_confirmation(&self) -> bool {
        self.total_mb * 2 > self.ram_mb * 3
    }
}

pub fn pool_mb(s: &PhpSettingsRow) -> u64 {
    u64::from(s.max_children) * u64::from(s.memory_limit_mb)
}

pub fn compute<'a>(pools: impl IntoIterator<Item = &'a PhpSettingsRow>, workers_mb: u64, ram_mb: u64) -> Budget {
    let pools_mb: u64 = pools.into_iter().map(pool_mb).sum();
    let total_mb = pools_mb + workers_mb + RESERVED_MB;
    Budget { pools_mb, workers_mb, reserved_mb: RESERVED_MB, total_mb, ram_mb, percent: if ram_mb == 0 { 0 } else { total_mb * 100 / ram_mb } }
}

/// MemTotal da /proc/meminfo, in MB.
pub fn parse_meminfo(text: &str) -> Option<u64> {
    let kb: u64 = text.lines().find_map(|l| l.strip_prefix("MemTotal:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kb / 1024)
}

pub fn ram_mb() -> u64 {
    std::fs::read_to_string("/proc/meminfo").ok().and_then(|t| parse_meminfo(&t)).unwrap_or(0)
}

/// Limiti del form: fuori da questi il valore è quasi certamente un errore di battitura.
pub fn validate(s: &PhpSettingsRow) -> Result<(), String> {
    let checks = [
        ("memory_limit", s.memory_limit_mb, 32, 4096),
        ("upload massimo", s.upload_max_mb, 1, 4096),
        ("tempo massimo", s.max_execution_time_s, 5, 3600),
        ("processi massimi", s.max_children, 1, 100),
    ];
    for (name, v, min, max) in checks {
        if !(min..=max).contains(&v) {
            return Err(format!("{name}: tra {min} e {max}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(mem: u32, children: u32) -> PhpSettingsRow {
        PhpSettingsRow { memory_limit_mb: mem, upload_max_mb: 64, max_execution_time_s: 60, max_children: children }
    }

    #[test]
    fn budget_sums_pools_and_reserved() {
        let b = compute(&[row(256, 5), row(128, 4)], 0, 4096);
        assert_eq!((b.pools_mb, b.total_mb, b.percent), (1792, 2816, 68));
        assert_eq!(compute(&[row(256, 5)], 512, 4096).total_mb, 1280 + 512 + RESERVED_MB, "i worker contano");
        assert!(!b.over_ram() && !b.needs_confirmation());
        let b = compute(&[row(512, 10)], 0, 4096);
        assert!(b.over_ram() && !b.needs_confirmation(), "{b:?}");
        let b = compute(&[row(1024, 10)], 0, 4096);
        assert!(b.needs_confirmation());
    }

    #[test]
    fn meminfo_is_parsed_in_megabytes() {
        assert_eq!(parse_meminfo("MemTotal:       24372440 kB\nMemFree: 1 kB\n"), Some(23801));
        assert_eq!(parse_meminfo("niente"), None);
    }

    #[test]
    fn form_values_have_sane_bounds() {
        assert!(validate(&row(256, 5)).is_ok());
        assert!(validate(&row(16, 5)).is_err());
        assert!(validate(&row(256, 0)).is_err());
    }
}
