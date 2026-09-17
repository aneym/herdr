# Fork feature ledger for the 0.9.1 port

Upstream `207be3c7 refactor: render the shell in the client (#3487)` (0.9.0)
moved the whole interaction and render layer out of the server
(`src/app/input/`, `src/ui/`) into `src/client/shell/`. Stage 1 of this port
merged upstream v0.9.1 into the fork, kept every fork feature that lives
outside that relocated layer, and removed the rest cleanly. Every removal in
the tree carries a `// PORT-0.9: <feature> UI pending (docs/fork/port-0.9/PORT.md)`
marker.

Stage 2 agents work from this file. For each feature the **fork source**
column points at the verbatim pre-merge file under `orig/`, with the unified
diff against the merge base under `diff/<path>.forkdiff`.

Status values:

- **carried** — present and compiling on `port-0.9`.
- **needs-port** — data or API side carried; the UI/input half must be
  rebuilt in `src/client/shell/`.
- **superseded-by-upstream** — upstream 0.9 ships an equivalent; use theirs.
- **dropped** — removed with a reason; nothing to rebuild unless Alex asks.

The base for every fork diff is `d79fd746` (herdr 0.8.2 merge base); fork
`main` is `bacbdfa2`; upstream is `5f3763dd` (v0.9.1).

## Ledger

| # | Feature | Fork commits | Status | Fork source (orig/ + fns) | Upstream target + anchors | Notes |
|---|---------|--------------|--------|---------------------------|---------------------------|-------|
| 1 | Workspace profiles: data model, visibility, API, persistence | `1911a8ac`, `4b146c03`, `8d50ec38`, `fafa3a70`, `f9872d5c` | carried | `orig/src/app/state.rs` (`profile_roster`, `set_pane_profiles`, `workspace_is_visible`, `switch_profile`, `reveal_workspace`, `settle_active_workspace_visibility`), `orig/src/api/schema/workspaces.rs` | server-side, unchanged | `active_profile` is in `SessionSnapshot` and in `AppState`; `workspace.list --visible-only`, `workspace.set_profiles`, `pane.set_profiles` all live. |
| 2 | Profile send/share context menus | `2777d8b9` | needs-port | `orig/src/app/state.rs` (`ProfileMenuState`, `ProfileMenuTarget`, `ProfileMenuMode`, `profile_menu_entries`, `open_profile_menu`, `refresh_profile_menu_entries`), `orig/src/app/input/mouse.rs` | `src/client/shell/context_menu.rs`, `src/client/shell/mouse.rs` | `profile_roster()` is restored on `AppState` as the data source; only the menu state and rendering were dropped. |
| 3 | Durable agent ownership + orchestrator groups | `61669bf9`, `b1b03f47`, `66237c06` | carried (stage 2) | `src/agent_ownership.rs` (fork-only, untouched by the merge), `orig/src/app/api/agents.rs` (`handle_agent_owner_set`, `handle_agent_owner_clear`) | rows: `src/client/shell/agent_sidebar.rs`, `src/client/shell/sidebar.rs` | The resolved owner pane, the orphan flag, `orchestrator_mode` and the workspace tab count now ride in `ClientShellSnapshot` behind `#[serde(default)]`; `tree::arrange_agent_hierarchy` rebuilds the nesting client-side, with tree guides, an orphan marker, the `[N]` orchestrator count and a right-aligned group chevron. |
| 4 | Agent usage sampling | `75f812fd` | carried (API) / needs-port (overlay) | `orig/src/app/usage.rs` (`UsageSampler`, `collect_agent_usage`, `toggle_usage_overlay`, `refresh_usage_overlay`), `orig/src/ui/...` | overlay: `src/client/shell/overlays.rs`, `src/client/shell/overlay_input.rs` | `agent.usage` over the socket still returns the same rows. `Mode::Usage`, `UsageState` and the refresh timer were dropped. |
| 5 | Clipboard image ingestion API | `0184adf4`, `3c99c37b` | carried | `orig/src/api/server.rs` (`handle_clipboard_image_write`), `src/server/clipboard_image.rs` | server-side, unchanged | `ResponseResult::ClipboardImageWritten` kept alongside upstream's new `ClientShellSurfaceSet`. |
| 6 | Terminal-title persistence | `e55ad431` | carried | `orig/src/persist/snapshot.rs` | server-side, unchanged | Snapshot still records terminal titles, so sidebar thread titles survive restart once the sidebar is ported. |
| 7 | Deferred attention-read (`ui.attention_read`) | part of `e838a372`, `3ed0689a` | carried (data) / needs-port (unfocus half) | `orig/src/app/actions.rs` (`update_attention_read_for_focus_change`, `mark_deferred_attention_read`, `pane_attention_generation`, `read_focused_attention`, `leave_focused_attention`) | client focus changes: `src/client/shell/input.rs`, `src/client/shell/actions.rs` | `leave_focused_attention` is `#[allow(dead_code)]` until the client calls it on unfocus. |
| 8 | Pane focus history + `last_pane` | part of `08704fd6` | carried | `orig/src/app/actions.rs` (`record_focus_history_change`, `focus_history_target`, `focus_back`, `focus_forward`, `last_pane`), `PaneFocusHistory` in `orig/src/app/state.rs` | keybinding: `src/client/shell/input.rs` | History is recorded on every focus change through `record_pane_focus_after_navigation`. `focus_history_target` is `#[allow(dead_code)]` until a client key binds it. |
| 9 | Mouse back/forward buttons drive focus history | `08704fd6` | needs-port | `orig/src/raw_input.rs` (`MouseNavButton`, `nav_button_from_cb`), `orig/src/app/runtime.rs` (`handle_mouse_nav_button`) | `src/client/shell/input.rs`, `src/client/shell/mouse.rs` | Buttons 8/9 are still parsed into `RawInputEvent::MouseNavButton`; both the client and the server input path now ignore it with a PORT-0.9 marker. Upstream has no equivalent. |
| 10 | Sidebar tree (spaces/tabs/agents) + attention-aware ⌘E | `e838a372`, `ca788cd8`, `3ed0689a`, `44411943` | carried (stage 2) | `orig/src/ui/sidebar.rs` (6040 lines), `orig/src/app/actions.rs` (`cycle_agent_entry`, `agent_cycle_target`, `cycle_attention_rank`, `focus_agent_entry`, `ensure_agent_panel_entry_visible`) | `src/client/shell/sidebar.rs`, `src/client/shell/agent_sidebar.rs` | Rebuilt in `src/client/shell/tree.rs` over `ClientShellSnapshot`. The tree layer toggles and collapse sets moved to per-client chrome state (stage-2 decision 6). `Triage` now sorts the panel and `Tree` groups it; the sort control cycles all four values. |
| 11 | Pinned spaces, hidden section | `96d05fa3`, `5d4a5203`, `b00105bd`, `c98b5ea1`, `ead187a2` | carried (stage 2) | `orig/src/ui/sidebar.rs`, `orig/src/app/api.rs` (`respawn_tab_for_pinned_workspace`) | `src/client/shell/sidebar.rs` | The pin toggle, the hidden section and space drag-to-reorder are client-side; the pin also mirrors to the endpoint through the new `workspace.set_pinned` method (decision 9), so `respawn_tab_for_pinned_workspace` still keeps a live tab. |
| 12 | Tab status glyphs (`ui.show_tab_status`) | `ca788cd8` | carried (stage 2) | `orig/src/ui/tabs.rs` | `src/client/shell/tabs.rs` (`tab_status_glyphs`) | One glyph per pane, gated on the tab's roll-up status by the four modes. `ui.sidebar.agents.state_icons` now reaches every glyph in the client shell through `resolved_status_icon`, including space and agent rows. |
| 13 | Space header `+` button / next-numbered tab | `f71ee286`, `51d4cf4c` | carried (stage 2) | `orig/src/ui/sidebar.rs` (`tree_header_plus_rect`, `TreeHeaderHit::NewTab`), `orig/src/app/creation.rs` (`next_new_tab_default_name`) | `src/client/shell/sidebar.rs`, `src/client/shell/mouse.rs` | The plus sends upstream's `tab.create` with no label, so the endpoint auto-names the next tab. It never prompts, even with `ui.prompt_new_tab_name` on. |
| 14 | ⌘C copy bridge into mouse-reporting pane apps | `8ad4ec54`, `e7858883`, `889e012b`, `338fa73f` | superseded-by-upstream / needs-port | `orig/src/app/input/mouse.rs` (`pane_app_drag_shadow`, `pane_app_drag_copy`, `pane_app_pending_word_copy`) | `src/client/shell/mouse.rs`, `src/client/shell/word_selection.rs`, `src/client/shell/copy_mode.rs` | Upstream 0.9 ships its own double-click word selection and copy-on-select. Re-check against upstream before rebuilding: the agent-interrupt guard (`e7858883`) may still be wanted. |
| 15 | Pane hover copy-location control | `d2672a12`, `c96b357b`, `e462ca83`, `60e0f6f5` | needs-port | `orig/src/app/input/mouse.rs`, `orig/src/ui/panes.rs` (`pane_hover`, `pane_copy_presses`) | `src/client/shell/mouse.rs`, `src/client/shell/render.rs` | Clipboard feedback plumbing (`AppEvent::ClipboardWrite.feedback`, `show_clipboard_feedback`) was dropped: upstream's client owns copy feedback (`src/client/shell/state.rs: copy_feedback`). |
| 16 | Triple-click selects and copies the line | `209f218c` | needs-port | `orig/src/app/actions.rs` (`select_line_at_pane_cell`, `url_at_pane_cell`, `copy_selection`) | `src/client/shell/word_selection.rs`, `src/client/shell/mouse.rs` | Upstream has double-click word selection but no triple-click line selection. |
| 17 | Bracketed-paste isolation (PTY input barrier) | `76321b59` | superseded-by-upstream | `orig/src/pty/actor/unix.rs` (`WriteUserInputBarrier`, `input_barrier_active`, `input_barrier_release_at`), `orig/src/pane.rs` (`send_bytes_with_barrier`) | `src/pty/actor/unix.rs` (`PtyIoDataCommand::SubmitUserInput`, `ActiveSubmission`, `SubmissionPhase`) | Upstream's ordered submission solves the same problem and is what `agent prompt` uses now. Verify the paste-ordering symptom is gone before closing this out. |
| 18 | OSC title evidence through first agent acquisition | `a67efd92` | superseded-by-upstream | `orig/src/pane.rs` (`agent_change_clears_osc_evidence`) | `src/pane.rs` (`clear_osc_evidence_for_agent_transition`) | Same semantics, upstream's shape. The fork's duplicate test was removed. |
| 19 | Keep close-pane focus in tab when siblings remain | `0d5a3d76` | carried | `orig/src/app/actions.rs` (`close_pane`) | server-side, unchanged | Verified by upstream's own close-pane tests. |
| 20 | `ui.agent_close_focus = "panel_next"` | part of `e838a372` | carried (stage 2) | `orig/src/app/actions.rs` (`panel_next_agent_close_target`, `focus_panel_agent_after_close`) | `src/client/shell/actions.rs` (`panel_next_close_target`) | The client queues a `pane.focus` behind its own close on the same ordered connection, so the decision uses the panel order the client owns. Five tests replace the four fork tests dropped in stage 1. |
| 21 | Content-fit spaces sidebar + automations section | `a045b2b4`, `4b175bef`, `1f0c5ba9`, `4cbdb86e`, `2f6d58ba` | carried (stage 2) | `orig/src/ui/sidebar.rs` | `src/client/shell/sidebar.rs` | `ordered_sidebar_sections` in the client honours `section_order`, `spaces.max_visible` and the tree view; `new_button` and `menu_position` move the footer controls; the automations section reads `ui.sidebar.automations.workspaces`. The multi-machine sidebar keeps upstream's plain ratio split (decision 7). |
| 22 | Session badge through handoff and overflow | `c73263bf` | carried (stage 2) | `orig/src/ui/tabs.rs` (`session_badge_rect`, `session_badge_text`) | `src/client/shell/tabs.rs` | `session_name` and `active_profile` were added to `ClientShellSnapshot` behind `#[serde(default)]`; the badge sits left of upstream's `tab_bar_right` status segments and is suppressed on the default session and profile (decision 11). |
| 23 | Mobile layout (glyphs, profiles, tabs in the header) | `7c9cb2da`, `49dfcf9f`, `301a6567`, `bacbdfa2` | needs-port | `orig/src/ui/mobile.rs`, `orig/src/app/input/mobile` paths | `src/client/shell/mobile.rs` (1127 lines, upstream's own mobile layer) | Upstream has its own mobile shell; port the fork's profile row and header tabs onto it rather than replacing it. |
| 24 | Navigator / ⌘K search palette + fuzzy scorer | pre-`d79fd746` plus `e838a372` refinements | needs-port | `src/app/fuzzy.rs` (fork-only, kept, `#![allow(dead_code)]`), `orig/src/app/actions.rs` (`open_navigator_from`, `navigator_rows_from`, `score_navigator_row`, `accept_navigator_selection_from`) | `src/client/shell/overlays.rs`, `src/client/shell/overlay_input.rs` | `Mode::Navigator`, `NavigatorState`, `NavigatorRow` and 19 navigator tests were dropped with the overlay. The scorer is untouched and ready. |
| 25 | Settings overlay, global menu, keybind help | pre-`d79fd746` | superseded-by-upstream | `orig/src/ui/keybind_help.rs`, `orig/src/ui/menus.rs`, `orig/src/app/input/settings.rs` | `src/client/shell/settings_overlay.rs`, `src/client/shell/global_menu.rs` | Upstream ships all three in the client shell. The fork's badge helpers (`global_menu_item_has_badge`, `settings_section_has_badge`) have upstream equivalents in `src/client/shell/global_menu.rs`. |
| 26 | Prefix ASCII input-source switching | pre-`d79fd746` | superseded-by-upstream | `orig/src/app/api.rs` (`sync_prefix_input_source`), `AppEvent::PrefixInputSource` | `src/platform/mod.rs` (`PrefixInputSource` trait), `src/client/mod.rs`, `src/client/shell_runtime.rs` | Upstream drives the host TIS switch from the client, which is where it belongs now. The fork's event variant was dropped. |
| 27 | Fork config keys | many | carried | `src/config/model.rs`, `src/config/sidebar.rs` | — | `agent_close_focus`, `attention_read`, `show_tab_status`, `status_indicators`, `hide_tab_bar_when_single_tab`, `tab_bar_position`, `mouse_back_button`, `mouse_forward_button`, `copy_on_select`, `mouse_capture`, `redraw_on_focus_gained`, `mouse_scroll_lines`, `sidebar.section_order`, `sidebar.new_button`, `sidebar.menu_position`, `sidebar.automations`, `sidebar.debug_bounds`, `accent`, `agent_panel_sort = triage\|tree` all still parse. Several are inert until their UI is ported. |
| 28 | Session snapshot UI preferences | many | carried (fork keys) / dropped (upstream's three) | `src/persist/snapshot.rs` (`UiPrefs`, `capture`), `src/app/state.rs` (`snapshot_ui_prefs`) | `src/client/shell/preferences.rs` (upstream's per-client `ClientChromePreferences`) | `automations_expanded`, `collapsed_agent_group_keys` and every `tree_*` key still round-trip through `session.json`. `sidebar_width`, `sidebar_section_split` and `collapsed_space_keys` are now written as `None`/empty: upstream's `persist::snapshot::tests::capture_contract_omits_legacy_server_chrome_state` freezes that, and 0.9 keeps chrome width per client in `state_dir()/client-shell/local-*.json`. Stage 2 should read those three from the client preferences file. |
| 29 | `pane move` | uncommitted WIP in the main checkout (`src/app/runtime_mutations.rs`) | superseded-by-upstream | not in this tree | `src/api/schema/panes.rs` (`PaneMoveParams`, `PaneMoveDestination`), `src/app/api/panes.rs` (`handle_pane_move`), `src/server/headless/tests/pane_move.rs` | Upstream 0.9.1 ships `pane move` (#4153) with its own API, destinations and tests. **The main checkout's `runtime_mutations.rs` pane-move WIP is superseded; do not port it.** That file no longer exists on `port-0.9` — upstream deleted it in the client-shell refactor. |
| 30 | Server-side sidebar/tab-bar geometry and host mouse capture | pre-`d79fd746` | dropped | `orig/src/app/state.rs` (`ViewState` hit areas, `DragState`, `WorkspacePressState`, `TabPressState`, `app_surface_pane_ids`, `should_capture_host_mouse_from`) | `src/client/shell/config.rs` (`layout`), `src/client/shell/mouse.rs` | The server no longer draws chrome, so hit rects and press tracking have no meaning there. `app_surface_pane_ids` and `is_prefix_key` are kept `#[allow(dead_code)]` for the port. |

## Tests moved out

Fork tests that exercised removed UI were deleted from the tree rather than
relaxed. Every one of them is preserved verbatim in its `orig/` file. Upstream
tests were never touched.

| File | Test | Reason |
|------|------|--------|
| `src/app/actions.rs` | `panel_next_close_target_ignores_non_agent_and_unfocused_agent` | `panel_next_agent_close_target` (feature 20) |
| `src/app/actions.rs` | `panel_next_close_target_none_when_tab_has_siblings` | same |
| `src/app/actions.rs` | `close_with_tab_siblings_stays_in_tab` | same |
| `src/app/actions.rs` | `closing_focused_agent_focuses_next_triage_entry` | same |
| `src/app/actions.rs` | `closing_last_triage_entry_wraps_to_first_remaining` | same |
| `src/app/actions.rs` | `stock_agent_close_focus_preserves_layout_focus` | same |
| `src/app/actions.rs` | `zero_visible_workspace_navigation_is_a_no_op` | `visible_workspace_order` (feature 21) |
| `src/app/actions.rs` | 19 navigator tests + 19 tree-cycle tests (conflict blocks) | features 24 and 10 |
| `src/app/actions.rs` | `switch_workspace_keeps_selected_visible_in_scrolled_sidebar` | sidebar scroll (feature 21) |
| `src/app/agent_view.rs` | `triage_sort_orders_tiers_and_oldest_state_change_first` | `AgentPanelSort::Triage` projection (feature 10) |
| `src/app/api/agents.rs` | `agent_focus_replacing_confirm_close_discards_pending_close_focus` | `Mode::ConfirmClose` (feature 25) |
| `src/app/api/tabs.rs` | `focused_tab_create_replacing_confirm_close_discards_pending_focus` | same |
| `src/app/api/panes.rs` | `pane_profiles_make_untagged_workspace_visible_in_personal_and_work` | used `crate::ui::agent_panel_entries` (feature 10) |
| `src/app/api/panes.rs` | `api_pane_move_focuses_copy_mode_pane_back_into_copy_mode`, `api_pane_zoom_focuses_copy_mode_pane_back_into_copy_mode`, `key_release_follows_pane_moved_across_workspaces` | copy mode moved to the client (feature 25) |
| `src/app/state.rs` | `invariants_reject_copy_and_chrome_press_for_same_input_source`, `invariants_reject_hover_for_missing_pane` | press/hover state (feature 30) |
| `src/app/state.rs` | 4 navigator display-line tests, 5 context-menu item tests | features 24 and 25 |
| `src/app/mod.rs` | the fork's client-input routing suite (1373 lines) | superseded by upstream's client shell |
| `src/pane.rs` | `first_agent_acquisition_keeps_osc_evidence_replacement_clears_it` | duplicate of upstream's (feature 18) |
| `tests/cross_area.rs` | fork frame assertions on the reattached client | upstream's `client_shell_handshake` replaces them |

`src/app/api/panes.rs: api_pane_zoom_explicit_background_pane_updates_focus_history`
was kept: it covers focus history (feature 8), which is carried.

## Decisions

Items 1-5 were taken in stage 1; 6 onwards in stage 2. These were forced by
upstream gates. Each one is reversible, but changing it
means changing an upstream test, so raise it with Alex first.

1. **`AppState.mode` is public again.** The fork's private mode holder behind
   `mode()` / `replace_mode()` existed to enforce copy-mode invariants that no
   longer live server-side; 0.9's `Mode` is just `Navigate | Terminal`. All 67
   call sites were converted to the field. This removes the recurring E0615
   merge class for good.
2. **`workspace.create`'s `profiles` field is `#[schemars(skip)]`.** Upstream
   freezes the advertised endpoint method shapes against
   `tests/fixtures/endpoint-method-shapes-v1.json`, and its failure message says
   to gate new fields explicitly. The field still serializes over the local
   socket API; it is simply not in the advertised v1 contract or the generated
   schema. Stage 2 decides whether to advertise it as a new method.
3. **The fork's Claude detection tweak was reverted to upstream.** The fork
   removed `visible_idle = true` from the `live_prompt_box` rule because
   bypassing the Working-to-Idle confirmation delay published false completions
   (~44 flips per minute). Upstream 0.9.1 restored it and gates on it. The
   comment and rationale are preserved in `src/detect/manifests/claude.toml`.
   Re-apply the tweak as `~/.config/herdr/agent-detection/claude.toml` or take
   it upstream. The fork's `background_shell_working` rule in the distributed
   manifest (`distribution/agent-detection/claude.toml`, renamed from
   `website/`) was also dropped for the same reason.
4. **Agent occupancy now ends on the second observation.** Upstream changed the
   release condition from "a process exit was observed" to "an exit was
   recorded and no agent is detected any more". Two fork tests
   (`agent_occupancy_identity_survives_unname_but_not_exit` and the
   `exit_agent_process` helper in `app::api::agents::tests`) were ported to
   drive both steps. Ownership still clears exactly when the agent leaves.
5. **`ui.agent_close_focus = "panel_next"` is inert.** It needs the sidebar
   panel order, which the client owns. The config key still parses.
   *(Superseded in stage 2: the client now queues the follow-up focus behind
   its own close, ledger #20.)*

6. **Tree chrome state moved to the per-client preferences file.** The fork
   kept `tree_show_*`, `tree_collapsed_*`, `tree_pinned_spaces`,
   `tree_show_hidden_spaces`, `hidden_spaces_expanded`, `automations_expanded`
   and `collapsed_agent_group_keys` on the server's `AppState`, because the
   server drew the sidebar. 0.9 draws it in the client, and the client only
   ever sees `ClientShellSnapshot`, whose shape upstream freezes. Rather than
   widen the wire contract for chrome state, these live beside upstream's own
   `collapsed_groups` in `state_dir()/client-shell/local-*.json`
   (`ClientTreeChromePreferences`), per endpoint, exactly as upstream keeps
   sidebar width and the worktree-group collapse set. The server-side fields
   still exist and still round-trip through `session.json`; they are simply no
   longer what the sidebar reads.

7. **The tree renders for the single-machine sidebar only.** With more than
   one machine connected, upstream's aggregated agent panel
   (`src/client/shell/endpoint_agents.rs`) owns the detail section and groups
   by machine. Layering a per-machine space/tab tree inside a cross-machine
   list would give two competing groupings in one panel, so the tree applies
   to the local-only sidebar. `prefix+w`, collapsed machine groups and the
   `machine` sidebar token are untouched.

8. **`ordered_agent_pane_ids` gained a real `Triage` arm, and ⌘E ranks
   separately from the space badge.** `status_priority` (used for the space
   roll-up) ranks `Working` above `Idle`; the fork's ⌘E and triage ordering
   ranks a read completion above a working agent. Both are wanted, so
   `tree::cycle_attention_rank` is a second function rather than a change to
   the first.


9. **`workspace.set_pinned` is a new advertised endpoint method.** Pinning is
   the one piece of tree chrome with server-side behaviour:
   `App::respawn_tab_for_pinned_workspace` grows a replacement tab so a pinned
   space survives its last close, and it reads `AppState.tree_pinned_spaces`.
   With the tree drawn in the client, nothing fed that set any more. Upstream's
   own failure message on
   `advertised_client_shell_method_shapes_stay_at_the_v1_contract` says to
   "add load-bearing behavior as a new advertised method", so the method is
   added and frozen separately in that test, exactly as upstream froze
   `pane.link.resolve`. No existing method changed shape. The generated schema
   artifact `docs/next/api/herdr-api.schema.json` was regenerated.

10. **The tree layer toggles live on the sort control's right-click.** The fork
    turned the agent-panel sort label into a menu carrying the sort plus the
    three layer toggles and the hidden-spaces reveal. Upstream's sort label is
    a left-click cycle, which is the mechanism to keep, so the fork's toggles
    moved to a right-click context menu on the same control
    (`ClientContextMenuTarget::SidebarView`). Left-click still cycles
    grouped/priority/triage/tree.

11. **No badge for a default session on the default profile.** The fork always
    drew the session badge, so a stock setup read `default` and lost 8 columns
    of tab strip. Two upstream tab-bar tests
    (`focused_last_overflow_tab_shows_its_full_label` and
    `tab_bar_renders_endpoint_status_ellipses_and_clamps_to_useful_scroll`)
    measure that strip. Rather than change them, the badge is suppressed when
    it would only say `default`: a named session or a non-default profile still
    shows, which is every case the badge was for.

12. **The session badge rides in `ClientShellSnapshot`, not the preferences
    file.** Unlike the tree chrome, the session name and active profile are
    facts about the endpoint, not per-client chrome. Both fields are
    `#[serde(default)]`, so `tests/fixtures/endpoint-snapshot-v1.json` still
    decodes and a pre-0.9.1 endpoint simply yields no badge.

13. **Ownership edges reach the client through the snapshot, resolved.** The
    client cannot resolve an `AgentOwnerRef` (it has no terminal registry), so
    the endpoint resolves the current owner to a public pane id and publishes
    that plus an `orphaned` flag on `ClientShellAgent`, and
    `orchestrator_mode`/`tab_count` on `ClientShellWorkspace`. All four are
    `#[serde(default)]`, so the frozen v1 snapshot fixture still decodes. The
    collapse keys are the owner's pane id, or `orch:<workspace-id>` for an
    orchestrator group; the fork used the durable agent identity, which is not
    on the wire. A pane id is stable for the life of the group, which is all
    the collapse set needs within a boot, but it does mean a collapsed group
    reopens after a pane move. Publishing `agent_identity` would fix that and
    is the next step if it matters.

## Build environment

0.9.1 requires **Zig 0.16.0**; this machine had 0.15.2. A standalone toolchain
was unpacked at `~/.cache/herdr-build/toolchains/zig-aarch64-macos-0.16.0/`;
pass it as `ZIG=` on every cargo command. Nothing system-wide was changed.

Zig 0.16 stores fetched packages as `~/.cache/zig/p/<hash>.tar.gz` files, while
0.15 stored `~/.cache/zig/p/<hash>/` directories. `scripts/prefetch_zig_dependencies.py`
checks `is_dir()`, so against a 0.15-populated cache it never converges and
loops forever. Zig's own HTTP client still gets `400` from
`deps.files.ghostty.org` where curl gets `200`, so the dependencies have to be
curled and handed to `zig fetch` as local files — and `zig fetch` in 0.16 must
run from a directory containing a `build.zig`.

## Config compatibility with Alex's live config

`herdr config check` against a copy of `~/.config/herdr/config.toml` under an
isolated `XDG_CONFIG_HOME` reports `config: ok` on the ported 0.9.1 binary —
no fork-only key warns and no key is ignored. The checker was verified to
catch unknown keys by probing it with one.

Only two keys moved between the fork's 0.8.2 default config and 0.9.1's:

| 0.8.2 key | 0.9.1 key | Effect on Alex's config |
|-----------|-----------|-------------------------|
| `experimental.kitty_graphics` | `terminal.kitty_graphics` | He sets the old one. It is a documented deprecated compatibility key (`src/config/model.rs`) and still applies. |
| `ui.sidebar.spaces.accent` | `ui.sidebar.accent` | He does not set either. |

The fork keys the port added back to the website config reference —
`ui.mouse_back_button`, `ui.mouse_forward_button`, `ui.sidebar.debug_bounds`
and `agent_panel_sort = "tree"` — all parse; several are inert until their UI
is ported (see the ledger).

