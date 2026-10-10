//! ssh(1) argument parsing is a pure parser with flag-value edge cases the
//! headless scenario (server/headless/tests/runs_on.rs) cannot enumerate.
use super::ssh_destination;

#[test]
fn ssh_destination_skips_options_and_their_values() {
    let cases: &[(&[&str], Option<&str>)] = &[
        (&["ssh", "macbook-ts"], Some("macbook-ts")),
        (&["/usr/bin/ssh", "-t", "pc-wsl", "herdr"], Some("pc-wsl")),
        (
            &["ssh", "-p", "2222", "-i", "~/.ssh/id", "jobs@forge"],
            Some("jobs@forge"),
        ),
        (
            &["ssh", "-p2222", "-o", "ConnectTimeout=5", "ax42"],
            Some("ax42"),
        ),
        (
            &["ssh", "-J", "jump", "-A", "box-1", "--", "x"],
            Some("box-1"),
        ),
        (&["ssh", "-v"], None),
        (&["sshd", "host"], None),
        (&["claude", "--resume", "id"], None),
    ];
    for (argv, want) in cases {
        let argv: Vec<String> = argv.iter().map(|arg| arg.to_string()).collect();
        assert_eq!(ssh_destination(&argv), *want, "{argv:?}");
    }
}
