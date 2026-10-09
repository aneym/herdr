use super::*;

/// Drains internal app events until `done` sees the one it waits for.
async fn pump_app_events_until(
    server: &mut HeadlessServer,
    mut done: impl FnMut(&crate::events::AppEvent) -> bool,
) -> bool {
    for _ in 0..400 {
        while let Ok(event) = server.app.event_rx.try_recv() {
            let finished = done(&event);
            server.app.handle_internal_event(event);
            if finished {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

/// Renders until the attached client gets a terminal frame whose bytes hold
/// `needle`; returns every terminal frame streamed up to it.
async fn wait_for_terminal_frame(
    server: &mut HeadlessServer,
    render_rx: &std::sync::mpsc::Receiver<Vec<u8>>,
    needle: &str,
) -> Option<Vec<protocol::TerminalFrame>> {
    let mut frames = Vec::new();
    for _ in 0..400 {
        while let Ok(event) = server.app.event_rx.try_recv() {
            server.app.handle_internal_event(event);
        }
        server.render_and_stream();
        while let Ok(bytes) = render_rx.try_recv() {
            if let ServerMessage::Terminal(frame) = read_server_message(bytes) {
                let found = String::from_utf8_lossy(&frame.bytes).contains(needle);
                frames.push(frame);
                if found {
                    return Some(frames);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    None
}

/// A `herdr terminal attach` client (the Mac Shell's pane surface) stays bound to
/// its terminal while `agent.restart` swaps the runtime, then gets a full repaint
/// of the replacement agent, which runs with git discovery capped at `$HOME`.
#[cfg(unix)]
#[tokio::test]
async fn terminal_attach_survives_in_place_agent_restart() {
    let fake_bin = std::env::temp_dir().join(format!("hh-restart-{}", std::process::id()));
    std::fs::create_dir_all(&fake_bin).unwrap();
    // A shell whose argv[0] basename is `claude` stands in for the agent.
    let fake_claude = fake_bin.join("claude");
    let _ = std::fs::remove_file(&fake_claude);
    std::os::unix::fs::symlink("/bin/sh", &fake_claude).unwrap();
    let fake_claude = fake_claude.display().to_string();
    let script = fake_bin.join("stand-in.sh");
    std::fs::write(
        &script,
        "case \":$GIT_CEILING_DIRECTORIES:\" in *\":$HOME:\"*) c=home;; *) c=none;; esac; \
         if [ \"$1\" = --resume ]; then echo \"after-restart ceiling=$c\"; else echo before-restart; fi; \
         sleep 30\n",
    )
    .unwrap();
    let script = script.display().to_string();

    let mut server = test_headless_server();
    let workspace = crate::workspace::Workspace::test_new("restart");
    let pane_id = workspace.tabs[0].root_pane;
    let terminal_id = workspace.terminal_id(pane_id).cloned().unwrap();
    let terminal_id_string = terminal_id.to_string();
    server.app.state.workspaces = vec![workspace];
    server.app.state.active = Some(0);
    server.app.state.ensure_test_terminals();
    server.app.state.default_shell = "/bin/sh".into();
    server.app.state.shell_mode = crate::config::ShellModeConfig::NonLogin;
    {
        let terminal = server.app.state.terminals.get_mut(&terminal_id).unwrap();
        terminal.cwd = std::env::temp_dir();
        // The attach starts the agent through the deferred resume path, like a restored pane.
        terminal.pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
            agent: "claude".into(),
            argv: vec![fake_claude.clone(), script.clone()],
            dedupe_key: "launch".into(),
        });
        // The recorded launcher makes the restart argv `sh <script> --resume sess-1`.
        terminal.launch_argv = Some(vec!["/bin/sh".into(), script.clone()]);
    }

    let (writer, control_rx, render_rx) = test_client_writer();
    assert!(!server.handle_server_event(ServerEvent::ClientConnected {
        client_id: 7,
        cols: 80,
        rows: 24,
        cell_width_px: 0,
        cell_height_px: 0,
        pixel_mouse: false,
        writer,
    }));
    server.handle_server_event(ServerEvent::ClientAttachTerminal {
        client_id: 7,
        terminal_id: terminal_id_string.clone(),
        takeover: false,
    });
    assert!(
        wait_for_terminal_frame(&mut server, &render_rx, "before-restart")
            .await
            .is_some(),
        "attached client never saw the stand-in agent"
    );

    // Report the idle claude session the restart resumes, as the agent's hook would.
    server
        .app
        .state
        .terminals
        .get_mut(&terminal_id)
        .unwrap()
        .set_hook_authority_with_session_ref(
            "herdr:claude".into(),
            "claude".into(),
            crate::detect::AgentState::Idle,
            None,
            crate::agent_resume::AgentSessionRef::id("sess-1"),
            Some(1),
        );
    server
        .app
        .restart_agent_in_place(0, pane_id, false, None)
        .expect("in-place restart starts");
    assert!(server.app.terminal_runtimes.get(&terminal_id).is_none());

    // Render while the old runtime is shutting down: the client must stay attached.
    server.render_and_stream();
    while let Ok(bytes) = control_rx.try_recv() {
        if let ServerMessage::ServerShutdown { reason } = read_server_message(bytes) {
            panic!("attach client was shut down during the restart: {reason:?}");
        }
    }
    assert!(matches!(
        server.clients.get(&7).map(|client| &client.mode),
        Some(ClientConnectionMode::TerminalAttach { terminal_id }) if terminal_id == &terminal_id_string
    ));

    assert!(
        pump_app_events_until(&mut server, |event| matches!(
            event,
            crate::events::AppEvent::AgentRestartShutdownFinished(_)
        ))
        .await,
        "old runtime never finished shutting down"
    );
    let frames = wait_for_terminal_frame(&mut server, &render_rx, "after-restart")
        .await
        .expect("attached client never saw the restarted agent");
    assert!(
        frames[0].full,
        "the replacement runtime is repainted in full"
    );
    let last = String::from_utf8_lossy(&frames[frames.len() - 1].bytes).into_owned();
    assert!(
        last.contains("ceiling=home"),
        "restarted agent runs without a $HOME git ceiling: {last}"
    );
    assert_eq!(
        server.terminal_attach_owners.get(&terminal_id_string),
        Some(&7)
    );

    shutdown_test_runtimes(&mut server);
    let _ = std::fs::remove_dir_all(&fake_bin);
}
