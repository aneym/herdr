use std::process::Command;

/// Exercise the real CLI before any transport connection, without PTYs or fake executables.
/// A missing server distinguishes an allowed stop attempt from a self-stop refusal.
#[test]
fn server_stop_guard_cli() {
    let root = std::env::temp_dir().join(format!("herdr-stop-guard-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("config directory");
    let host = root.join("host.sock");
    let cases: &[(&[&str], bool, bool)] = &[
        (&["server", "stop"], true, true),
        (&["server", "stop", "--session", "other"], true, false),
        (&["server", "stop"], false, false),
        (&["server", "stop", "--force-self"], true, false),
        (&["session", "stop", "other"], true, false),
    ];
    for (args, inside, refused) in cases {
        let mut command = Command::new(env!("CARGO_BIN_EXE_herdr"));
        command
            .args(*args)
            .env("XDG_CONFIG_HOME", &root)
            .env("HERDR_SOCKET_PATH", &host)
            .env_remove("HERDR_SESSION")
            .env_remove("HERDR_CLIENT_SOCKET_PATH")
            .env_remove("HERDR_PANE_ID");
        if *inside {
            command.env("HERDR_PANE_ID", "w1:p1");
        }
        let output = command.output().expect("run CLI");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "no server should be running");
        if *refused {
            assert!(
                stderr.contains("refusing to stop the server hosting"),
                "{args:?}: {stderr}"
            );
            assert!(stderr.contains("--force-self"), "{stderr}");
        } else {
            assert!(!stderr.contains("refusing to stop"), "{args:?}: {stderr}");
            assert!(
                stderr.contains("not running or cannot be reached"),
                "{args:?}: {stderr}"
            );
        }
        if args[0] == "server" && !args.contains(&"--session") {
            assert!(
                stderr.contains("HERDR_SOCKET_PATH takes precedence over XDG_CONFIG_HOME"),
                "{stderr}"
            );
            assert!(stderr.contains(&host.display().to_string()), "{stderr}");
        }
    }
    // Session stop must also protect the hosting session, including its explicit override.
    let app = if cfg!(debug_assertions) {
        "herdr-dev"
    } else {
        "herdr"
    };
    let session_socket = root.join(app).join("sessions/other/herdr.sock");
    for force in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_herdr"));
        command
            .args(["session", "stop", "other"])
            .env("XDG_CONFIG_HOME", &root)
            .env("HERDR_SOCKET_PATH", &session_socket)
            .env("HERDR_PANE_ID", "w1:p1")
            .env_remove("HERDR_SESSION")
            .env_remove("HERDR_CLIENT_SOCKET_PATH");
        if force {
            command.arg("--force-self");
        }
        let output = command.output().expect("run session stop");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            stderr.contains("refusing to stop the server hosting"),
            !force,
            "{stderr}"
        );
        if force {
            assert!(
                stderr.contains("not running or cannot be reached"),
                "{stderr}"
            );
        }
    }
    std::fs::remove_dir_all(root).expect("remove test config");
}
