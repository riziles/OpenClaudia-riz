//! Final-answer validation for grounded agent turns.

use crate::evidence::{authoritative_evidence, Denial};
use crate::ledger::{ObsId, ObservationKind, RealityLedger};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalGateReport {
    pub evidence: Vec<ObsId>,
    pub verification: Vec<ObsId>,
}

/// Validate that a final answer cites reality and verification.
///
/// A final can report failed verification, but it cannot omit verification.
/// If it mentions tests, it must also cite a concrete command observation.
pub fn validate_final_answer(
    summary: &str,
    evidence: &[ObsId],
    verification: &[ObsId],
    ledger: &RealityLedger,
) -> Result<FinalGateReport, Denial> {
    if summary.trim().is_empty() {
        return Err(Denial::new("final answer requires a non-empty summary"));
    }

    let hydrated_evidence =
        authoritative_evidence(evidence, ledger, "final answer requires evidence")?;

    if verification.is_empty() {
        return Err(Denial::new(
            "final answer requires verification observation",
        ));
    }

    let hydrated_verification = authoritative_evidence(
        verification,
        ledger,
        "final answer requires verification observation",
    )?;
    if !hydrated_verification
        .iter()
        .all(|obs| matches!(obs.kind, ObservationKind::Verification { .. }))
    {
        return Err(Denial::new(
            "final verification ids must reference verification observations",
        ));
    }

    if summary_mentions_tests(summary)
        && !hydrated_evidence
            .iter()
            .any(|obs| matches!(obs.kind, ObservationKind::CommandRun { .. }))
    {
        return Err(Denial::new(
            "final test claims require a command observation",
        ));
    }

    Ok(FinalGateReport {
        evidence: evidence.to_vec(),
        verification: verification.to_vec(),
    })
}

pub fn validate_cited_final_answer(
    summary: &str,
    ledger: &RealityLedger,
) -> Result<FinalGateReport, Denial> {
    let evidence = extract_cited_obs_ids(summary);
    let verification = evidence
        .iter()
        .copied()
        .filter(|id| {
            ledger
                .get(*id)
                .is_some_and(|obs| matches!(obs.kind, ObservationKind::Verification { .. }))
        })
        .collect::<Vec<_>>();
    validate_final_answer(summary, &evidence, &verification, ledger)
}

#[must_use]
pub fn extract_cited_obs_ids(text: &str) -> Vec<ObsId> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    const UUID_LEN: usize = 36;
    for (start, _) in text.char_indices() {
        let end = start + UUID_LEN;
        if end > text.len() || !text.is_char_boundary(end) {
            continue;
        }
        let Ok(id) = text[start..end].parse::<ObsId>() else {
            continue;
        };
        if seen.insert(id) {
            ids.push(id);
        }
    }
    ids
}

fn summary_mentions_tests(summary: &str) -> bool {
    let lower = summary.to_ascii_lowercase();
    lower.contains("test") || lower.contains("cargo check") || lower.contains("verified")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{Authority, ObservationKind, RealityLedger};

    #[test]
    fn extract_cited_obs_ids_finds_unique_uuid_tokens() {
        let first = ObsId::new();
        let second = ObsId::new();
        let text = format!("Used [{first}] and obs-{second}; repeated {first}.");

        assert_eq!(extract_cited_obs_ids(&text), vec![first, second]);
    }

    #[test]
    fn cited_final_requires_cited_verification_observation() {
        let mut ledger = RealityLedger::new();
        let command = ledger
            .observe_command_run("/tmp", vec!["cargo".into(), "check".into()], 0, "", "")
            .expect("command");
        let verification = ledger
            .append(
                Authority::Verifier,
                ObservationKind::Verification {
                    passed: true,
                    command: Some("cargo check".to_string()),
                    findings: Vec::new(),
                },
            )
            .expect("verification");
        let summary = format!("Verified with cargo check using [{command}] and [{verification}].");

        let report = validate_cited_final_answer(&summary, &ledger).expect("valid final");
        assert_eq!(report.evidence, vec![command, verification]);
        assert_eq!(report.verification, vec![verification]);
    }

    #[test]
    fn cited_final_rejects_uncited_summary() {
        let ledger = RealityLedger::new();
        let denial =
            validate_cited_final_answer("Verified with cargo check.", &ledger).expect_err("denied");
        assert_eq!(denial.reason(), "final answer requires evidence");
    }
}
