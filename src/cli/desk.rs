use crate::api::schema::{
    DeskCloseParams, DeskFocusParams, DeskItemKind, DeskOpenParams, DeskTarget, Method, Request,
    ResponseResult,
};

fn resolve_target(
    tab: Option<String>,
    pane: Option<String>,
    own_pane: Option<&str>,
    list: bool,
    all: bool,
) -> Result<DeskTarget, String> {
    if all {
        return Ok(DeskTarget::default());
    }
    if let Some(tab_id) = tab {
        return Ok(DeskTarget {
            tab_id: Some(tab_id),
            pane_id: None,
        });
    }
    let pane_id = pane.or_else(|| own_pane.map(str::to_owned));
    if pane_id.is_none() && !list {
        return Err("not in a herdr pane; pass --tab".into());
    }
    Ok(DeskTarget {
        tab_id: None,
        pane_id,
    })
}

fn classify_reference(reference: &str) -> Result<String, String> {
    let scheme_and_rest = reference.split_once(':');
    if let Some((scheme, rest)) = scheme_and_rest {
        if (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
            && rest.starts_with("//")
        {
            // The server expects lowercase schemes; preserve the rest of the URL exactly.
            return Ok(format!("{}:{rest}", scheme.to_ascii_lowercase()));
        }
    }
    let decoded;
    let path = if let Some((_, rest)) = scheme_and_rest
        .filter(|(scheme, rest)| scheme.eq_ignore_ascii_case("file") && rest.starts_with("//"))
    {
        let path = &rest[2..];
        let path = if path
            .get(..10)
            .is_some_and(|host| host.eq_ignore_ascii_case("localhost/"))
        {
            &path[9..]
        } else {
            path
        };
        decoded = decode_file_path(path)?;
        decoded.as_str()
    } else {
        if let Some((scheme, rest)) = scheme_and_rest {
            let drive_path = scheme.len() == 1
                && scheme.as_bytes()[0].is_ascii_alphabetic()
                && rest.starts_with(['\\', '/']);
            if !drive_path
                && !scheme.is_empty()
                && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            {
                return Err(format!("unsupported scheme: {scheme}"));
            }
        }
        reference
    };
    std::fs::canonicalize(path)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|_| format!("no such file: {path}"))
}

fn decode_file_path(path: &str) -> Result<String, String> {
    let mut bytes = path.bytes();
    let mut decoded = Vec::with_capacity(path.len());
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hex = |byte: u8| (byte as char).to_digit(16).map(|digit| digit as u8);
            let high = bytes.next().and_then(hex);
            let low = bytes.next().and_then(hex);
            let (Some(high), Some(low)) = (high, low) else {
                return Err(format!("invalid file URL escape: {path}"));
            };
            decoded.push(high * 16 + low);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded).map_err(|_| format!("invalid UTF-8 file URL path: {path}"))
}

pub(super) fn run_desk_command(args: &[String]) -> std::io::Result<i32> {
    let matches = match super::spec::command().try_get_matches_from(
        ["herdr".to_owned(), "desk".to_owned()]
            .into_iter()
            .chain(args.iter().cloned()),
    ) {
        Ok(matches) => matches,
        Err(error) => {
            let code = error.exit_code();
            error.print()?;
            return Ok(code);
        }
    };
    let Some((action, matches)) = matches
        .subcommand_matches("desk")
        .and_then(|m| m.subcommand())
    else {
        let mut command = super::spec::command();
        if let Some(desk) = command.find_subcommand_mut("desk") {
            desk.print_help()?;
            println!();
        }
        return Ok(2);
    };
    let own_pane = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|id| !id.is_empty());
    let target = match resolve_target(
        matches
            .get_one::<String>("tab")
            .map(|id| super::normalize_tab_id(id)),
        matches
            .get_one::<String>("pane")
            .map(|id| super::normalize_pane_id(id)),
        own_pane.as_deref(),
        action == "list",
        action == "list" && matches.get_flag("all"),
    ) {
        Ok(target) => target,
        Err(error) => {
            eprintln!("error: {error}");
            return Ok(1);
        }
    };
    let item = if action == "list" {
        None
    } else {
        matches
            .get_one::<String>(if action == "open" {
                "reference"
            } else {
                "item"
            })
            .cloned()
    };
    let method = match action {
        "open" => {
            let reference = match classify_reference(item.as_deref().unwrap_or_default()) {
                Ok(reference) => reference,
                Err(error) => {
                    eprintln!("error: {error}");
                    return Ok(1);
                }
            };
            Method::DeskOpen(DeskOpenParams {
                target,
                reference,
                title: matches.get_one::<String>("title").cloned(),
                background: matches.get_flag("background"),
                opened_by: Some(
                    own_pane
                        .map(|id| format!("pane:{id}"))
                        .unwrap_or_else(|| "cli".into()),
                ),
            })
        }
        "list" => Method::DeskList(target),
        "close" => Method::DeskClose(DeskCloseParams { target, item }),
        "focus" => Method::DeskFocus(DeskFocusParams {
            target,
            item: item.unwrap_or_default(),
        }),
        _ => return Ok(2),
    };
    let response = super::send_request(&Request {
        id: "cli:desk".into(),
        method,
    })?;
    if let Some(error) = response.get("error") {
        eprintln!(
            "error: {}: {}",
            error["code"].as_str().unwrap_or("unknown"),
            error["message"].as_str().unwrap_or("unknown")
        );
        return Ok(1);
    }
    let result = response
        .get("result")
        .ok_or_else(|| std::io::Error::other("missing desk result"))?;
    if matches.get_flag("json") {
        println!("{}", serde_json::to_string(result)?);
    } else {
        let result: ResponseResult = serde_json::from_value(result.clone())?;
        print!("{}", text_result(action, &result));
    }
    Ok(0)
}

fn text_result(action: &str, result: &ResponseResult) -> String {
    use std::fmt::Write;
    let mut output = String::new();
    match result {
        ResponseResult::Desk { desk, item_id } if action == "open" => {
            if let Some(item) = desk.desk.items.iter().find(|item| item.id == *item_id) {
                let _ = writeln!(output, "{} {} {}", item_id, desk.tab_id, item.reference);
            }
        }
        ResponseResult::Desk { desk, .. } => {
            if let Some(front) = &desk.desk.front {
                let _ = writeln!(output, "{front}");
            }
        }
        ResponseResult::DeskList { desks } => {
            for desk in desks {
                if desks.len() > 1 {
                    let _ = writeln!(output, "{}", desk.tab_id);
                }
                for item in &desk.desk.items {
                    let marker = if desk.desk.front.as_deref() == Some(&item.id) {
                        '*'
                    } else {
                        ' '
                    };
                    let kind = match item.kind {
                        DeskItemKind::Url => "url",
                        DeskItemKind::File => "file",
                    };
                    let _ = writeln!(
                        output,
                        "{marker} {} {kind} {} {}",
                        item.id, item.title, item.reference
                    );
                }
            }
        }
        _ => {}
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    // Parser policy has independent positional, flag and target-precedence edge cases.
    #[test]
    fn cli_desk_parse_and_target() {
        for args in [
            vec![
                "open",
                "https://example.com",
                "--title",
                "Example",
                "--background",
                "--json",
            ],
            vec!["list", "--all", "--json"],
            vec!["close", "--pane", "w1:p1"],
            vec!["focus", "d1", "--tab", "w1:t1"],
        ] {
            assert!(super::super::spec::command()
                .try_get_matches_from(["herdr", "desk"].into_iter().chain(args),)
                .is_ok());
        }
        for args in [
            vec!["open"],
            vec!["focus"],
            vec!["list", "--all", "--tab", "w1:t1"],
        ] {
            assert!(super::super::spec::command()
                .try_get_matches_from(["herdr", "desk"].into_iter().chain(args),)
                .is_err());
        }
        assert_eq!(
            resolve_target(
                Some("t".into()),
                Some("p".into()),
                Some("own"),
                false,
                false
            )
            .unwrap()
            .tab_id
            .as_deref(),
            Some("t")
        );
        assert_eq!(
            resolve_target(None, Some("p".into()), Some("own"), false, false)
                .unwrap()
                .pane_id
                .as_deref(),
            Some("p")
        );
        assert_eq!(
            resolve_target(None, None, Some("own"), false, false)
                .unwrap()
                .pane_id
                .as_deref(),
            Some("own")
        );
        assert!(resolve_target(None, None, None, false, false).is_err());
        assert_eq!(
            resolve_target(None, None, None, true, false).unwrap(),
            DeskTarget::default()
        );
        assert_eq!(
            resolve_target(None, None, Some("own"), true, true).unwrap(),
            DeskTarget::default()
        );
    }

    // Golden output covers the public CLI contract independently of server lifecycle tests.
    #[test]
    fn cli_desk_text_output_golden() {
        use crate::api::schema::{DeskInfo, DeskItem, TabDesk};
        let desk = TabDesk {
            tab_id: "w1:t1".into(),
            workspace_id: "w1".into(),
            desk: DeskInfo {
                items: vec![DeskItem {
                    id: "d1".into(),
                    kind: DeskItemKind::Url,
                    reference: "https://example.com".into(),
                    title: "Example".into(),
                    mime: "text/html".into(),
                    opened_by: "cli".into(),
                    opened_at_ms: 0,
                }],
                front: Some("d1".into()),
            },
        };
        let result = ResponseResult::Desk {
            desk: desk.clone(),
            item_id: "d1".into(),
        };
        assert_eq!(
            text_result("open", &result),
            "d1 w1:t1 https://example.com\n"
        );
        assert_eq!(text_result("focus", &result), "d1\n");
        assert_eq!(text_result("close", &result), "d1\n");
        assert_eq!(
            text_result(
                "list",
                &ResponseResult::DeskList {
                    desks: vec![desk.clone()]
                }
            ),
            "* d1 url Example https://example.com\n"
        );
        let mut second = desk.clone();
        second.tab_id = "w1:t2".into();
        second.desk.front = None;
        assert_eq!(text_result("list", &ResponseResult::DeskList { desks: vec![desk, second] }),
            "w1:t1\n* d1 url Example https://example.com\nw1:t2\n  d1 url Example https://example.com\n");
        assert_eq!(
            text_result(
                "close",
                &ResponseResult::Desk {
                    desk: TabDesk {
                        tab_id: "w1:t1".into(),
                        workspace_id: "w1".into(),
                        desk: DeskInfo::default()
                    },
                    item_id: "d1".into(),
                }
            ),
            ""
        );
    }

    #[test]
    fn cli_desk_reference_filesystem_boundary() {
        assert_eq!(
            classify_reference("https://example.com").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            classify_reference("http://example.com").unwrap(),
            "http://example.com"
        );
        for (reference, expected) in [
            ("HTTP://example.com/A", "http://example.com/A"),
            ("Https://example.com/A", "https://example.com/A"),
        ] {
            assert_eq!(classify_reference(reference).unwrap(), expected);
        }
        // Drive paths must reach the filesystem, even on a non-Windows test host.
        for reference in [
            r"C:\docs\a.md",
            "C:/docs/a.md",
            r"z:\docs\a.md",
            "z:/docs/a.md",
        ] {
            let expected = std::fs::canonicalize(reference)
                .map(|path| path.to_string_lossy().into_owned())
                .map_err(|_| format!("no such file: {reference}"));
            assert_eq!(classify_reference(reference), expected);
        }
        let path = std::fs::canonicalize("Cargo.toml").unwrap();
        assert_eq!(
            classify_reference("Cargo.toml").unwrap(),
            path.to_string_lossy()
        );
        assert_eq!(
            classify_reference(&format!("file://{}", path.display())).unwrap(),
            path.to_string_lossy()
        );
        for prefix in ["file://localhost", "FILE://LOCALHOST"] {
            assert_eq!(
                classify_reference(&format!("{prefix}{}", path.display())).unwrap(),
                path.to_string_lossy()
            );
        }
        let spaced =
            std::env::temp_dir().join(format!("herdr desk reference {}.md", std::process::id()));
        let _file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&spaced)
            .unwrap();
        let expected = std::fs::canonicalize(&spaced).unwrap();
        let encoded = spaced.to_string_lossy().replace(' ', "%20");
        for reference in [
            format!("file://{encoded}"),
            format!("file://localhost{encoded}"),
        ] {
            assert_eq!(
                classify_reference(&reference).unwrap(),
                expected.to_string_lossy()
            );
        }
        drop(_file);
        std::fs::remove_file(&spaced).unwrap();
        for reference in ["file:///bad%", "file:///bad%GG", "file:///bad%FF"] {
            assert!(classify_reference(reference)
                .unwrap_err()
                .starts_with("invalid"));
        }
        assert!(classify_reference("no-such-desk-file-123456789")
            .unwrap_err()
            .starts_with("no such file:"));
        assert!(classify_reference("javascript:alert(1)").is_err());
        assert!(classify_reference("ftp://example.com").is_err());
    }
}
