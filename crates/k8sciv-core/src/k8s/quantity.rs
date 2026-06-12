use k8s_openapi::apimachinery::pkg::api::resource::Quantity;

/// Parse a Kubernetes resource Quantity into a canonical f64.
/// CPU canonicalizes to cores, memory to bytes.
///
/// Handles decimal SI suffixes (m, k, M, G, T, P, E), binary SI (Ki..Ei),
/// scientific notation ("1e3"), and the n/u micro-units the metrics API
/// emits. Per the upstream grammar, `e`/`E` followed by a digit or sign is
/// an exponent ("1e3" = 1000); a trailing `E` alone is exa.
pub fn parse(q: &str) -> Option<f64> {
    let q = q.trim();
    if q.is_empty() {
        return None;
    }
    let bytes = q.as_bytes();
    let mut i = 0;
    // Sign and mantissa digits.
    if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        i = 1;
    }
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
        i += 1;
    }
    // Exponent only if e/E is followed by digit or sign.
    if i < bytes.len()
        && (bytes[i] == b'e' || bytes[i] == b'E')
        && bytes
            .get(i + 1)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'+' || *c == b'-')
    {
        i += 1;
        if matches!(bytes.get(i), Some(b'+') | Some(b'-')) {
            i += 1;
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    let (num, suffix) = q.split_at(i);
    let num: f64 = num.parse().ok()?;
    let mult: f64 = match suffix {
        "" => 1.0,
        "n" => 1e-9,
        "u" => 1e-6,
        "m" => 1e-3,
        "k" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        "T" => 1e12,
        "P" => 1e15,
        "E" => 1e18,
        "Ki" => 1024.0,
        "Mi" => 1024f64.powi(2),
        "Gi" => 1024f64.powi(3),
        "Ti" => 1024f64.powi(4),
        "Pi" => 1024f64.powi(5),
        "Ei" => 1024f64.powi(6),
        _ => return None,
    };
    Some(num * mult)
}

pub fn value(q: &Quantity) -> Option<f64> {
    parse(&q.0)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn plain_and_decimal() {
        assert_eq!(parse("2"), Some(2.0));
        assert_eq!(parse("0.5"), Some(0.5));
        assert_eq!(parse("-1"), Some(-1.0));
    }

    #[test]
    fn cpu_millis_and_micro() {
        assert_eq!(parse("250m"), Some(0.25));
        let n = parse("100n").unwrap();
        assert!((n - 1e-7).abs() < 1e-15, "100n parsed to {n}");
        let u = parse("500u").unwrap();
        assert!((u - 5e-4).abs() < 1e-12, "500u parsed to {u}");
    }

    #[test]
    fn binary_si() {
        assert_eq!(parse("128Mi"), Some(134_217_728.0));
        assert_eq!(parse("1Gi"), Some(1_073_741_824.0));
        assert_eq!(parse("2Ki"), Some(2048.0));
    }

    #[test]
    fn decimal_si() {
        assert_eq!(parse("1k"), Some(1000.0));
        assert_eq!(parse("2G"), Some(2e9));
    }

    #[test]
    fn exponent_vs_exa() {
        assert_eq!(parse("1e3"), Some(1000.0));
        assert_eq!(parse("1E3"), Some(1000.0));
        assert_eq!(parse("1E"), Some(1e18));
        assert_eq!(parse("1e-3"), Some(0.001));
    }

    #[test]
    fn garbage_is_none() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("abc"), None);
        assert_eq!(parse("1Qi"), None);
    }
}
