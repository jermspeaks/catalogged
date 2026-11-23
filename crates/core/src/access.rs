//! Cloudflare Access JWT checks that need no crypto: parsing, claim validation, JWKS lookup.
//! The Worker does the RS256 signature check with WebCrypto (native, so it stays inside the free
//! CPU limit). Everything here is pure so it is unit-tested natively.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;

/// Clock skew allowed on `exp`/`nbf`, in seconds.
const LEEWAY: u64 = 30;

#[derive(Debug)]
pub struct Parsed {
    pub kid: String,
    /// `header.payload` exactly as signed.
    pub signing_input: Vec<u8>,
    pub signature: Vec<u8>,
    pub claims: Value,
}

/// Split and decode a compact JWT. Only RS256 is accepted (what Access signs with); anything else,
/// including `alg: none`, is rejected here.
pub fn parse(token: &str) -> Result<Parsed, String> {
    let mut parts = token.split('.');
    let (Some(h), Some(p), Some(s), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err("not a JWT".into());
    };
    let dec = |part: &str| URL_SAFE_NO_PAD.decode(part).map_err(|_| "bad base64".to_string());
    let header: Value = serde_json::from_slice(&dec(h)?).map_err(|_| "bad header".to_string())?;
    if header.get("alg").and_then(Value::as_str) != Some("RS256") {
        return Err("unsupported alg".into());
    }
    let kid = header.get("kid").and_then(Value::as_str).ok_or("missing kid")?.to_owned();
    let claims: Value = serde_json::from_slice(&dec(p)?).map_err(|_| "bad payload".to_string())?;
    Ok(Parsed { kid, signing_input: format!("{h}.{p}").into_bytes(), signature: dec(s)?, claims })
}

/// Validate `iss`, `aud`, `exp` (required) and `nbf` (if present). `issuer` is the team domain URL
/// with scheme, e.g. `https://team.cloudflareaccess.com`. `now` is Unix seconds.
pub fn check_claims(p: &Parsed, aud: &str, issuer: &str, now: u64) -> Result<(), String> {
    let c = &p.claims;
    if c.get("iss").and_then(Value::as_str) != Some(issuer.trim_end_matches('/')) {
        return Err("wrong issuer".into());
    }
    let aud_ok = match c.get("aud") {
        Some(Value::String(a)) => a == aud,
        Some(Value::Array(list)) => list.iter().any(|a| a.as_str() == Some(aud)),
        _ => false,
    };
    if !aud_ok {
        return Err("wrong audience".into());
    }
    let exp = c.get("exp").and_then(Value::as_u64).ok_or("missing exp")?;
    if now > exp + LEEWAY {
        return Err("expired".into());
    }
    if let Some(nbf) = c.get("nbf").and_then(Value::as_u64) {
        if now + LEEWAY < nbf {
            return Err("not yet valid".into());
        }
    }
    Ok(())
}

/// The JWK (as JSON text) with this `kid` from a JWKS document.
pub fn find_jwk(jwks: &str, kid: &str) -> Option<String> {
    let v: Value = serde_json::from_str(jwks).ok()?;
    v.get("keys")?
        .as_array()?
        .iter()
        .find(|k| k.get("kid").and_then(Value::as_str) == Some(kid) && k.get("kty").and_then(Value::as_str) == Some("RSA"))
        .map(Value::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(header: &str, payload: &str) -> String {
        let e = |s: &str| URL_SAFE_NO_PAD.encode(s);
        format!("{}.{}.{}", e(header), e(payload), e("sig"))
    }
    const ISS: &str = "https://t.cloudflareaccess.com";

    #[test]
    fn parses_and_checks_claims() {
        let t = token(r#"{"alg":"RS256","kid":"k1"}"#, r#"{"iss":"https://t.cloudflareaccess.com","aud":["a1","a2"],"exp":1000}"#);
        let p = parse(&t).unwrap();
        assert_eq!(p.kid, "k1");
        assert_eq!(p.signature, b"sig");
        assert!(check_claims(&p, "a2", ISS, 900).is_ok());
        assert!(check_claims(&p, "a2", &format!("{ISS}/"), 900).is_ok());
        assert_eq!(check_claims(&p, "zz", ISS, 900), Err("wrong audience".into()));
        assert_eq!(check_claims(&p, "a1", "https://evil.example", 900), Err("wrong issuer".into()));
        assert_eq!(check_claims(&p, "a1", ISS, 1031), Err("expired".into()));
        assert!(check_claims(&p, "a1", ISS, 1030).is_ok()); // within leeway
    }

    #[test]
    fn rejects_bad_tokens() {
        assert!(parse("a.b").is_err());
        assert!(parse("a.b.c.d").is_err());
        assert!(parse(&token(r#"{"alg":"none","kid":"k"}"#, "{}")).is_err());
        assert!(parse(&token(r#"{"alg":"HS256","kid":"k"}"#, "{}")).is_err());
        assert!(parse(&token(r#"{"alg":"RS256"}"#, "{}")).is_err()); // no kid
        let no_exp = parse(&token(r#"{"alg":"RS256","kid":"k"}"#, r#"{"iss":"https://t.cloudflareaccess.com","aud":"a"}"#)).unwrap();
        assert_eq!(check_claims(&no_exp, "a", ISS, 1), Err("missing exp".into()));
        let single = parse(&token(r#"{"alg":"RS256","kid":"k"}"#, r#"{"iss":"https://t.cloudflareaccess.com","aud":"a","exp":5,"nbf":900}"#)).unwrap();
        assert_eq!(check_claims(&single, "a", ISS, 1), Err("not yet valid".into()));
    }

    #[test]
    fn finds_keys_by_kid() {
        let jwks = r#"{"keys":[{"kid":"a","kty":"RSA","n":"x","e":"AQAB"},{"kid":"b","kty":"EC"}]}"#;
        assert!(find_jwk(jwks, "a").unwrap().contains("\"n\":\"x\""));
        assert!(find_jwk(jwks, "b").is_none()); // not RSA
        assert!(find_jwk(jwks, "c").is_none());
        assert!(find_jwk("garbage", "a").is_none());
    }
}
