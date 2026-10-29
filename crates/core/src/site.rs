#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Domain(String);

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidDomain;

pub const SLUG_MAX: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Slug(String);

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidSlug;

impl std::fmt::Display for InvalidSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("slug non valido")
    }
}

impl TryFrom<String> for Slug {
    type Error = InvalidSlug;
    fn try_from(s: String) -> Result<Self, InvalidSlug> {
        Self::parse(&s)
    }
}

impl From<Slug> for String {
    fn from(s: Slug) -> String {
        s.0
    }
}

impl Slug {
    /// Il valore entra in path, nomi utente e nomi DB: niente oltre [a-z0-9_].
    pub fn parse(s: &str) -> Result<Self, InvalidSlug> {
        let ok = (1..=SLUG_MAX).contains(&s.len())
            && s.starts_with(|c: char| c.is_ascii_lowercase())
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if ok { Ok(Self(s.to_string())) } else { Err(InvalidSlug) }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Domain {
    /// Hostname ASCII (IDN già in punycode), almeno due etichette.
    pub fn parse(s: &str) -> Result<Self, InvalidDomain> {
        let s = s.trim().to_ascii_lowercase();
        let labels: Vec<&str> = s.split('.').collect();
        let label_ok = |l: &&str| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        };
        if s.len() > 253 || labels.len() < 2 || !labels.iter().all(label_ok) {
            return Err(InvalidDomain);
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Slug per cartella, utente Unix e DB: [a-z0-9_], inizia con una lettera, max 24 caratteri.
pub fn slug_for(d: &Domain, taken: impl Fn(&str) -> bool) -> Slug {
    let host = d.as_str().strip_prefix("www.").unwrap_or(d.as_str());
    let mut base: String = host.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if base.starts_with(|c: char| c.is_ascii_digit()) {
        base.insert(0, 's');
    }
    let fit = |s: &str, max: usize| s[..s.len().min(max)].trim_end_matches('_').to_string();
    let first = fit(&base, SLUG_MAX);
    if !taken(&first) {
        return Slug(first);
    }
    let next = (2..)
        .map(|n| {
            let suffix = format!("_{n}");
            format!("{}{suffix}", fit(&base, SLUG_MAX - suffix.len()))
        })
        .find(|c| !taken(c))
        .expect("serie infinita");
    Slug(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_is_normalized_to_lowercase_without_spaces() {
        let d = Domain::parse("  Example.COM ").unwrap();
        assert_eq!(d.as_str(), "example.com");
    }

    #[test]
    fn domain_rejects_invalid_hostnames() {
        for bad in [
            "", "localhost", "exa mple.com", "-a.com", "a-.com", "a..com", ".a.com", "a.com.",
            "../etc", "a_b.com", "à.com", "a.c/m",
        ] {
            assert!(Domain::parse(bad).is_err(), "accettato: {bad:?}");
        }
        let long_label = format!("{}.com", "a".repeat(64));
        assert!(Domain::parse(&long_label).is_err());
    }

    #[test]
    fn domain_accepts_subdomains_and_punycode() {
        assert!(Domain::parse("www.pro-loco.sona.it").is_ok());
        assert!(Domain::parse("xn--caff-dsa.it").is_ok());
    }

    #[test]
    fn slug_replaces_separators_and_drops_www() {
        let d = Domain::parse("www.pro-loco.sona.it").unwrap();
        assert_eq!(slug_for(&d, |_| false).as_str(), "pro_loco_sona_it");
    }

    #[test]
    fn slug_is_at_most_24_chars() {
        let d = Domain::parse("associazione-molto-lunga-di-sona.example.org").unwrap();
        let s = slug_for(&d, |_| false);
        assert_eq!(s.as_str().len(), 24);
        assert_eq!(s.as_str(), "associazione_molto_lunga");
    }

    #[test]
    fn slug_starting_with_digit_gets_letter_prefix() {
        let d = Domain::parse("123.it").unwrap();
        assert_eq!(slug_for(&d, |_| false).as_str(), "s123_it");
    }

    #[test]
    fn slug_parse_accepts_only_safe_names() {
        assert!(Slug::parse("pro_loco_2").is_ok());
        for bad in ["", "2abc", "Abc", "a-b", "a.b", "a/b", "../x", "abcdefghijklmnopqrstuvwxy"] {
            assert!(Slug::parse(bad).is_err(), "accettato: {bad:?}");
        }
    }

    #[test]
    fn slug_collision_adds_numeric_suffix_within_limit() {
        let d = Domain::parse("associazione-molto-lunga.example.org").unwrap();
        let taken = ["associazione_molto_lunga", "associazione_molto_lun_2"];
        let s = slug_for(&d, |c| taken.contains(&c));
        assert_eq!(s.as_str(), "associazione_molto_lun_3");
    }
}
