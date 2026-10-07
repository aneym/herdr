//! Placement facts are captured by the row builder and confirmed by the renderer.
use std::{cell::RefCell, path::{Path, PathBuf}, time::{Duration, Instant}};
use serde::Serialize;
use super::tree::{AgentPanelListEntry as Entry, ClientTreeChrome};
use crate::factory_overlay::{TabKind, TabSection};

#[derive(Serialize, PartialEq, Eq)]
pub(super) struct WorkspaceReport {
    id: String,
    space_collapsed: bool,
    goal_filter: Option<String>,
    sections: Vec<SectionReport>,
    pub(super) tabs: Vec<TabReport>,
}
#[derive(Serialize, PartialEq, Eq)]
struct SectionReport { label: String, collapsed: bool }
#[derive(Serialize, PartialEq, Eq)]
pub(super) struct TabReport {
    pub(super) tab: String,
    kind: TabKind,
    pub(super) section: Option<String>,
    shown: bool,
    row: Option<u16>,
    indent: Option<u8>,
    under: Option<String>,
    hidden: Option<String>,
}
#[derive(Default)]
struct Reports {
    pending: Option<Vec<WorkspaceReport>>,
    recording: bool,
    last: std::collections::HashMap<PathBuf, (Instant, Vec<WorkspaceReport>)>,
    logged_error: bool,
}
thread_local! { static REPORTS: RefCell<Reports> = RefCell::new(Reports::default()); }

pub(super) fn section_label(section: Option<TabSection>) -> Option<&'static str> {
    Some(match section? {
        TabSection::Orchestrator => "ORCHESTRATOR",
        TabSection::Scoping => "SCOPING",
        TabSection::Implementing => "IMPLEMENTING",
        TabSection::Reviewing => "READY FOR REVIEW",
        TabSection::Monitoring => "MONITORING",
        TabSection::Closed => "closed",
    })
}
impl WorkspaceReport {
    pub(super) fn new(id: &str, tree: &ClientTreeChrome, priority: &super::tree::PriorityIndex, filter: Option<&str>) -> Self {
        Self { id: id.to_owned(), space_collapsed: tree.space_collapsed(priority, id),
            goal_filter: filter.map(str::to_owned), sections: Vec::new(), tabs: Vec::new() }
    }
    pub(super) fn add(&mut self, tab: &str, kind: TabKind, section: Option<String>, under: Option<String>, hidden: &str) {
        if self.tabs.iter().any(|entry| entry.tab == tab) { return; }
        self.tabs.push(TabReport { tab: tab.to_owned(), kind, section, under, shown: false,
            row: None, indent: None, hidden: Some(hidden.to_owned()) });
    }
    pub(super) fn placements(&mut self, entries: &[Entry]) {
        let mut section = None;
        let mut parents: Vec<(u8, String)> = Vec::new();
        for entry in entries {
            match entry {
                Entry::FactorySection { label, .. } => {
                    section = Some((*label).to_owned());
                    parents.clear();
                    self.sections.push(SectionReport { label: (*label).to_owned(), collapsed: false });
                }
                Entry::FactoryBackground { kind, collapsed, .. } => {
                    section = Some(kind.label().to_owned());
                    parents.clear();
                    self.sections.push(SectionReport { label: kind.label().to_owned(), collapsed: *collapsed });
                }
                Entry::FactoryTab(row) if row.header.tab_id.as_ref() == Some(&row.header.key) => {
                    parents.retain(|(indent, _)| *indent < row.header.indent);
                    self.add(&row.header.key, if row.workflow { TabKind::Workflow }
                        else if section.as_deref() == Some("ORCHESTRATOR") && parents.is_empty() { TabKind::Orchestrator }
                        else { TabKind::Lane }, section.clone(), parents.last().map(|(_, id)| id.clone()), "offscreen");
                    if let Some(tab) = self.tabs.last_mut() { tab.indent = Some(row.header.indent); }
                    parents.push((row.header.indent, row.header.key.clone()));
                }
                _ => {}
            }
        }
    }
    pub(super) fn inherit_sections(&mut self) {
        for _ in 0..self.tabs.len() {
            let sections = self.tabs.iter().map(|tab| (tab.tab.clone(), tab.section.clone()))
                .collect::<std::collections::HashMap<_, _>>();
            for tab in &mut self.tabs {
                if let Some(Some(section)) = tab.under.as_ref().and_then(|id| sections.get(id)) {
                    tab.section = Some(section.clone());
                }
            }
        }
    }
    pub(super) fn kinds(&mut self, overlay: &crate::factory_overlay::FactoryOverlay) {
        for tab in &mut self.tabs {
            if let Some(tag) = overlay.tab(&tab.tab) { tab.kind = tag.kind; }
        }
        self.tabs.retain(|tab| overlay.tab(&tab.tab).is_some());
    }
    pub(super) fn visibility(&mut self, entries: &[Entry], tree: &ClientTreeChrome) {
        for section in &mut self.sections {
            if tree.factory_sections_collapsed.contains(&format!("{}:{}", self.id, section.label))
                && !tree.factory_section_focus.contains_key(&self.id) { section.collapsed = true; }
        }
        for tab in &mut self.tabs {
            if tab.hidden.as_deref() == Some("goal_filter") && !self.space_collapsed { continue; }
            if self.space_collapsed { tab.hidden = Some("space_collapsed".into()); continue; }
            let visible = entries.iter().any(|entry| matches!(entry, Entry::FactoryTab(row) if row.header.key == tab.tab));
            if visible { continue; }
            if tree.factory_section_focus.get(&self.id).is_some_and(|focus|
                tab.section.as_deref() != Some("ORCHESTRATOR") && tab.section.as_ref() != Some(focus))
                || tab.section.as_ref().is_some_and(|section| tree.factory_sections_collapsed
                    .contains(&format!("{}:{section}", self.id))) && !tree.factory_section_focus.contains_key(&self.id) {
                tab.hidden = Some("section_collapsed".into());
            }
        }
    }
}
pub(super) fn recording() -> bool { REPORTS.with(|reports| reports.borrow().recording) }
pub(super) fn begin() {
    REPORTS.with(|reports| {
        let mut reports = reports.borrow_mut();
        reports.pending = Some(Vec::new());
        reports.recording = true;
    });
}
pub(super) fn end() { REPORTS.with(|reports| reports.borrow_mut().recording = false); }
/// Run `build` without adding its tree to the report: for a side computation
/// that rebuilds part of the tree but draws none of it.
pub(super) fn paused<T>(build: impl FnOnce() -> T) -> T {
    let was = REPORTS.with(|reports| std::mem::replace(&mut reports.borrow_mut().recording, false));
    let out = build();
    REPORTS.with(|reports| reports.borrow_mut().recording = was);
    out
}
pub(super) fn record(report: WorkspaceReport) {
    REPORTS.with(|reports| {
        if let Some(pending) = &mut reports.borrow_mut().pending { pending.push(report); }
    });
}
pub(super) fn drawn(entry: &Entry, y: u16) {
    let Entry::FactoryTab(row) = entry else { return };
    REPORTS.with(|reports| {
        if let Some(pending) = &mut reports.borrow_mut().pending {
            for workspace in pending {
                if workspace.space_collapsed { continue; }
                if let Some(tab) = workspace.tabs.iter_mut().find(|tab| tab.tab == row.header.key) {
                    tab.shown = true; tab.row = Some(y); tab.indent = Some(row.header.indent); tab.hidden = None;
                }
            }
        }
    });
}
pub(super) fn finish(preferences: Option<&Path>) {
    REPORTS.with(|reports| {
        let mut reports = reports.borrow_mut();
        let Some(workspaces) = reports.pending.take() else { return };
        let Some(path) = preferences.and_then(super::preferences::sidebar_path) else { return };
        let now = Instant::now();
        if let Some((last, previous)) = reports.last.get(&path) {
            let elapsed = now.duration_since(*last);
            if elapsed < Duration::from_secs(1) || (previous == &workspaces && elapsed < Duration::from_secs(60)) { return; }
        }
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
        let result = (|| -> std::io::Result<()> {
            let parent = path.parent().ok_or_else(|| std::io::Error::other("invalid sidebar report path"))?;
            std::fs::create_dir_all(parent)?;
            let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
            let payload = serde_json::json!({"version": 1, "at": at, "pid": std::process::id(), "workspaces": &workspaces});
            std::fs::write(&tmp, serde_json::to_vec(&payload)?)?;
            std::fs::rename(tmp, &path)
        })();
        // Throttle failed writes too, so an unwritable directory cannot busy-loop.
        reports.last.insert(path, (now, workspaces));
        if let Err(error) = result {
            if !reports.logged_error { eprintln!("sidebar report: {error}"); reports.logged_error = true; }
        }
    });
}
