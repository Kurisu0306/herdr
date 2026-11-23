//! Build identity helpers.

pub const BASE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn channel() -> &'static str {
    non_empty(option_env!("HERDR_BUILD_CHANNEL")).unwrap_or("stable")
}

pub fn build_id() -> Option<&'static str> {
    non_empty(option_env!("HERDR_BUILD_ID"))
}

pub fn version() -> String {
    match channel() {
        "stable" => BASE_VERSION.to_string(),
        channel => match build_id() {
            Some(build_id) => format!("{BASE_VERSION}-{channel}.{build_id}"),
            None => format!("{BASE_VERSION}-{channel}"),
        },
    }
}

/// Tag of the fork release this tree builds, from the checked-in `FORK_VERSION` file. Upstream
/// builds have no such identity; `version()` stays the upstream version so update checks and
/// the endpoint handshake compare what they always compared.
pub fn fork_version() -> Option<&'static str> {
    non_empty(Some(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/FORK_VERSION"
    ))))
}

/// The version as shown to people: the upstream version, then the fork release in parentheses.
pub fn display_version() -> String {
    match fork_version() {
        Some(fork) => format!("{} ({fork})", version()),
        None => version(),
    }
}

pub fn is_preview() -> bool {
    channel() == "preview"
}

fn non_empty(value: Option<&'static str>) -> Option<&'static str> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn stable_version_defaults_to_cargo_version() {
        assert!(!super::version().is_empty());
    }

    #[test]
    fn display_version_names_the_fork_release_after_the_upstream_version() {
        let fork = super::fork_version().expect("FORK_VERSION names a release");
        assert!(fork.starts_with("fork-"), "{fork}");
        assert_eq!(
            super::display_version(),
            format!("{} ({fork})", super::version())
        );
    }
}
