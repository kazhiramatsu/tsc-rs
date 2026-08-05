use super::*;

#[test]
fn parses_supported_host_resolution_commands() {
    assert_eq!(
        parse_host_resolution_command(Some("draft")),
        Ok(HostResolutionCommand::Draft)
    );
    assert_eq!(
        parse_host_resolution_command(Some("check")),
        Ok(HostResolutionCommand::Check)
    );
}

#[test]
fn rejects_unknown_or_missing_host_resolution_commands() {
    assert_eq!(
        parse_host_resolution_command(Some("update")),
        Err("unknown host-resolution command: update".to_owned())
    );
    assert_eq!(
        parse_host_resolution_command(None),
        Err("missing host-resolution command (draft|check)".to_owned())
    );
}
