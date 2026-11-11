//! Detection of the country of the public IP address.
//! Used only to PROPOSE the country: the user must always confirm it, and the language
//! and the keyboard are proposed from the country afterwards (countries.rs, keyboard.rs).
//!
//! If no network connection is available (timeout / DNS error /
//! HTTP error), no country is preselected
//!
//! The service is KDE's geoip, over https (the one Calamares used). Its "ubiquity"
//! answer is a short XML document: <CountryCode>IT</CountryCode>…

// Without the "geoip" feature (reqwest) only the parsing is compiled
#![cfg_attr(not(feature = "geoip"), allow(dead_code))]

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct GeoInfo {
    pub country_code: String,
}

const GEO_API_URL: &str = "https://geoip.kde.org/v1/ubiquity";
const TIMEOUT: Duration = Duration::from_secs(3);

/// Text of the first <name>…</name> element of the answer
fn element<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = start + xml[start..].find("</")?;
    Some(xml[start..end].trim())
}

fn parse(xml: &str) -> Option<GeoInfo> {
    if element(xml, "Status")? != "OK" {
        return None;
    }
    let country_code = element(xml, "CountryCode").filter(|c| c.len() == 2)?;
    Some(GeoInfo { country_code: country_code.to_string() })
}

/// Attempts IP detection. Returns `None` if there is no network connection or an error occurs —
/// this is a silent, expected failure (not a bug); the caller should
/// simply fall back to the default values (English)
#[cfg(feature = "geoip")]
pub async fn detect_geolocation() -> Option<GeoInfo> {
    let client = reqwest::Client::builder().timeout(TIMEOUT).build().ok()?;

    let resp = client.get(GEO_API_URL).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    parse(&resp.text().await.ok()?)
}

/// Quickly checks whether there is a usable network connection by
/// attempting a lightweight request with a short timeout. Used to
/// decide whether to attempt geolocation or to switch directly to
/// “English by default” mode
#[cfg(feature = "geoip")]
pub async fn has_internet() -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    client
        .head(GEO_API_URL)
        .send()
        .await
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_country() {
        let xml = "<Response><Ip>5.95.1.1</Ip><Status>OK</Status><CountryCode>IT</CountryCode>\
                   <CountryCode3></CountryCode3><CountryName>Italy</CountryName>\
                   <TimeZone>Europe/Rome</TimeZone></Response>";
        let geo = parse(xml).unwrap();
        assert_eq!(geo.country_code, "IT");
    }

    #[test]
    fn refuses_a_failed_answer() {
        assert!(parse("<Response><Status>FAIL</Status></Response>").is_none());
        assert!(parse("<Response><Status>OK</Status><CountryCode></CountryCode></Response>").is_none());
        assert!(parse("not xml").is_none());
    }
}
