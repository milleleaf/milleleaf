//! Which signatures a project requires (`.ox/policy.json`; a built-in default when absent) and
//! until when each algorithm is trusted (plan decision 5, 18 Oct 2026). `ox verify --require`
//! applies it. Dates are `YYYY-MM-DD` and compare as text with an RFC 3339 "now".

use std::collections::BTreeMap;
use std::fs;

use serde::{Deserialize, Serialize};

use crate::keys::SigInfo;
use crate::pq::{ED25519, MLDSA65, SLH};
use crate::{Result, Store, StoreError, io};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub signatures: Signatures,
    /// The algorithms a signature may use; one missing here is refused. `until`: refused from then.
    pub algorithms: BTreeMap<String, Algorithm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamps: Option<Timestamps>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Signatures {
    /// Every signer needs a valid signature in each of these.
    pub require: Vec<String>,
    /// Until this date a signer with Ed25519 alone is accepted, with a warning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classical_only_until: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Algorithm {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timestamps {
    /// RFC 3161 time-stamp authority used by `ox timestamp` when no `--tsa` is given.
    pub tsa: Option<String>,
}

impl Default for Policy {
    fn default() -> Policy {
        let until = Some("2035-01-01".to_string());
        Policy {
            signatures: Signatures {
                require: vec![ED25519.into(), MLDSA65.into()],
                classical_only_until: until.clone(),
            },
            algorithms: [(ED25519, until), (MLDSA65, None), (SLH, None), ("xwing", None)]
                .into_iter()
                .map(|(a, until)| (a.to_string(), Algorithm { until }))
                .collect(),
            timestamps: None,
        }
    }
}

/// A layer's signatures judged by a policy.
#[derive(Debug, Default)]
pub struct Verdict {
    pub refused: Vec<String>,
    pub warnings: Vec<String>,
}

fn past(now: &str, date: &Option<String>) -> bool {
    date.as_deref().is_some_and(|d| now.get(..10).unwrap_or(now) >= d)
}

impl Policy {
    /// `pq_listed`: signers the project lists with an ML-DSA key. They get no classical-only grace:
    /// otherwise deleting their ML-DSA file would downgrade a layer to Ed25519 alone.
    pub fn check(&self, sigs: &[SigInfo], now: &str, pq_listed: &[String]) -> Verdict {
        let mut v = Verdict::default();
        if sigs.is_empty() {
            v.refused.push("unsigned".into());
        }
        // signer → algorithms with a valid, still-trusted signature
        let mut by: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for g in sigs.iter().filter(|g| g.valid) {
            let Some(name) = &g.known_as else {
                v.refused.push(format!("signed by key {} (says {}), which the project doesn't list", g.key_id, g.claimed));
                continue;
            };
            match self.algorithms.get(&g.alg) {
                None => v.refused.push(format!("{name}: {} isn't an algorithm the policy allows", g.alg)),
                Some(a) if past(now, &a.until) => v.refused.push(format!(
                    "{name}: {} is no longer trusted (policy: until {})",
                    g.alg,
                    a.until.as_deref().unwrap_or_default()
                )),
                Some(_) => by.entry(name).or_default().push(&g.alg),
            }
        }
        for (name, algs) in by {
            let missing: Vec<&str> =
                self.signatures.require.iter().map(String::as_str).filter(|r| !algs.contains(r)).collect();
            if missing.is_empty() {
                continue;
            }
            let until = &self.signatures.classical_only_until;
            if algs == [ED25519] && until.is_some() && !past(now, until) && !pq_listed.iter().any(|n| n == name) {
                v.warnings.push(format!(
                    "{name}: classical only (Ed25519), accepted until {}; `ox key upgrade` and sign again",
                    until.as_deref().unwrap_or_default()
                ));
            } else {
                v.refused.push(format!("{name}: no valid {} signature", missing.join(", ")));
            }
        }
        v
    }
}

impl Store {
    /// `.ox/policy.json`, or the built-in default.
    pub fn policy(&self) -> Result<Policy> {
        let p = self.dir.join("policy.json");
        if !p.exists() {
            return Ok(Policy::default());
        }
        let bytes = io(&p, fs::read(&p))?;
        serde_json::from_slice(&bytes).map_err(|e| StoreError::Key(format!("{}: {e}", p.display())))
    }

    /// Names the project lists with an ML-DSA-65 key (`.ox/keys/<name>.mldsa65`).
    pub fn pq_listed(&self) -> Vec<String> {
        self.known("mldsa65").into_iter().map(|(n, _)| n).collect()
    }
}
