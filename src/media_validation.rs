//! Checks of existing companion configuration and credentials.
use anyhow::Context as _;
use std::path::Path;

fn parse_media_environment(
    root: &Path,
) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    let bytes = xcss::config::read_managed_file(
        &root.join("config/mediamtx.env"),
        cfg!(debug_assertions) && env!("XCOS_SOURCE_REVISION") == "unbound",
    )?;
    anyhow::ensure!(
        bytes.len() <= 16 * 1024,
        "media environment exceeds its bound"
    );
    let text = std::str::from_utf8(&bytes)?;
    let mut values = std::collections::BTreeMap::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (key, input) = line
            .split_once('=')
            .context("media environment requires key=value")?;
        let value =
            if let Some(quoted) = input.strip_prefix('"') {
                let quoted = quoted
                    .strip_suffix('"')
                    .context("invalid quoted media value")?;
                let mut value = String::new();
                let mut characters = quoted.chars();
                while let Some(character) = characters.next() {
                    if character == '\\' {
                        let escaped = characters.next().context("incomplete media escape")?;
                        anyhow::ensure!(matches!(escaped, '\\' | '"'), "unsupported media escape");
                        value.push(escaped);
                    } else {
                        anyhow::ensure!(character != '"', "unescaped media quote");
                        value.push(character);
                    }
                }
                value
            } else {
                anyhow::ensure!(
                    !input.chars().any(|character| character.is_whitespace()
                        || matches!(character, '"' | '\\' | '\'')),
                    "invalid unquoted media value"
                );
                input.to_owned()
            };
        anyhow::ensure!(
            values.insert(key.to_owned(), value).is_none(),
            "duplicate media environment key"
        );
    }
    validate_media_environment(root, &values)?;
    Ok(values)
}

fn validate_media_environment(
    root: &Path,
    values: &std::collections::BTreeMap<String, String>,
) -> anyhow::Result<()> {
    const KEYS: [&str; 6] = [
        "MTX_WEBRTCADDITIONALHOSTS",
        "MTX_RTSPENCRYPTION",
        "MTX_RTSPSADDRESS",
        "MTX_RTSPSERVERCERT",
        "MTX_RTSPSERVERKEY",
        "MTX_PATHDEFAULTS_RECORDDELETEAFTER",
    ];
    anyhow::ensure!(
        values.len() == KEYS.len() && KEYS.iter().all(|key| values.contains_key(*key)),
        "media environment must contain exactly the managed key set"
    );
    anyhow::ensure!(
        values
            .values()
            .all(|value| !value.is_empty() && !value.chars().any(char::is_control)),
        "media values must be bounded nonempty text"
    );
    anyhow::ensure!(
        values["MTX_RTSPENCRYPTION"] == "strict",
        "media requires strict RTSPS"
    );
    anyhow::ensure!(
        Path::new(&values["MTX_RTSPSERVERCERT"]) == root.join("config/rtsp-cert.pem")
            && Path::new(&values["MTX_RTSPSERVERKEY"]) == root.join("config/rtsp-key.pem"),
        "media certificates must use the fixed ROOT/config paths"
    );
    let address = &values["MTX_RTSPSADDRESS"];
    let socket = if address.starts_with(':') {
        format!("0.0.0.0{address}")
    } else {
        address.clone()
    };
    anyhow::ensure!(
        socket.parse::<std::net::SocketAddr>()?.port() != 0,
        "media requires a concrete RTSPS port"
    );
    validate_retention(&values["MTX_PATHDEFAULTS_RECORDDELETEAFTER"])?;
    for host in values["MTX_WEBRTCADDITIONALHOSTS"].split(',') {
        anyhow::ensure!(
            !host.is_empty() && !host.chars().any(char::is_whitespace),
            "media ICE host is invalid"
        );
        if host.parse::<std::net::IpAddr>().is_err() {
            let url = url::Url::parse(&format!("https://{host}"))?;
            anyhow::ensure!(
                url.host_str()
                    .is_some_and(|parsed| parsed.eq_ignore_ascii_case(host))
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.port().is_none()
                    && url.path() == "/"
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "media ICE hostname is invalid"
            );
        }
    }
    Ok(())
}

fn validate_retention(value: &str) -> anyhow::Result<()> {
    // MediaMTX uses nonnegative native durations (including an initial day component), with zero disabling expiry.
    // Keep that current business option without introducing a second syntax.
    let value = value.strip_prefix('+').unwrap_or(value);
    if value == "0" {
        return Ok(());
    }
    anyhow::ensure!(
        !value.is_empty() && value.len() <= 256,
        "recording retention exceeds its text bound"
    );
    let mut remaining = value;
    let mut total = 0_u128;
    let mut first = true;
    while !remaining.is_empty() {
        let split = remaining
            .char_indices()
            .find(|(_, character)| !character.is_ascii_digit() && *character != '.')
            .map_or(remaining.len(), |(index, _)| index);
        let number = &remaining[..split];
        remaining = &remaining[split..];
        anyhow::ensure!(!number.is_empty(), "retention requires a number and unit");
        let (unit, scale) = [
            ("ns", 1_u128),
            ("us", 1_000),
            ("µs", 1_000),
            ("μs", 1_000),
            ("ms", 1_000_000),
            ("s", 1_000_000_000),
            ("m", 60_000_000_000),
            ("h", 3_600_000_000_000),
            ("d", 86_400_000_000_000),
        ]
        .into_iter()
        .find(|(unit, _)| remaining.starts_with(*unit))
        .context("unsupported retention unit")?;
        remaining = &remaining[unit.len()..];
        if unit == "d" {
            anyhow::ensure!(
                first && !number.contains('.'),
                "days are only allowed as the first integer component"
            );
        }
        first = false;
        let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
        anyhow::ensure!(
            !whole.is_empty() || !fraction.is_empty(),
            "invalid retention number"
        );
        let whole = if whole.is_empty() {
            0
        } else {
            whole.parse::<u128>()?
        };
        anyhow::ensure!(
            fraction.bytes().all(|byte| byte.is_ascii_digit()),
            "invalid retention fraction"
        );
        let fraction = &fraction[..fraction.len().min(18)];
        let partial = if fraction.is_empty() {
            0
        } else {
            fraction.parse::<u128>()? * scale / 10_u128.pow(u32::try_from(fraction.len())?)
        };
        total = total
            .checked_add(whole.checked_mul(scale).context("retention overflow")?)
            .and_then(|value| value.checked_add(partial))
            .context("retention overflow")?;
        anyhow::ensure!(
            total <= i64::MAX as u128,
            "retention exceeds the native duration bound"
        );
    }
    anyhow::ensure!(
        total == 0 || total >= 15 * 60 * 1_000_000_000,
        "retention cannot be shorter than the fixed fifteen-minute recording segment"
    );
    Ok(())
}

pub(crate) fn verify_media_environment(root: &Path) -> anyhow::Result<()> {
    parse_media_environment(root)?;
    Ok(())
}
