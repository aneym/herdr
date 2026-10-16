use super::state::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PinSectionError {
    InvalidName,
    Exists,
    NotFound,
    NotPinned,
    AgentPin,
}

impl PinSectionError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::InvalidName => "pin_section_invalid_name",
            Self::Exists => "pin_section_exists",
            Self::NotFound => "pin_section_not_found",
            Self::NotPinned => "tab_not_pinned",
            Self::AgentPin => "pin_section_agent_pin",
        }
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::InvalidName => "section name must be 1 to 64 characters",
            Self::Exists => "a pinned section with that name already exists",
            Self::NotFound => "no pinned section has that name",
            Self::NotPinned => "the tab is not pinned",
            Self::AgentPin => "agent pins stay in the Agents list and take no section",
        }
    }
}

fn clean_name(name: &str) -> Result<String, PinSectionError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(PinSectionError::InvalidName);
    }
    Ok(name.to_owned())
}

impl AppState {
    pub(crate) fn pin_section_rank(&self, section: Option<&str>) -> usize {
        section
            .and_then(|name| self.pin_sections.iter().position(|s| s == name))
            .map_or(0, |index| index + 1)
    }

    pub(crate) fn normalize_pin_sections(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.pin_sections
            .retain(|name| !name.trim().is_empty() && seen.insert(name.clone()));
        let sections = self.pin_sections.clone();
        for pin in &mut self.pinned_tabs {
            if pin.role.is_some()
                || pin
                    .section
                    .as_ref()
                    .is_some_and(|name| !sections.contains(name))
            {
                pin.section = None;
            }
        }
        let agents = self
            .pinned_tabs
            .iter()
            .take_while(|pin| pin.role.is_some())
            .count();
        let ranks: Vec<usize> = self.pinned_tabs[agents..]
            .iter()
            .map(|pin| self.pin_section_rank(pin.section.as_deref()))
            .collect();
        let mut block: Vec<(usize, super::state::PinnedTab)> = ranks
            .into_iter()
            .zip(self.pinned_tabs.drain(agents..))
            .collect();
        block.sort_by_key(|(rank, _)| *rank);
        self.pinned_tabs
            .extend(block.into_iter().map(|(_, pin)| pin));
    }

    pub(crate) fn create_pin_section(
        &mut self,
        name: &str,
        after: Option<&str>,
    ) -> Result<String, PinSectionError> {
        let name = clean_name(name)?;
        if self.pin_sections.contains(&name) {
            return Err(PinSectionError::Exists);
        }
        let index = match after {
            Some(after) => {
                self.pin_sections
                    .iter()
                    .position(|s| s == after.trim())
                    .ok_or(PinSectionError::NotFound)?
                    + 1
            }
            None => self.pin_sections.len(),
        };
        self.pin_sections.insert(index, name.clone());
        self.normalize_pin_sections();
        Ok(name)
    }

    pub(crate) fn rename_pin_section(
        &mut self,
        name: &str,
        new_name: &str,
    ) -> Result<String, PinSectionError> {
        let new_name = clean_name(new_name)?;
        let index = self
            .pin_sections
            .iter()
            .position(|s| s == name.trim())
            .ok_or(PinSectionError::NotFound)?;
        let old = self.pin_sections[index].clone();
        if old == new_name {
            return Ok(new_name);
        }
        if self.pin_sections.contains(&new_name) {
            return Err(PinSectionError::Exists);
        }
        self.pin_sections[index] = new_name.clone();
        for pin in &mut self.pinned_tabs {
            if pin.section.as_deref() == Some(old.as_str()) {
                pin.section = Some(new_name.clone());
            }
        }
        Ok(new_name)
    }

    pub(crate) fn delete_pin_section(&mut self, name: &str) -> Result<(), PinSectionError> {
        let index = self
            .pin_sections
            .iter()
            .position(|s| s == name.trim())
            .ok_or(PinSectionError::NotFound)?;
        let old = self.pin_sections.remove(index);
        let agents = self
            .pinned_tabs
            .iter()
            .take_while(|pin| pin.role.is_some())
            .count();
        let mut freed = Vec::new();
        let mut rest = Vec::new();
        for pin in self.pinned_tabs.drain(agents..) {
            if pin.section.as_deref() == Some(old.as_str()) {
                freed.push(super::state::PinnedTab {
                    section: None,
                    ..pin
                });
            } else {
                rest.push(pin);
            }
        }
        let unsectioned = rest.iter().take_while(|pin| pin.section.is_none()).count();
        self.pinned_tabs.extend(rest.drain(..unsectioned));
        self.pinned_tabs.extend(freed);
        self.pinned_tabs.extend(rest);
        self.normalize_pin_sections();
        Ok(())
    }

    pub(crate) fn move_pin_section(
        &mut self,
        name: &str,
        index: usize,
    ) -> Result<usize, PinSectionError> {
        let from = self
            .pin_sections
            .iter()
            .position(|s| s == name.trim())
            .ok_or(PinSectionError::NotFound)?;
        let section = self.pin_sections.remove(from);
        let index = index.min(self.pin_sections.len());
        self.pin_sections.insert(index, section);
        self.normalize_pin_sections();
        Ok(index)
    }

    pub(crate) fn set_pin_section(
        &mut self,
        tab_id: &str,
        section: Option<&str>,
    ) -> Result<(), PinSectionError> {
        let section = match section.map(str::trim) {
            Some(name) => Some(
                self.pin_sections
                    .iter()
                    .find(|s| s.as_str() == name)
                    .cloned()
                    .ok_or(PinSectionError::NotFound)?,
            ),
            None => None,
        };
        let from = self
            .pinned_tab_index(tab_id)
            .ok_or(PinSectionError::NotPinned)?;
        if self.pinned_tabs[from].role.is_some() {
            return Err(PinSectionError::AgentPin);
        }
        if self.pinned_tabs[from].section == section {
            return Ok(());
        }
        let mut pin = self.pinned_tabs.remove(from);
        pin.section = section;
        let rank = self.pin_section_rank(pin.section.as_deref());
        let agents = self
            .pinned_tabs
            .iter()
            .take_while(|pin| pin.role.is_some())
            .count();
        let end = (agents..self.pinned_tabs.len())
            .filter(|&i| self.pin_section_rank(self.pinned_tabs[i].section.as_deref()) <= rank)
            .map(|i| i + 1)
            .max()
            .unwrap_or(agents);
        self.pinned_tabs.insert(end, pin);
        self.normalize_pin_sections();
        Ok(())
    }

    pub(crate) fn pin_section_members(&self, name: &str) -> Vec<String> {
        self.pinned_tabs
            .iter()
            .filter(|pin| pin.section.as_deref() == Some(name))
            .map(|pin| pin.tab_id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::PinnedTab;

    fn pin(id: &str) -> PinnedTab {
        PinnedTab {
            tab_id: id.into(),
            priority: 0,
            role: None,
            hidden: false,
            section: None,
        }
    }

    fn order(state: &AppState) -> Vec<(String, Option<String>)> {
        state
            .pinned_tabs
            .iter()
            .map(|pin| (pin.tab_id.clone(), pin.section.clone()))
            .collect()
    }

    fn ids(state: &AppState) -> Vec<&str> {
        state
            .pinned_tabs
            .iter()
            .map(|p| p.tab_id.as_str())
            .collect()
    }

    #[test]
    fn pin_sections_group_pins_after_unsectioned_and_keep_order_through_edits() {
        let mut state = AppState::test_new();
        state.pinned_tabs = ["a", "b", "c", "d"].into_iter().map(pin).collect();
        state.pinned_tabs.insert(
            0,
            PinnedTab {
                role: Some(crate::api::schema::TabRole::Agent),
                ..pin("agent")
            },
        );
        state.create_pin_section("Later", None).unwrap();
        state.create_pin_section("Needs you", None).unwrap();
        assert_eq!(state.move_pin_section("Needs you", 0), Ok(0));
        state
            .create_pin_section("Northstar: building", Some("Needs you"))
            .unwrap();
        assert_eq!(
            state.pin_sections,
            ["Needs you", "Northstar: building", "Later"]
        );
        assert_eq!(
            state.create_pin_section(" Later ", None),
            Err(PinSectionError::Exists)
        );
        assert_eq!(
            state.create_pin_section("  ", None),
            Err(PinSectionError::InvalidName)
        );

        state.set_pin_section("a", Some("Later")).unwrap();
        state.set_pin_section("b", Some("Needs you")).unwrap();
        state.set_pin_section("d", Some("Needs you")).unwrap();
        assert_eq!(ids(&state), ["agent", "c", "b", "d", "a"]);
        assert_eq!(
            state.set_pin_section("agent", Some("Later")),
            Err(PinSectionError::AgentPin)
        );
        assert_eq!(
            state.set_pin_section("zzz", Some("Later")),
            Err(PinSectionError::NotPinned)
        );
        assert_eq!(
            state.set_pin_section("c", Some("Nope")),
            Err(PinSectionError::NotFound)
        );

        assert_eq!(state.pin_role_range("d"), Some(2..=3));
        assert!(state.move_pinned_tab("d", 0));
        assert_eq!(ids(&state), ["agent", "c", "d", "b", "a"]);

        state.pin_tab("e".into(), 0);
        assert_eq!(ids(&state), ["agent", "c", "e", "d", "b", "a"]);
        state.pin_tab("d".into(), 5);
        assert_eq!(state.pinned_tabs[3].section.as_deref(), Some("Needs you"));

        state.rename_pin_section("Needs you", "Needs Alex").unwrap();
        assert_eq!(state.pin_section_members("Needs Alex"), ["d", "b"]);

        state.delete_pin_section("Needs Alex").unwrap();
        assert_eq!(
            order(&state),
            [
                ("agent".into(), None),
                ("c".into(), None),
                ("e".into(), None),
                ("d".into(), None),
                ("b".into(), None),
                ("a".into(), Some("Later".into())),
            ]
        );
        assert_eq!(state.pin_sections, ["Northstar: building", "Later"]);
    }

    #[test]
    fn normalize_drops_unknown_sections_and_agent_sections() {
        let mut state = AppState::test_new();
        state.pin_sections = vec!["Later".into(), "Later".into(), " ".into()];
        state.pinned_tabs = vec![
            PinnedTab {
                section: Some("Later".into()),
                ..pin("a")
            },
            PinnedTab {
                section: Some("Gone".into()),
                ..pin("b")
            },
        ];
        state.normalize_pin_sections();
        assert_eq!(state.pin_sections, ["Later"]);
        assert_eq!(
            order(&state),
            [("b".into(), None), ("a".into(), Some("Later".into()))]
        );
    }
}
