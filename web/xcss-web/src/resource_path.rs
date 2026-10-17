//! Map MediaMTX's root-relative WHEP resources through the ticket's gateway.
//! URL resolution and origin/credential checks happen before this path-only step.
pub fn resource_path(
    resource: &str,
    endpoint: &str,
    stream_path: &str,
) -> Result<String, &'static str> {
    let upstream = format!("/{stream_path}/whep");
    let prefix = endpoint
        .strip_suffix(&upstream)
        .ok_or("WHEP endpoint does not match the camera stream")?;
    // Only the known upstream stream path needs a gateway prefix. An already
    // rewritten Location (or a resolved relative reference) stays unchanged.
    if resource == upstream
        || resource
            .strip_prefix(&upstream)
            .is_some_and(|tail| tail.starts_with('/'))
    {
        Ok(format!("{prefix}{resource}"))
    } else {
        Ok(resource.into())
    }
}

#[cfg(test)]
mod tests {
    use super::resource_path;

    const STREAM: &str = "cam_aaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa_main";

    #[test]
    fn gateway_prefix_comes_from_the_matching_ticket_endpoint() {
        for prefix in ["", "/media-webrtc", "/custom/rtc"] {
            let endpoint = format!("{prefix}/{STREAM}/whep");
            let upstream = format!("/{STREAM}/whep/session-1");
            let mapped = format!("{prefix}{upstream}");
            assert_eq!(resource_path(&upstream, &endpoint, STREAM).unwrap(), mapped);
            assert_eq!(resource_path(&mapped, &endpoint, STREAM).unwrap(), mapped);
            // A query-only Location resolves to this endpoint path. Its query
            // stays on the URL object, outside the path-only remapping.
            assert_eq!(
                resource_path(&endpoint, &endpoint, STREAM).unwrap(),
                endpoint
            );
            let relative = format!("{prefix}/{STREAM}/session-1");
            assert_eq!(
                resource_path(&relative, &endpoint, STREAM).unwrap(),
                relative
            );
        }
    }

    #[test]
    fn no_prefix_is_guessed_for_another_stream_or_path_boundary() {
        let endpoint = format!("/custom/rtc/{STREAM}/whep");
        assert!(resource_path("/session", "/custom/rtc/other/whep", STREAM).is_err());
        for path in [
            format!("/{STREAM}/whep-other/session"),
            "/other/whep/session".into(),
        ] {
            assert_eq!(resource_path(&path, &endpoint, STREAM).unwrap(), path);
        }
    }
}
