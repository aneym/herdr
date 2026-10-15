use crate::api::schema::{DeskTarget, Method, Request, ResponseResult, TabDocSetParams};

fn resolve_target(
    tab: Option<String>,
    pane: Option<String>,
    own_pane: Option<&str>,
) -> Result<DeskTarget, String> {
    if let Some(tab_id) = tab {
        return Ok(DeskTarget {
            tab_id: Some(super::normalize_tab_id(&tab_id)),
            pane_id: None,
        });
    }
    let pane_id = pane
        .map(|id| super::normalize_pane_id(&id))
        .or_else(|| own_pane.map(str::to_owned))
        .ok_or("not in a herdr pane; pass a tab id or --tab")?;
    Ok(DeskTarget {
        tab_id: None,
        pane_id: Some(pane_id),
    })
}

pub(super) fn build_method(
    action: &str,
    matches: &clap::ArgMatches,
    own_pane: Option<&str>,
) -> Result<Method, String> {
    let option = |name: &str| matches.get_one::<String>(name).cloned();
    if option("tab").is_some() && option("pane").is_some() {
        return Err("pass --tab or --pane, not both".into());
    }
    match action {
        "set" => {
            let args: Vec<String> = matches
                .get_many::<String>("args")
                .map(|values| values.cloned().collect())
                .unwrap_or_default();
            let (positional_tab, raw) = match args.as_slice() {
                [raw] => (None, raw.clone()),
                [tab, raw] => (Some(tab.clone()), raw.clone()),
                _ => return Err("expected [TAB_ID] URL_OR_PATH".into()),
            };
            if positional_tab.is_some() && (option("tab").is_some() || option("pane").is_some()) {
                return Err("pass the tab once".into());
            }
            let target =
                resolve_target(positional_tab.or(option("tab")), option("pane"), own_pane)?;
            Ok(Method::TabDocSet(TabDocSetParams {
                target,
                url: super::desk::classify_reference(raw.trim())?,
                title: option("title"),
                set_by: Some(
                    own_pane
                        .map(|id| format!("pane:{id}"))
                        .unwrap_or_else(|| "cli".into()),
                ),
            }))
        }
        "get" | "clear" => {
            let positional = option("tab_id");
            if positional.is_some() && (option("tab").is_some() || option("pane").is_some()) {
                return Err("pass the tab once".into());
            }
            let target = resolve_target(positional.or(option("tab")), option("pane"), own_pane)?;
            Ok(if action == "get" {
                Method::TabDocGet(target)
            } else {
                Method::TabDocClear(target)
            })
        }
        _ => Err(format!("unknown action: {action}")),
    }
}

pub(super) fn render(result: &ResponseResult) -> Option<String> {
    let ResponseResult::TabDoc { tab_id, doc, .. } = result else {
        return None;
    };
    Some(match doc {
        Some(doc) => format!("{tab_id} {} {}", doc.url, doc.title),
        None => format!("{tab_id} none"),
    })
}

pub(super) fn run_tab_doc_command(args: &[String]) -> std::io::Result<i32> {
    let matches = match super::spec::command().try_get_matches_from(
        ["herdr".to_owned(), "tab".to_owned(), "doc".to_owned()]
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
        .subcommand_matches("tab")
        .and_then(|m| m.subcommand_matches("doc"))
        .and_then(|m| m.subcommand())
    else {
        let mut command = super::spec::command();
        if let Some(doc) = command
            .find_subcommand_mut("tab")
            .and_then(|tab| tab.find_subcommand_mut("doc"))
        {
            doc.print_help()?;
            println!();
        }
        return Ok(2);
    };
    let own_pane = std::env::var("HERDR_PANE_ID")
        .ok()
        .filter(|id| !id.is_empty());
    let method = match build_method(action, matches, own_pane.as_deref()) {
        Ok(method) => method,
        Err(error) => {
            eprintln!("error: {error}");
            return Ok(1);
        }
    };
    let response = super::send_request(&Request {
        id: format!("cli:tab:doc_{action}"),
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
        .ok_or_else(|| std::io::Error::other("missing tab doc result"))?;
    if matches.get_flag("json") {
        println!("{}", serde_json::to_string(result)?);
    } else {
        let result: ResponseResult = serde_json::from_value(result.clone())?;
        if let Some(line) = render(&result) {
            println!("{line}");
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method(args: &[&str], own_pane: Option<&str>) -> Result<Method, String> {
        let matches = super::super::spec::command()
            .try_get_matches_from(["herdr", "tab", "doc"].iter().chain(args))
            .map_err(|error| error.to_string())?;
        let (action, matches) = matches
            .subcommand_matches("tab")
            .and_then(|m| m.subcommand_matches("doc"))
            .and_then(|m| m.subcommand())
            .unwrap();
        build_method(action, matches, own_pane)
    }

    #[test]
    fn cli_tab_doc_targets_own_tab_or_named_tab() {
        let own = method(
            &["set", "https://Example.com/Plan", "--title", "Plan"],
            Some("w5H:p1"),
        );
        assert_eq!(
            own.unwrap(),
            Method::TabDocSet(TabDocSetParams {
                target: DeskTarget {
                    tab_id: None,
                    pane_id: Some("w5H:p1".into())
                },
                url: "https://Example.com/Plan".into(),
                title: Some("Plan".into()),
                set_by: Some("pane:w5H:p1".into()),
            })
        );
        let named = method(&["set", "w5H:t1FF", "https://example.com/x"], None).unwrap();
        let Method::TabDocSet(params) = named else {
            panic!()
        };
        assert_eq!(params.target.tab_id.as_deref(), Some("w5H:t1FF"));
        assert_eq!(params.set_by.as_deref(), Some("cli"));
        let flagged = method(&["set", "--tab", "w5H:t1FF", "HTTPS://example.com/x"], None).unwrap();
        let Method::TabDocSet(params) = flagged else {
            panic!()
        };
        assert_eq!(params.url, "https://example.com/x");

        assert_eq!(
            method(&["get"], Some("w1:p2")).unwrap(),
            Method::TabDocGet(DeskTarget {
                tab_id: None,
                pane_id: Some("w1:p2".into())
            })
        );
        assert_eq!(
            method(&["clear", "w1:t3"], None).unwrap(),
            Method::TabDocClear(DeskTarget {
                tab_id: Some("w1:t3".into()),
                pane_id: None
            })
        );
    }

    #[test]
    fn cli_tab_doc_rejects_missing_target_and_bad_refs() {
        assert!(method(&["set", "https://example.com"], None)
            .unwrap_err()
            .contains("not in a herdr pane"));
        assert!(method(&["get"], None).is_err());
        assert!(method(&["set", "w1:t1", "https://e.com", "--tab", "w1:t2"], None).is_err());
        assert!(method(&["set", "ftp://example.com/x"], Some("w1:p1"))
            .unwrap_err()
            .contains("unsupported scheme"));
        assert!(method(&["set", "/no/such/herdr-doc.md"], Some("w1:p1"))
            .unwrap_err()
            .contains("no such file"));
        assert!(method(&["set"], Some("w1:p1")).is_err());
        assert!(method(&["set", "a", "b", "c"], Some("w1:p1")).is_err());
    }

    #[test]
    fn cli_tab_doc_text_output() {
        let set = ResponseResult::TabDoc {
            tab_id: "w1:t1".into(),
            workspace_id: "w1".into(),
            doc: Some(crate::api::schema::TabDoc {
                url: "https://example.com/x".into(),
                title: "Rails scope".into(),
                set_by: None,
                set_at_ms: 0,
            }),
        };
        assert_eq!(
            render(&set).unwrap(),
            "w1:t1 https://example.com/x Rails scope"
        );
        let none = ResponseResult::TabDoc {
            tab_id: "w1:t1".into(),
            workspace_id: "w1".into(),
            doc: None,
        };
        assert_eq!(render(&none).unwrap(), "w1:t1 none");
    }
}
