use crate::api::schema::{
    EmptyParams, Method, PinSectionCreateParams, PinSectionMoveParams, PinSectionRenameParams,
    PinSectionTarget, Request,
};

pub(super) fn pin_source() -> String {
    std::env::var("HERDR_PIN_SOURCE")
        .ok()
        .filter(|source| !source.is_empty())
        .unwrap_or_else(|| "cli".into())
}

pub(super) fn build_method(action: &str, matches: &clap::ArgMatches) -> Result<Method, String> {
    let value = |name: &str| matches.get_one::<String>(name).cloned();
    let name = || value("name").ok_or_else(|| "missing section name".to_owned());
    Ok(match action {
        "list" => Method::PinSectionList(EmptyParams::default()),
        "create" => Method::PinSectionCreate(PinSectionCreateParams {
            name: name()?,
            after: value("after"),
        }),
        "rename" => Method::PinSectionRename(PinSectionRenameParams {
            source: Some(pin_source()),
            name: name()?,
            new_name: value("new_name").ok_or("missing new name")?,
        }),
        "delete" => Method::PinSectionDelete(PinSectionTarget {
            source: Some(pin_source()),
            name: name()?,
        }),
        "move" => Method::PinSectionMove(PinSectionMoveParams {
            source: Some(pin_source()),
            name: name()?,
            index: value("index")
                .and_then(|raw| raw.parse().ok())
                .ok_or("--index must be a non-negative integer")?,
        }),
        _ => return Err(format!("unknown action: {action}")),
    })
}

pub(super) fn render(result: &serde_json::Value) -> String {
    let ids = |value: &serde_json::Value| -> String {
        value
            .as_array()
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| id.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    };
    let mut lines = vec![format!("(unsectioned) {}", ids(&result["unsectioned"]))
        .trim_end()
        .to_owned()];
    for section in result["sections"].as_array().into_iter().flatten() {
        lines.push(
            format!(
                "{}: {}",
                section["name"].as_str().unwrap_or_default(),
                ids(&section["tab_ids"])
            )
            .trim_end()
            .to_owned(),
        );
    }
    lines.join("\n")
}

pub(super) fn run_pin_section_command(args: &[String]) -> std::io::Result<i32> {
    let matches = match super::spec::command().try_get_matches_from(
        ["herdr".to_owned(), "pin-section".to_owned()]
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
        .subcommand_matches("pin-section")
        .and_then(|m| m.subcommand())
    else {
        let mut command = super::spec::command();
        if let Some(section) = command.find_subcommand_mut("pin-section") {
            section.print_help()?;
            println!();
        }
        return Ok(2);
    };
    let method = match build_method(action, matches) {
        Ok(method) => method,
        Err(error) => {
            eprintln!("error: {error}");
            return Ok(1);
        }
    };
    let response = super::send_request(&Request {
        id: format!("cli:pin_section:{action}"),
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
    let result = &response["result"];
    if action == "list" && !matches.get_flag("json") {
        println!("{}", render(result));
    } else {
        println!("{}", serde_json::to_string(result)?);
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method(args: &[&str]) -> Result<Method, String> {
        let matches = super::super::spec::command()
            .try_get_matches_from(
                ["herdr", "pin-section"]
                    .iter()
                    .chain(args)
                    .map(|s| s.to_string()),
            )
            .map_err(|e| e.to_string())?;
        let (action, sub) = matches
            .subcommand_matches("pin-section")
            .and_then(|m| m.subcommand())
            .unwrap();
        build_method(action, sub)
    }

    #[test]
    fn pin_section_cli_builds_each_request() {
        assert_eq!(
            method(&["create", "Northstar: building", "--after", "Needs you"]),
            Ok(Method::PinSectionCreate(PinSectionCreateParams {
                name: "Northstar: building".into(),
                after: Some("Needs you".into()),
            }))
        );
        assert_eq!(
            method(&["rename", "Later", "Someday"]),
            Ok(Method::PinSectionRename(PinSectionRenameParams {
                source: Some("cli".into()),
                name: "Later".into(),
                new_name: "Someday".into(),
            }))
        );
        assert_eq!(
            method(&["move", "Later", "--index", "0"]),
            Ok(Method::PinSectionMove(PinSectionMoveParams {
                source: Some("cli".into()),
                name: "Later".into(),
                index: 0,
            }))
        );
        assert_eq!(
            method(&["delete", "Later"]),
            Ok(Method::PinSectionDelete(PinSectionTarget {
                source: Some("cli".into()),
                name: "Later".into()
            }))
        );
        assert!(method(&["move", "Later", "--index", "x"]).is_err());
        assert_eq!(
            render(&serde_json::json!({
                "unsectioned": ["w1:t1"],
                "sections": [{"name": "Needs you", "tab_ids": ["w1:t2", "w2:t1"]}, {"name": "Later", "tab_ids": []}]
            })),
            "(unsectioned) w1:t1\nNeeds you: w1:t2 w2:t1\nLater:"
        );
    }
}
