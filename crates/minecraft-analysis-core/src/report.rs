//! Deterministic machine-readable migration reports and object locations.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::preflight::PreflightResult;
use crate::rules::CandidateOutcome;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Unchanged,
    Transformed,
    Deleted,
    ReplacedWithAir,
    Dropped,
    Unresolved,
    Copied,
    Excluded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InputFingerprint {
    pub role: String,
    pub algorithm: String,
    pub digest: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ObjectLocation {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk: Option<[i32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<[i32; 3]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nbt_path: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectRecord {
    pub kind: String,
    pub source_identity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_identity: Option<String>,
    pub disposition: Disposition,
    pub location: ObjectLocation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<CandidateOutcome>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_maps: Vec<crate::template::ValueMapCallOutcome>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub template_diagnostics: Vec<TemplateDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum TemplateDiagnostic {
    SelectedTemplate {
        rule_id: String,
        template: String,
    },
    Render {
        success: bool,
        detail: Option<String>,
    },
    TypedDecode {
        success: bool,
        detail: Option<String>,
    },
    NestedItemCall {
        outcome: crate::rules::NestedItemCallOutcome,
    },
    IdentityMap {
        outcome: crate::rules::IdentityMapOutcome,
    },
    Resolution {
        identity: Option<String>,
        success: bool,
    },
    Disposition {
        disposition: Disposition,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: String,
    pub disposition: Disposition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MigrationReport {
    pub report_schema: u32,
    pub outcome: Outcome,
    pub source_profile: String,
    pub input_fingerprints: Vec<InputFingerprint>,
    pub rule_sets: Vec<String>,
    pub registry_mappings: BTreeMap<String, String>,
    pub counts: BTreeMap<Disposition, u64>,
    pub objects: crate::spool::RecordStore<ObjectRecord>,
    pub files: crate::spool::RecordStore<FileRecord>,
    pub validation_findings: crate::spool::RecordStore<crate::inventory::ValidationFinding>,
    pub warnings: Vec<String>,
    #[serde(skip)]
    pub opaque_files: Vec<crate::preflight::OpaqueFile>,
}

impl Default for MigrationReport {
    fn default() -> Self {
        Self {
            report_schema: 1,
            outcome: Outcome::Failed,
            source_profile: String::new(),
            input_fingerprints: Vec::new(),
            rule_sets: Vec::new(),
            registry_mappings: BTreeMap::new(),
            counts: BTreeMap::new(),
            objects: crate::spool::RecordStore::default(),
            files: crate::spool::RecordStore::default(),
            validation_findings: crate::spool::RecordStore::default(),
            warnings: Vec::new(),
            opaque_files: Vec::new(),
        }
    }
}

impl MigrationReport {
    #[must_use]
    pub fn with_source_profile(mut self, profile: crate::rules::SourceProfile) -> Self {
        self.source_profile = profile.as_str().into();
        self
    }
    #[must_use]
    pub fn from_preflight(preflight: PreflightResult, rule_sets: Vec<String>) -> Self {
        let outcome = if preflight.can_convert() {
            Outcome::Success
        } else {
            Outcome::Failed
        };
        let opaque_files = preflight.opaque_files;
        let mut report = Self {
            outcome,
            input_fingerprints: preflight.fingerprints,
            rule_sets,
            objects: preflight.objects,
            files: preflight.files,
            counts: preflight.counts,
            validation_findings: preflight.validation_findings,
            opaque_files,
            ..Self::default()
        };
        report.normalize();
        report
    }

    /// Append one object record in report-contribution order.
    ///
    /// # Errors
    ///
    /// Returns a report-spool error.
    pub fn record_object(&mut self, record: &ObjectRecord) -> crate::spool::Result<()> {
        *self.counts.entry(record.disposition.clone()).or_default() += 1;
        self.objects.push(record)
    }

    /// Append one file record in report-contribution order.
    ///
    /// # Errors
    ///
    /// Returns a report-spool error.
    pub fn record_file(&mut self, record: &FileRecord) -> crate::spool::Result<()> {
        self.files.push(record)
    }

    /// Normalize metadata collections whose order remains semantic or canonical.
    pub fn normalize(&mut self) {
        self.input_fingerprints.sort_by(|a, b| a.role.cmp(&b.role));
        self.rule_sets.sort();
        self.rule_sets.dedup();
        self.warnings.sort();
        self.warnings.dedup();
    }

    /// Serialize pretty JSON; report record arrays retain contribution order.
    ///
    /// # Errors
    ///
    /// Returns a JSON serialization error.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Serialize pretty JSON directly to a writer.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or writing fails.
    pub fn write_json_pretty(&self, writer: impl std::io::Write) -> serde_json::Result<()> {
        serde_json::to_writer_pretty(writer, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_retain_contribution_order_and_count_dispositions() {
        let record = |file: &str| ObjectRecord {
            kind: "block".into(),
            source_identity: "mod:block".into(),
            target_identity: None,
            disposition: Disposition::Unresolved,
            location: ObjectLocation {
                file: file.into(),
                dimension: Some("DIM7".into()),
                chunk: Some([0, 0]),
                block: Some([1, 2, 3]),
                nbt_path: vec![],
            },
            rules: vec![],
            candidates: vec![],
            value_maps: vec![],
            template_diagnostics: vec![],
            diagnostic: Some("missing target mapping".into()),
        };
        let mut report = MigrationReport::default();
        report.record_object(&record("a.mca")).unwrap();
        report.record_object(&record("z.mca")).unwrap();
        assert_eq!(report.objects.read_all().unwrap()[0].location.file, "a.mca");
        assert_eq!(report.counts[&Disposition::Unresolved], 2);
        assert_eq!(
            report.to_json_pretty().unwrap(),
            report.to_json_pretty().unwrap()
        );
    }

    #[test]
    fn completion_order_changes_bytes_but_not_semantic_object_data() {
        let record = |file: &str| ObjectRecord {
            kind: "block".into(),
            source_identity: "mod:block".into(),
            target_identity: None,
            disposition: Disposition::Unresolved,
            location: ObjectLocation {
                file: file.into(),
                dimension: Some("overworld".into()),
                chunk: Some([0, 0]),
                block: Some([0, 0, 0]),
                nbt_path: vec![],
            },
            rules: vec![],
            candidates: vec![],
            value_maps: vec![],
            template_diagnostics: vec![],
            diagnostic: Some("missing target mapping".into()),
        };
        let build = |files: &[&str]| {
            let mut report = MigrationReport::default();
            for file in files {
                report.record_object(&record(file)).unwrap();
            }
            report
        };
        let forward = build(&["a.mca", "z.mca"]);
        let reverse = build(&["z.mca", "a.mca"]);
        assert_ne!(
            forward.to_json_pretty().unwrap(),
            reverse.to_json_pretty().unwrap()
        );
        let normalized = |report: &MigrationReport| {
            let mut records = report.objects.read_all().unwrap();
            records.sort_by(|left, right| left.location.file.cmp(&right.location.file));
            records
        };
        assert_eq!(normalized(&forward), normalized(&reverse));
        assert_eq!(forward.counts, reverse.counts);
    }

    #[test]
    fn normalizes_file_rule_warning_and_fingerprint_order() {
        let mut report = MigrationReport {
            rule_sets: vec!["z".into(), "a".into(), "a".into()],
            warnings: vec!["z".into(), "a".into()],
            input_fingerprints: vec![
                InputFingerprint {
                    role: "template".into(),
                    algorithm: "x".into(),
                    digest: "2".into(),
                },
                InputFingerprint {
                    role: "source".into(),
                    algorithm: "x".into(),
                    digest: "1".into(),
                },
            ],
            ..MigrationReport::default()
        };
        report
            .record_file(&FileRecord {
                path: "a".into(),
                disposition: Disposition::Copied,
                diagnostic: None,
            })
            .unwrap();
        report
            .record_file(&FileRecord {
                path: "z".into(),
                disposition: Disposition::Excluded,
                diagnostic: Some("lock".into()),
            })
            .unwrap();
        report.normalize();
        assert_eq!(report.rule_sets, ["a", "z"]);
        assert_eq!(report.files.read_all().unwrap()[0].path, "a");
        assert_eq!(report.input_fingerprints[0].role, "source");
    }

    #[test]
    fn in_memory_and_forced_spill_reports_are_json_equivalent() {
        let record = ObjectRecord {
            kind: "item".into(),
            source_identity: "mod:item".into(),
            target_identity: Some("mod:item2".into()),
            disposition: Disposition::Transformed,
            location: ObjectLocation {
                file: "playerdata/a.dat".into(),
                dimension: None,
                chunk: None,
                block: None,
                nbt_path: vec!["Inventory".into(), "0".into()],
            },
            rules: vec!["rule".into()],
            candidates: vec![],
            value_maps: vec![],
            template_diagnostics: vec![],
            diagnostic: None,
        };
        let mut memory = MigrationReport::default();
        let mut spilled = MigrationReport {
            objects: crate::spool::RecordStore::new(crate::spool::Threshold::new(0, 0)),
            files: crate::spool::RecordStore::new(crate::spool::Threshold::new(0, 0)),
            ..MigrationReport::default()
        };
        for report in [&mut memory, &mut spilled] {
            report.record_object(&record).unwrap();
            report
                .record_file(&FileRecord {
                    path: "playerdata/a.dat".into(),
                    disposition: Disposition::Copied,
                    diagnostic: None,
                })
                .unwrap();
        }
        assert!(spilled.objects.is_spilled());
        assert!(spilled.files.is_spilled());
        assert_eq!(
            memory.to_json_pretty().unwrap(),
            spilled.to_json_pretty().unwrap()
        );
    }

    #[test]
    fn streaming_report_propagates_output_failures() {
        struct FailingWriter;
        impl std::io::Write for FailingWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("synthetic output exhaustion"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("synthetic flush failure"))
            }
        }
        let error = MigrationReport::default()
            .write_json_pretty(FailingWriter)
            .unwrap_err();
        assert!(error.to_string().contains("synthetic output exhaustion"));
    }

    #[test]
    fn validation_findings_spill_serialize_and_do_not_fail_successful_outcome() {
        let finding = crate::inventory::ValidationFinding {
            code: crate::inventory::INVALID_INVENTORY_SHAPE.into(),
            file: "data/death.dat".into(),
            nbt_path: crate::rules::NbtPath(vec![crate::rules::PathElement::Field(
                "Inventory".into(),
            )]),
            expected_shape: crate::inventory::EXPECTED_INVENTORY_SHAPE.into(),
            observed_incompatibility: "found Compound".into(),
        };
        let mut findings = crate::spool::RecordStore::new(crate::spool::Threshold::new(0, 0));
        findings.push(&finding).unwrap();
        let preflight = PreflightResult {
            fingerprints: vec![],
            objects: crate::spool::RecordStore::default(),
            files: crate::spool::RecordStore::default(),
            counts: BTreeMap::new(),
            unresolved_count: 0,
            source_bytes: 0,
            estimated_staging_bytes: 0,
            opaque_files: vec![],
            validation_findings: findings,
        };
        let report = MigrationReport::from_preflight(preflight, vec![]);
        assert_eq!(report.outcome, Outcome::Success);
        assert!(report.validation_findings.is_spilled());
        assert!(report
            .to_json_pretty()
            .unwrap()
            .contains("invalid_inventory_shape"));
    }
}
