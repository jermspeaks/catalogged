//! Validation for "fetch a cover from this link" (the admin image proxy). The Worker only ever
//! fetches public https hostnames; this is a first filter, not the only protection (the admin is
//! behind Access, and the response must be a bounded image).

pub const MAX_REMOTE_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// Ok(()) when `u` is an https URL with a plain public-looking hostname.
pub fn check(u: &str) -> Result<(), &'static str> {
    let rest = u.strip_prefix("https://").ok_or("only https links are allowed")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') || host.contains(':') || host.contains('[') {
        return Err("links with credentials, ports or IPv6 hosts are not allowed");
    }
    let lower = host.to_ascii_lowercase();
    if !lower.contains('.') || lower.ends_with('.') {
        return Err("the link needs a full hostname");
    }
    if lower.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err("IP addresses are not allowed");
    }
    if ["localhost", "local", "internal", "lan", "home", "corp"].iter().any(|s| lower == *s || lower.ends_with(&format!(".{s}"))) {
        return Err("local hostnames are not allowed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn accepts_public_https() {
        assert!(check("https://image.tmdb.org/t/p/w500/abc.jpg").is_ok());
        assert!(check("https://covers.openlibrary.org/b/id/1-L.jpg?default=false").is_ok());
    }

    #[test]
    fn rejects_unsafe_links() {
        for bad in [
            "http://example.com/a.jpg",
            "https://localhost/a.jpg",
            "https://127.0.0.1/a.jpg",
            "https://user:pw@example.com/a.jpg",
            "https://example.com:8443/a.jpg",
            "https://[::1]/a.jpg",
            "https://intranet/a.jpg",
            "https://printer.local/a.jpg",
            "ftp://example.com/a.jpg",
            "https:///a.jpg",
        ] {
            assert!(check(bad).is_err(), "{bad}");
        }
    }
}
