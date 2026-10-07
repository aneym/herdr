use std::collections::HashMap;

use crate::api::schema::{
    Method, Request, TabCreateParams, TabListParams, TabMoveParams, TabPinMoveParams,
    TabRenameParams, TabSetPinnedParams, TabTarget,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Placement {
    Before(String),
    After(String),
    Position(usize),
}

pub(super) fn run_tab_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(|arg| arg.as_str()) else {
        print_tab_help();
        return Ok(2);
    };

    match subcommand {
        "list" => tab_list(&args[1..]),
        "create" => tab_create(&args[1..]),
        "get" => tab_get(&args[1..]),
        "focus" => tab_focus(&args[1..]),
        "rename" => tab_rename(&args[1..]),
        "move" => tab_move(&args[1..]),
        "pin" => tab_pin(&args[1..]),
        "unpin" => tab_unpin(&args[1..]),
        "set-role" => tab_set_role(&args[1..]),
        "pin-move" => tab_pin_move(&args[1..]),
        "close" => tab_close(&args[1..]),
        "help" | "--help" | "-h" => {
            print_tab_help();
            Ok(0)
        }
        _ => {
            print_tab_help();
            Ok(2)
        }
    }
}

fn tab_list(args: &[String]) -> std::io::Result<i32> {
    let mut workspace_id = None;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --workspace");
                    return Ok(2);
                };
                workspace_id = Some(super::normalize_workspace_id(value));
                index += 2;
            }
            other => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
        }
    }

    super::runtime::tab_list(TabListParams { workspace_id })
}

fn tab_create(args: &[String]) -> std::io::Result<i32> {
    let mut workspace_id = None;
    let mut cwd = None;
    let mut focus = false;
    let mut label = None;
    let mut env = HashMap::new();

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --workspace");
                    return Ok(2);
                };
                workspace_id = Some(super::normalize_workspace_id(value));
                index += 2;
            }
            "--cwd" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --cwd");
                    return Ok(2);
                };
                cwd = Some(value.clone());
                index += 2;
            }
            "--label" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --label");
                    return Ok(2);
                };
                label = Some(value.clone());
                index += 2;
            }
            "--focus" => {
                focus = true;
                index += 1;
            }
            "--no-focus" => {
                focus = false;
                index += 1;
            }
            "--env" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --env");
                    return Ok(2);
                };
                let (key, value) = match super::parse_env_assignment(value) {
                    Ok(pair) => pair,
                    Err(err) => {
                        eprintln!("{err}");
                        return Ok(2);
                    }
                };
                env.insert(key, value);
                index += 2;
            }
            other => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
        }
    }

    super::runtime::tab_create(TabCreateParams {
        workspace_id,
        cwd,
        focus,
        label,
        env,
    })
}

fn tab_get(args: &[String]) -> std::io::Result<i32> {
    let Some(raw_tab_id) = args.first() else {
        eprintln!("usage: herdr tab get <tab_id>");
        return Ok(2);
    };
    if args.len() != 1 {
        eprintln!("usage: herdr tab get <tab_id>");
        return Ok(2);
    }

    super::runtime::tab_get(super::normalize_tab_id(raw_tab_id))
}

fn tab_focus(args: &[String]) -> std::io::Result<i32> {
    let Some(raw_tab_id) = args.first() else {
        eprintln!("usage: herdr tab focus <tab_id>");
        return Ok(2);
    };
    if args.len() != 1 {
        eprintln!("usage: herdr tab focus <tab_id>");
        return Ok(2);
    }

    super::runtime::tab_focus(super::normalize_tab_id(raw_tab_id))
}

fn tab_rename(args: &[String]) -> std::io::Result<i32> {
    if args.len() < 2 {
        eprintln!("usage: herdr tab rename <tab_id> <label>");
        return Ok(2);
    }

    super::runtime::tab_rename(TabRenameParams {
        tab_id: super::normalize_tab_id(&args[0]),
        label: args[1..].join(" "),
    })
}

fn parse_move_args(args: &[String]) -> Result<(String, Placement), String> {
    let Some(tab_id) = args.first() else {
        return Err("usage: herdr tab move <tab_id> (--before <tab_id> | --after <tab_id> | --position <N>)".into());
    };
    let mut placement = None;
    let mut options = args[1..].chunks_exact(2);
    for pair in &mut options {
        if placement.is_some() {
            return Err("choose exactly one of --before, --after, or --position".into());
        }
        placement = Some(match pair[0].as_str() {
            "--before" => Placement::Before(super::normalize_tab_id(&pair[1])),
            "--after" => Placement::After(super::normalize_tab_id(&pair[1])),
            "--position" => {
                let position = pair[1]
                    .parse::<usize>()
                    .map_err(|_| "--position must be a positive integer")?;
                if position == 0 {
                    return Err("--position must be at least 1".into());
                }
                Placement::Position(position)
            }
            other => return Err(format!("unknown option: {other}")),
        });
    }
    if !options.remainder().is_empty() {
        return Err("missing value for move option or unexpected argument".into());
    }
    let placement = placement.ok_or("choose exactly one of --before, --after, or --position")?;
    Ok((super::normalize_tab_id(tab_id), placement))
}

fn same_workspace(source: &str, other: &str) -> Result<(), String> {
    if source == other {
        Ok(())
    } else {
        Err(format!(
            "cannot move tabs across workspaces ({source} and {other})"
        ))
    }
}

// The server interprets insert_index in the original list, then subtracts one
// when removing a tab that precedes the insertion point.
fn move_insert_index(
    order: &[String],
    tab_id: &str,
    placement: &Placement,
) -> Result<usize, String> {
    let source = order
        .iter()
        .position(|id| id == tab_id)
        .ok_or("tab not in workspace")?;
    let insert_index = match placement {
        Placement::Before(other) => order
            .iter()
            .position(|id| id == other)
            .ok_or("reference tab not in workspace")?,
        Placement::After(other) => {
            order
                .iter()
                .position(|id| id == other)
                .ok_or("reference tab not in workspace")?
                + 1
        }
        Placement::Position(position) => {
            let final_index = position.saturating_sub(1).min(order.len() - 1);
            final_index + usize::from(source < final_index)
        }
    };
    Ok(insert_index)
}

fn response_field<'a>(response: &'a serde_json::Value, field: &str) -> std::io::Result<&'a str> {
    response["result"]["tab"][field]
        .as_str()
        .ok_or_else(|| std::io::Error::other(format!("tab.get response missing {field}")))
}

fn tab_move(args: &[String]) -> std::io::Result<i32> {
    let (tab_id, placement) = match parse_move_args(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}");
            return Ok(2);
        }
    };
    let source = super::send_request(&Request {
        id: "cli:tab:move:source".into(),
        method: Method::TabGet(TabTarget { tab_id }),
    })?;
    if source.get("error").is_some() {
        return super::print_response(&source);
    }
    let source_id = response_field(&source, "tab_id")?;
    let workspace_id = response_field(&source, "workspace_id")?;
    let placement = match &placement {
        Placement::Before(other) | Placement::After(other) if other == source_id => {
            placement.clone()
        }
        Placement::Before(other) | Placement::After(other) => {
            let reference = super::send_request(&Request {
                id: "cli:tab:move:reference".into(),
                method: Method::TabGet(TabTarget {
                    tab_id: other.clone(),
                }),
            })?;
            if reference.get("error").is_some() {
                return super::print_response(&reference);
            }
            if let Err(message) =
                same_workspace(workspace_id, response_field(&reference, "workspace_id")?)
            {
                eprintln!("{message}");
                return Ok(1);
            }
            let reference_id = response_field(&reference, "tab_id")?.to_string();
            match placement {
                Placement::Before(_) => Placement::Before(reference_id),
                Placement::After(_) => Placement::After(reference_id),
                Placement::Position(_) => unreachable!(),
            }
        }
        Placement::Position(_) => placement.clone(),
    };
    let list = super::send_request(&Request {
        id: "cli:tab:move:list".into(),
        method: Method::TabList(TabListParams {
            workspace_id: Some(workspace_id.to_string()),
        }),
    })?;
    if list.get("error").is_some() {
        return super::print_response(&list);
    }
    let tabs = list["result"]["tabs"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("tab.list response missing tabs"))?;
    let order: Vec<String> = tabs
        .iter()
        .map(|tab| {
            tab["tab_id"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| std::io::Error::other("tab.list response missing tab_id"))
        })
        .collect::<std::io::Result<_>>()?;
    let insert_index = match move_insert_index(&order, source_id, &placement) {
        Ok(index) => index,
        Err(message) => {
            eprintln!("{message}");
            return Ok(1);
        }
    };
    if matches!(&placement, Placement::Before(other) | Placement::After(other) if other == source_id)
    {
        return super::print_response(&list);
    }
    super::runtime::tab_move(TabMoveParams {
        tab_id: source_id.to_string(),
        insert_index,
    })
}

fn tab_pin(args: &[String]) -> std::io::Result<i32> {
    let Some(raw_tab_id) = args.first() else {
        eprintln!("usage: herdr tab pin <tab_id> [--priority N]");
        return Ok(2);
    };
    let mut priority = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--priority" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --priority");
                    return Ok(2);
                };
                match value.parse::<i64>() {
                    Ok(parsed) => priority = Some(parsed),
                    Err(_) => {
                        eprintln!("--priority must be an integer");
                        return Ok(2);
                    }
                }
                index += 2;
            }
            other => {
                eprintln!("unknown option: {other}");
                return Ok(2);
            }
        }
    }

    super::runtime::tab_set_pinned(TabSetPinnedParams {
        tab_id: super::normalize_tab_id(raw_tab_id),
        pinned: true,
        priority,
    })
}

fn tab_unpin(args: &[String]) -> std::io::Result<i32> {
    let Some(raw_tab_id) = args.first() else {
        eprintln!("usage: herdr tab unpin <tab_id>");
        return Ok(2);
    };
    if args.len() != 1 {
        eprintln!("usage: herdr tab unpin <tab_id>");
        return Ok(2);
    }

    super::runtime::tab_set_pinned(TabSetPinnedParams {
        tab_id: super::normalize_tab_id(raw_tab_id),
        pinned: false,
        priority: None,
    })
}

fn tab_set_role(args: &[String]) -> std::io::Result<i32> {
    if args.len() != 2 || !matches!(args[1].as_str(), "agent" | "none") {
        eprintln!("usage: herdr tab set-role <tab_id> <agent|none>");
        return Ok(2);
    }
    super::runtime::tab_set_role(crate::api::schema::TabSetRoleParams {
        tab_id: super::normalize_tab_id(&args[0]),
        role: (args[1] == "agent").then_some(crate::api::schema::TabRole::Agent),
    })
}

fn tab_pin_move(args: &[String]) -> std::io::Result<i32> {
    let [raw_tab_id, raw_index] = args else {
        eprintln!("usage: herdr tab pin-move <tab_id> <pin_index>");
        return Ok(2);
    };
    let Ok(pin_index) = raw_index.parse::<usize>() else {
        eprintln!("pin_index must be a non-negative integer (0 is the top pin)");
        return Ok(2);
    };
    super::runtime::tab_pin_move(TabPinMoveParams {
        tab_id: super::normalize_tab_id(raw_tab_id),
        pin_index,
    })
}

fn tab_close(args: &[String]) -> std::io::Result<i32> {
    let Some(raw_tab_id) = args.first() else {
        eprintln!("usage: herdr tab close <tab_id>");
        return Ok(2);
    };
    if args.len() != 1 {
        eprintln!("usage: herdr tab close <tab_id>");
        return Ok(2);
    }

    super::runtime::tab_close(super::normalize_tab_id(raw_tab_id))
}

fn print_tab_help() {
    eprintln!("herdr tab commands:");
    eprintln!("  herdr tab list [--workspace <workspace_id>]");
    eprintln!(
        "  herdr tab create [--workspace <workspace_id>] [--cwd PATH] [--label TEXT] [--env KEY=VALUE] [--focus] [--no-focus]"
    );
    eprintln!("  herdr tab get <tab_id>");
    eprintln!("  herdr tab focus <tab_id>");
    eprintln!("  herdr tab rename <tab_id> <label>");
    eprintln!("  herdr tab move <tab_id> (--before <tab_id> | --after <tab_id> | --position <N>)");
    eprintln!("  herdr tab pin <tab_id> [--priority N]");
    eprintln!("  herdr tab unpin <tab_id>");
    eprintln!("  herdr tab set-role <tab_id> <agent|none>");
    eprintln!("  herdr tab pin-move <tab_id> <pin_index>");
    eprintln!("  herdr tab close <tab_id>");
}

#[cfg(test)]
mod tests {
    use super::{move_insert_index, parse_move_args, same_workspace, Placement};

    #[test]
    fn move_index_matches_server_remove_then_insert_order() {
        let order = ["a", "b", "c", "d"].map(str::to_string);
        for (source, placement, expected) in [
            ("a", Placement::After("c".into()), ["b", "c", "a", "d"]),
            ("d", Placement::Before("b".into()), ["a", "d", "b", "c"]),
            ("c", Placement::Position(1), ["c", "a", "b", "d"]),
            ("a", Placement::Position(99), ["b", "c", "d", "a"]),
            ("d", Placement::Position(99), ["a", "b", "c", "d"]),
        ] {
            let insert_index = move_insert_index(&order, source, &placement).unwrap();
            let source_index = order.iter().position(|id| id == source).unwrap();
            let destination = if source_index < insert_index {
                insert_index - 1
            } else {
                insert_index
            }
            .min(order.len() - 1);
            let mut final_order = order.to_vec();
            let moved = final_order.remove(source_index);
            final_order.insert(destination, moved);
            assert_eq!(final_order, expected, "{source:?} {placement:?}");
        }
    }

    #[test]
    fn move_arguments_require_one_placement_and_reject_cross_workspace() {
        let parse = |args: &[&str]| {
            parse_move_args(&args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>())
        };
        assert_eq!(
            parse(&["w5H:tE7", "--after", "w5H:tE8"]),
            Ok(("w5H:tE7".into(), Placement::After("w5H:tE8".into())))
        );
        assert_eq!(
            parse(&["w5H:tE7", "--position", "1"]),
            Ok(("w5H:tE7".into(), Placement::Position(1)))
        );
        for args in [
            &[][..],
            &["w5H:tE7"][..],
            &["w5H:tE7", "--before", "w5H:tE8", "--after", "w5H:tE9"],
            &["w5H:tE7", "--position", "0"],
            &["w5H:tE7", "--position", "oops"],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
        assert!(same_workspace("w5H", "w6Q")
            .unwrap_err()
            .contains("across workspaces"));
        assert!(same_workspace("w5H", "w5H").is_ok());
        for args in [&[][..], &["--before", "w5H:tE8", "--after", "w5H:tE9"][..]] {
            let mut cli_args = vec!["herdr", "tab", "move", "w5H:tE7"];
            cli_args.extend(args);
            assert!(super::super::spec::command()
                .try_get_matches_from(cli_args)
                .is_err());
        }
        assert!(super::super::spec::command()
            .try_get_matches_from(["herdr", "tab", "move", "w5H:tE7", "--before", "w5H:tE8"])
            .is_ok());
    }
    /// CLI grammar is the public invocation contract, including rejecting unknown roles.
    #[test]
    fn cli_tab_set_role_accepts_agent_and_none() {
        for role in ["agent", "none"] {
            assert!(super::super::spec::command()
                .try_get_matches_from(["herdr", "tab", "set-role", "w1:t1", role])
                .is_ok());
        }
        for args in [
            vec!["herdr", "tab", "set-role", "w1:t1"],
            vec!["herdr", "tab", "set-role", "w1:t1", "future"],
        ] {
            assert!(super::super::spec::command()
                .try_get_matches_from(args)
                .is_err());
        }
    }
}
