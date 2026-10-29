//! Detection of approximate location via public IP address.
//! Used only to PRE-FILL options; the user must
//! always explicitly confirm (see UX guidelines).
//!
//! If no network connection is available (timeout / DNS error /
//! HTTP error), the system falls back to English with no country preselected

use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct GeoInfo {
    #[serde(rename = "countryCode")]
    pub country_code: String,
    pub country: String,
    pub timezone: String,
    #[serde(default)]
    pub status: String,
}

const GEO_API_URL: &str = "http://ip-api.com/json/?fields=status,message,country,countryCode,timezone";
const TIMEOUT: Duration = Duration::from_secs(3);

/// Attempts IP detection. Returns `None` if there is no network connection or an error occurs —
/// this is a silent, expected failure (not a bug); the caller should
/// simply fall back to the default values (English)
pub async fn detect_geolocation() -> Option<GeoInfo> {
    let client = reqwest::Client::builder().timeout(TIMEOUT).build().ok()?;

    let resp = client.get(GEO_API_URL).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let info: GeoInfo = resp.json().await.ok()?;
    if info.status != "success" {
        return None;
    }

    Some(info)
}

/// Quickly checks whether there is a usable network connection by
/// attempting a lightweight request with a short timeout. Used to
/// decide whether to attempt geolocation or to switch directly to
/// “English by default” mode
pub async fn has_internet() -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    client
        .head("http://ip-api.com/json/")
        .send()
        .await
        .is_ok()
}
