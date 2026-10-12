use twin_app::WhyResult;

use crate::output::format::{Lines, Status};

pub fn render(result: &WhyResult) -> String {
    let mut out = Lines::new();
    out.title("twin why");
    out.status_row(Status::Neutral, "target", &result.target);
    if !result.proof_chains.is_empty() {
        out.blank();
        out.section("supporting evidence chains · up to 4 links");
        out.tree_leaf(false, "interpretation", "stored links traversed in either direction; each link is assessed below; associations do not establish cause");
        for chain in &result.proof_chains {
            out.tree_leaf(
                false,
                &chain.class,
                &chain
                    .nodes
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" ↔ "),
            );
            for (index, id) in chain.relationship_ids.iter().enumerate() {
                if let Some(edge) = result.relationships.iter().find(|edge| &edge.id == id) {
                    out.tree_leaf(
                        index + 1 == chain.relationship_ids.len(),
                        &edge.class,
                        &format!(
                            "{} {} {} · {} ({}/100)",
                            edge.from,
                            edge.kind,
                            edge.to,
                            edge.evidence_strength,
                            edge.evidence_score
                        ),
                    );
                }
            }
        }
    }
    for relationship in &result.relationships {
        out.blank();
        out.section(&format!(
            "{} {} {} ({})",
            relationship.from, relationship.kind, relationship.to, relationship.class
        ));
        out.tree_leaf(
            false,
            "evidence",
            &format!(
                "{} ({}/100)",
                relationship.evidence_strength, relationship.evidence_score
            ),
        );
        for observation in &relationship.observations {
            let seen = if observation.repeat_count > 1 {
                format!(
                    "{} ×{} from {} to {} — latest {}",
                    observation.kind,
                    observation.repeat_count,
                    observation.first_timestamp_label(),
                    observation.timestamp_label(),
                    observation.id
                )
            } else {
                format!(
                    "{} at {} — {}",
                    observation.kind,
                    observation.timestamp_label(),
                    observation.id
                )
            };
            out.tree_leaf(false, &observation.source, &seen);
            if let Some(raw_ref) = &observation.raw_ref {
                out.tree_leaf(false, "source path", raw_ref);
            }
            for fact in &observation.facts {
                out.tree_leaf(false, "observed fact", fact);
            }
        }
        for unknown in &relationship.unknowns {
            out.tree_leaf(false, "unknown", unknown);
        }
        out.tree_leaf(true, "relationship", &relationship.id);
    }
    for unknown in &result.unknowns {
        out.tree_leaf(true, "unknown", unknown);
    }
    out.into_string()
}
