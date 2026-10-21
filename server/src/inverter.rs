use std::net::IpAddr;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::redirect::Policy;

/// The dongle answers in well under a second on a local network. Anything
/// slower is a wrong address, not a slow inverter.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);

/// A real reply is a few kilobytes. Reading without a ceiling would let a
/// wrong host stream until memory runs out.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct Sample {
    pub ts: i64,
    pub sn: String,
    pub ver: String,
    pub inverter_type: i64,
    pub data: Vec<i64>,
    pub information: serde_json::Value,
    /// The dongle adds an Error object only when something is wrong. It is
    /// not part of Data, so nothing in the metric registry can carry it.
    pub error: Option<serde_json::Value>,
}

/// The dongle's host and port, checked for shape when the config loads. The
/// host is resolved again on every connect, so an address that moves is
/// followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

pub struct Client {
    http: reqwest::Client,
    url: String,
    dongle_serial: String,
    minimum_data_len: usize,
}

impl Client {
    pub async fn connect(
        endpoint: &Endpoint,
        dongle_serial: &str,
        allow_public: bool,
        minimum_data_len: usize,
    ) -> Result<Self> {
        let host = &endpoint.host;
        let port = endpoint.port;
        let resolved = resolve_local(host, port, allow_public).await?;

        // Resolve once, then connect to that literal while still sending the
        // hostname the user typed. Without this pinning a hostname could
        // resolve privately on the check and publicly on the request.
        let http = reqwest::Client::builder()
            .resolve(host, std::net::SocketAddr::new(resolved, port))
            .redirect(Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("building the HTTP client")?;

        let authority = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };

        tracing::info!(%host, %resolved, port, "inverter endpoint resolved");

        Ok(Self {
            http,
            url: format!("http://{authority}/"),
            dongle_serial: dongle_serial.to_string(),
            minimum_data_len,
        })
    }

    pub async fn read(&self) -> Result<Sample> {
        let body = format!(
            "optType=ReadRealTimeData&pwd={}",
            form_encode(&self.dongle_serial)
        );

        let response = self
            .http
            .post(&self.url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|err| anyhow!("could not reach the inverter: {err}"))?;

        let status = response.status();
        if status.is_redirection() {
            bail!(
                "the address tried to redirect the request ({status}). \
                 A dongle never does that, so check that this really is the inverter"
            );
        }
        if !status.is_success() {
            bail!("the inverter answered with HTTP {status}");
        }

        let text = read_capped(response).await?;
        self.parse(&text)
    }

    fn parse(&self, text: &str) -> Result<Sample> {
        // A wrong password does not get a polite rejection: the dongle answers
        // with the malformed fragment 0:code,message:"failed"}, so catch that
        // before blaming the JSON.
        if text.contains("\"failed\"") {
            bail!(
                "the inverter rejected the request. That is what it sends when \
                 inverter.dongle_serial is wrong"
            );
        }

        // serde_json rejects bare NaN and Infinity tokens outright, so a
        // reply carrying them fails here rather than poisoning the database.
        let payload: serde_json::Value =
            serde_json::from_str(text).context("the reply was not valid JSON")?;

        let object = payload
            .as_object()
            .ok_or_else(|| anyhow!("the reply was not a JSON object"))?;

        let sn = object
            .get("sn")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if sn != self.dongle_serial {
            bail!(
                "the device identified itself as {sn:?}, which is not the configured \
                 dongle serial. Check inverter.address and inverter.dongle_serial"
            );
        }

        let data_value = object
            .get("Data")
            .ok_or_else(|| anyhow!("the reply had no Data field"))?;
        let array = data_value
            .as_array()
            .ok_or_else(|| anyhow!("Data was not an array"))?;

        if array.len() < self.minimum_data_len {
            bail!(
                "Data held {} values but the registry reads up to index {}",
                array.len(),
                self.minimum_data_len - 1
            );
        }

        let mut data = Vec::with_capacity(array.len());
        for (index, value) in array.iter().enumerate() {
            let number = value
                .as_f64()
                .ok_or_else(|| anyhow!("Data[{index}] was not a number"))?;
            if !number.is_finite() {
                bail!("Data[{index}] was not a finite number");
            }
            data.push(number.round() as i64);
        }

        Ok(Sample {
            ts: chrono::Utc::now().timestamp(),
            sn: sn.to_string(),
            ver: object
                .get("ver")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            inverter_type: object
                .get("type")
                .and_then(|value| value.as_i64())
                .unwrap_or(-1),
            data,
            information: object
                .get("Information")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            error: object
                .get("Error")
                .filter(|value| !value.is_null())
                .cloned(),
        })
    }
}

async fn read_capped(response: reqwest::Response) -> Result<String> {
    if let Some(length) = response.content_length() {
        if length as usize > MAX_RESPONSE_BYTES {
            bail!("the device announced more than 256 KB of data. That is not an inverter");
        }
    }

    let mut response = response;
    let mut buffer: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| anyhow!("reading the reply failed: {err}"))?
    {
        buffer.extend_from_slice(&chunk);
        if buffer.len() > MAX_RESPONSE_BYTES {
            bail!("the device sent more than 256 KB. That is not an inverter");
        }
    }

    String::from_utf8(buffer).context("the reply was not valid UTF-8")
}

/// Split a configured address into host and port, rejecting anything that
/// could smuggle a second host past the private address check. The userinfo
/// form http://192.168.1.5@evil.example.com/ is the one this exists for.
pub fn split_endpoint(address: &str) -> Result<Endpoint> {
    let trimmed = address.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .unwrap_or(trimmed)
        .trim_end_matches('/');

    if without_scheme.is_empty() {
        bail!("inverter.address is empty");
    }
    if without_scheme
        .chars()
        .any(|c| matches!(c, '/' | '@' | '?' | '#' | '\\') || c.is_whitespace())
    {
        bail!("inverter.address {address:?} contains characters that do not belong in a host");
    }

    let (host, port) = if let Some(rest) = without_scheme.strip_prefix('[') {
        let (host, tail) = rest
            .split_once(']')
            .ok_or_else(|| anyhow!("inverter.address {address:?} has an unclosed ["))?;
        let port = match tail.strip_prefix(':') {
            Some(port) => parse_port(port)?,
            None if tail.is_empty() => 80,
            None => bail!("inverter.address {address:?} is malformed after ]"),
        };
        (host.to_string(), port)
    } else if let Some((host, port)) = without_scheme.rsplit_once(':') {
        // A bare IPv6 literal also contains colons; treat it as a host.
        if host.contains(':') {
            (without_scheme.to_string(), 80)
        } else {
            (host.to_string(), parse_port(port)?)
        }
    } else {
        (without_scheme.to_string(), 80)
    };

    if host.is_empty() {
        bail!("inverter.address {address:?} has no host");
    }
    if host.parse::<IpAddr>().is_err() && !is_plausible_hostname(&host) {
        bail!("inverter.address host {host:?} is not a valid hostname or IP address");
    }

    Ok(Endpoint { host, port })
}

fn parse_port(value: &str) -> Result<u16> {
    let port: u32 = value
        .parse()
        .map_err(|_| anyhow!("port {value:?} is not a number"))?;
    if !(1..=65535).contains(&port) {
        bail!("port {port} is out of range");
    }
    Ok(port as u16)
}

fn is_plausible_hostname(host: &str) -> bool {
    if host.len() > 253 {
        return false;
    }
    let bytes = host.as_bytes();
    let edges_ok = bytes
        .first()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && bytes.last().is_some_and(|c| c.is_ascii_alphanumeric());
    edges_ok
        && bytes
            .iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
}

/// Resolve the host and refuse to continue if any answer points off the local
/// network. Checking every address rather than the first one means a name
/// that returns both a private and a public record is rejected.
pub async fn resolve_local(host: &str, port: u16, allow_public: bool) -> Result<IpAddr> {
    let addresses: Vec<IpAddr> = tokio::net::lookup_host((host, port))
        .await
        .with_context(|| format!("could not resolve {host:?}"))?
        .map(|socket| socket.ip())
        .collect();

    let first = addresses
        .first()
        .copied()
        .ok_or_else(|| anyhow!("{host:?} resolved to no addresses"))?;

    if !allow_public {
        for address in &addresses {
            if !is_local(address) {
                bail!(
                    "{host:?} resolves to {address}, which is not on a local network. \
                     Set inverter.allow_public_address if that is really intended"
                );
            }
        }
    }

    Ok(first)
}

fn is_local(address: &IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                // Unique local addresses, fc00::/7.
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // Link local, fe80::/10.
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn form_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_userinfo_smuggling() {
        assert!(split_endpoint("http://192.168.1.5@evil.example.com/").is_err());
    }

    fn endpoint(host: &str, port: u16) -> Endpoint {
        Endpoint {
            host: host.to_string(),
            port,
        }
    }

    #[test]
    fn parses_plain_host_and_port() {
        assert_eq!(
            split_endpoint("http://192.168.29.10/").unwrap(),
            endpoint("192.168.29.10", 80)
        );
        assert_eq!(
            split_endpoint("192.168.29.10:8080").unwrap(),
            endpoint("192.168.29.10", 8080)
        );
        assert_eq!(
            split_endpoint("[fd00::1]:8080").unwrap(),
            endpoint("fd00::1", 8080)
        );
        assert_eq!(split_endpoint("fd00::1").unwrap(), endpoint("fd00::1", 80));
    }

    #[test]
    fn classifies_local_addresses() {
        assert!(is_local(&"192.168.1.5".parse().unwrap()));
        assert!(is_local(&"10.0.0.1".parse().unwrap()));
        assert!(is_local(&"127.0.0.1".parse().unwrap()));
        assert!(is_local(&"169.254.1.1".parse().unwrap()));
        assert!(is_local(&"fd00::1".parse().unwrap()));
        assert!(!is_local(&"8.8.8.8".parse().unwrap()));
        assert!(!is_local(&"2001:4860:4860::8888".parse().unwrap()));
    }
}
