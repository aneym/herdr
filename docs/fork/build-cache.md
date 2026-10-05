# Fork build cache

On Studio, `just build` (and the other Cargo recipes) exports
`CARGO_TARGET_DIR=/Volumes/StudioExt/repos/herdr-target` for every fork worktree
under `/Volumes/StudioExt/repos`, with `sccache` as `RUSTC_WRAPPER` when available.
Cargo locks the shared target directory, so concurrent builds serialize.
Caller-provided environment values win; CI and other machines keep `target/`
and no wrapper. Unavailable sharing or sccache logs a warning and falls back.

Install the optional cache with `brew install sccache`, then use `just build`.
Direct Cargo invocations bypass just; opt in explicitly with the same environment
values. Do not delete another build's shared artifacts with `cargo clean`.
The vendored Zig build still writes its own worktree-local output.
