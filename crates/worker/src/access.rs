//! Defence in depth for `/admin`: verify the Cloudflare Access JWT (`Cf-Access-Jwt-Assertion`)
//! inside the Worker, so a misconfigured Access app (a path left out, a policy deleted) cannot
//! expose the admin. Fails closed: with no Access settings the admin answers 503, unless
//! `ACCESS_JWT_OPTIONAL=1` (local dev and tests only; never set it in production).
//!
//! Config (plain vars, not secrets): `ACCESS_TEAM_DOMAIN` = `https://<team>.cloudflareaccess.com`,
//! `ACCESS_AUD` = the Access application's Audience (AUD) tag. `ACCESS_JWKS_URL` overrides where
//! the public keys come from (tests). Service-token requests (the CLI) carry a JWT too.
//!
//! Cost: the JWKS is cached for an hour (Cache API), the signature check is native WebCrypto, so
//! a request adds well under a millisecond of Worker CPU and at most one subrequest per hour.

use catalogged_core::access as jwt;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use worker::*;

type Denied = (u16, String);

fn var(env: &Env, name: &str) -> Option<String> {
    env.var(name).map(|v| v.to_string()).ok().filter(|v| !v.is_empty())
}

pub async fn verify(env: &Env, headers: &http::HeaderMap) -> std::result::Result<(), Denied> {
    if var(env, "ACCESS_JWT_OPTIONAL").as_deref() == Some("1") {
        return Ok(());
    }
    let (Some(team), Some(aud)) = (var(env, "ACCESS_TEAM_DOMAIN"), var(env, "ACCESS_AUD")) else {
        return Err((503, "admin disabled: Access verification is not configured".into()));
    };
    let deny = |why: &str| (403, format!("forbidden: {why}"));
    let token = headers
        .get("cf-access-jwt-assertion")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| deny("missing Access token"))?;
    let parsed = jwt::parse(token).map_err(|e| deny(&e))?;
    jwt::check_claims(&parsed, &aud, &team, (js_sys::Date::now() / 1000.0) as u64).map_err(|e| deny(&e))?;
    let jwks_url = var(env, "ACCESS_JWKS_URL").unwrap_or_else(|| format!("{}/cdn-cgi/access/certs", team.trim_end_matches('/')));
    let jwks = jwks(&jwks_url).await.map_err(|e| (502, format!("could not load Access keys: {e}")))?;
    let jwk = jwt::find_jwk(&jwks, &parsed.kid).ok_or_else(|| deny("unknown signing key"))?;
    match rs256_verify(&jwk, &parsed.signing_input, &parsed.signature).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(deny("bad signature")),
        Err(e) => Err((500, format!("signature check failed: {e}"))),
    }
}

async fn jwks(url: &str) -> Result<String> {
    let cache = Cache::default();
    if let Some(mut hit) = cache.get(url, false).await? {
        return hit.text().await;
    }
    let mut res = Fetch::Url(Url::parse(url)?).send().await?;
    if res.status_code() != 200 {
        return Err(Error::RustError(format!("status {}", res.status_code())));
    }
    let text = res.text().await?;
    let mut out = Response::ok(text.clone())?;
    out.headers_mut().set("cache-control", "public, max-age=3600")?;
    cache.put(url, out).await?;
    Ok(text)
}

async fn rs256_verify(jwk_json: &str, data: &[u8], sig: &[u8]) -> std::result::Result<bool, String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let crypto = js_sys::Reflect::get(&js_sys::global(), &"crypto".into()).map_err(js)?.unchecked_into::<web_sys::Crypto>();
    let subtle = crypto.subtle();
    let algo = js_sys::Object::new();
    js_sys::Reflect::set(&algo, &"name".into(), &"RSASSA-PKCS1-v1_5".into()).map_err(js)?;
    js_sys::Reflect::set(&algo, &"hash".into(), &"SHA-256".into()).map_err(js)?;
    let jwk = js_sys::JSON::parse(jwk_json).map_err(js)?.unchecked_into::<js_sys::Object>();
    let usages = js_sys::Array::of1(&"verify".into());
    let key = JsFuture::from(subtle.import_key_with_object("jwk", &jwk, &algo, false, &usages).map_err(js)?)
        .await
        .map_err(js)?
        .unchecked_into::<web_sys::CryptoKey>();
    let ok = JsFuture::from(
        subtle
            .verify_with_object_and_u8_array_and_u8_array(&algo, &key, &mut sig.to_vec(), &mut data.to_vec())
            .map_err(js)?,
    )
    .await
    .map_err(js)?;
    Ok(ok.as_bool().unwrap_or(false))
}
