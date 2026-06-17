use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use k8s_openapi::jiff;

/// FNV-1a 64-bit hash. Map layout ordering must be stable across runs and
/// Rust releases, which rules out `std`'s `DefaultHasher`.
pub fn fnv1a64(s: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// Compact single-unit age: "12s", "5m", "3h", "2d".
pub fn format_age(then: &Time) -> String {
    let secs = jiff::Timestamp::now()
        .duration_since(then.0)
        .as_secs()
        .max(0);
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86_399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

pub fn format_age_opt(then: Option<&Time>) -> String {
    then.map(format_age).unwrap_or_else(|| "?".into())
}

/// "1.5Gi", "512Mi", "3.2Ti" — binary units, one decimal where it matters.
pub fn human_bytes(bytes: f64) -> String {
    const UNITS: [(&str, f64); 4] = [
        ("Ti", 1024f64 * 1024.0 * 1024.0 * 1024.0),
        ("Gi", 1024f64 * 1024.0 * 1024.0),
        ("Mi", 1024f64 * 1024.0),
        ("Ki", 1024f64),
    ];
    for (unit, scale) in UNITS {
        if bytes >= scale {
            let v = bytes / scale;
            return if v >= 10.0 {
                format!("{v:.0}{unit}")
            } else {
                format!("{v:.1}{unit}")
            };
        }
    }
    format!("{bytes:.0}B")
}

/// Compact live-usage label: cpu in millicores + memory (e.g. `12m 45Mi`),
/// the `kubectl top` idiom. Cpu is cores in, scaled to millicores.
pub fn format_usage(cpu_cores: f64, mem_bytes: f64) -> String {
    format!("{:.0}m {}", cpu_cores * 1000.0, human_bytes(mem_bytes))
}

/// Truncate to `max` display characters, appending `…` when cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_is_stable() {
        // Pinned values: these must never change, or map layouts reshuffle
        // between releases.
        assert_eq!(fnv1a64(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64("kind-worker"), fnv1a64("kind-worker"));
        assert_ne!(fnv1a64("kind-worker"), fnv1a64("kind-worker2"));
    }

    #[test]
    fn truncate_handles_unicode() {
        assert_eq!(truncate("abcdef", 6), "abcdef");
        assert_eq!(truncate("abcdefg", 6), "abcde…");
        assert_eq!(truncate("ééééééé", 6), "ééééé…");
    }
}
