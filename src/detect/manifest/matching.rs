use std::collections::HashMap;

use super::{CompiledGate, CompiledRule, ManifestRule, RuleEvidence};

#[derive(Default)]
pub(super) struct RegionMatcher {
    lowered: HashMap<(usize, usize), String>,
}

impl RegionMatcher {
    pub(super) fn matches(&mut self, rule: &CompiledRule, text: &str) -> bool {
        let lower_text = self
            .lowered
            .entry((text.as_ptr() as usize, text.len()))
            .or_insert_with(|| text.to_lowercase());
        compiled_gate_matches(&rule.gate, text, lower_text)
    }
}

pub(super) fn rule_evidence(rule: &ManifestRule, region_text: &str) -> RuleEvidence {
    RuleEvidence {
        contains: rule.contains.clone(),
        regex: rule.regex.clone(),
        line_regex: rule.line_regex.clone(),
        all_count: rule.all.len(),
        any_count: rule.any.len(),
        not_count: rule.not_gate.len(),
        region_bytes: region_text.len(),
        region_preview: bounded_preview(region_text),
    }
}

fn bounded_preview(text: &str) -> String {
    const MAX_CHARS: usize = 240;
    let mut preview: String = text.chars().take(MAX_CHARS).collect();
    if text.chars().count() > MAX_CHARS {
        preview.push_str("...");
    }
    preview
}

fn compiled_gate_matches(gate: &CompiledGate, text: &str, lower_text: &str) -> bool {
    if !gate
        .contains
        .iter()
        .all(|needle| lower_text.contains(needle))
    {
        return false;
    }

    if !gate.regex.iter().all(|regex| regex.is_match(text)) {
        return false;
    }

    if !gate
        .line_regex
        .iter()
        .all(|regex| text.lines().any(|line| regex.is_match(line)))
    {
        return false;
    }

    if !gate
        .all
        .iter()
        .all(|nested| compiled_gate_matches(nested, text, lower_text))
    {
        return false;
    }

    if !gate.any.is_empty()
        && !gate
            .any
            .iter()
            .any(|nested| compiled_gate_matches(nested, text, lower_text))
    {
        return false;
    }

    if gate
        .not_gate
        .iter()
        .any(|nested| compiled_gate_matches(nested, text, lower_text))
    {
        return false;
    }

    true
}
