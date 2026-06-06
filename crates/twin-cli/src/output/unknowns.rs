use std::collections::BTreeMap;
use std::str::FromStr;

use twin_app::ImpactUnknown;
use twin_core::UnknownKind;

use super::format::Lines;

#[derive(Debug, Clone)]
pub struct UnknownGroup {
    pub kind: String,
    pub human_label: String,
    pub entries: usize,
    pub weakens_evidence: bool,
    pub sample_details: Vec<String>,
}

pub fn humanize_unknown_kind(kind: &str) -> String {
    UnknownKind::from_str(kind)
        .map(|k| k.human_label().to_string())
        .unwrap_or_else(|_| kind.replace('_', " "))
}

pub fn group_unknowns(unknowns: &[ImpactUnknown]) -> Vec<UnknownGroup> {
    group_unknown_refs(unknowns.iter())
}

pub fn group_unknown_refs<'a, I>(unknowns: I) -> Vec<UnknownGroup>
where
    I: IntoIterator<Item = &'a ImpactUnknown>,
{
    let mut map: BTreeMap<String, UnknownGroup> = BTreeMap::new();
    for unknown in unknowns {
        let group = map
            .entry(unknown.kind.clone())
            .or_insert_with(|| UnknownGroup {
                kind: unknown.kind.clone(),
                human_label: humanize_unknown_kind(&unknown.kind),
                entries: 0,
                weakens_evidence: unknown.weakens_evidence,
                sample_details: Vec::new(),
            });
        group.entries += 1;
        group.weakens_evidence |= unknown.weakens_evidence;
        if group.sample_details.len() < 3 && !group.sample_details.contains(&unknown.detail) {
            group.sample_details.push(unknown.detail.clone());
        }
    }
    map.into_values().collect()
}

pub fn render_unknown_groups(out: &mut Lines, groups: &[UnknownGroup]) {
    let total = groups.len();
    for (i, group) in groups.iter().enumerate() {
        let is_last_group = i + 1 == total;
        let summary = if group.entries == 1 {
            group
                .sample_details
                .first()
                .cloned()
                .unwrap_or_else(|| format!("1 {}", group.human_label))
        } else {
            format!("{} report(s)", group.entries)
        };
        let has_samples = group.entries > 1 && !group.sample_details.is_empty();
        let parent_is_last = is_last_group && !has_samples;
        out.tree_branch("", parent_is_last, &group.human_label, &summary);
        if has_samples {
            let indent = Lines::child_indent("", is_last_group);
            let sample_total = group.sample_details.len();
            for (j, detail) in group.sample_details.iter().enumerate() {
                let is_last_sample = j + 1 == sample_total;
                out.tree_branch(&indent, is_last_sample, "example", detail);
            }
        }
    }
}

pub fn split_scan_health(unknowns: &[ImpactUnknown]) -> (Vec<&ImpactUnknown>, Vec<&ImpactUnknown>) {
    let scan_health = UnknownKind::ScanHealth.as_str();
    let mut health = Vec::new();
    let mut rest = Vec::new();
    for unknown in unknowns {
        if unknown.kind == scan_health {
            health.push(unknown);
        } else {
            rest.push(unknown);
        }
    }
    (health, rest)
}
