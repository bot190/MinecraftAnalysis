//! Complete registry-identity worksheets for item rule authors.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::profile::WorldProfile;
use crate::registry::{RegistryCatalog, RegistryEntry, RegistryKind};
use crate::rules::{self, ItemIdentityProjection, LoadedRules, SourceProfile};

pub const ITEM_MAPPING_REPORT_SCHEMA: u32 = 1;
pub const CANDIDATE_THRESHOLD: u32 = 600;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ItemMappingReport {
    pub report_schema: u32,
    pub source_profile: SourceProfile,
    pub target_profile: String,
    pub rule_sets: Vec<String>,
    pub summary: MappingSummary,
    pub source_items: Vec<SourceItem>,
    pub unmatched_target_items: Vec<TargetItem>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct MappingSummary {
    pub source_items: usize,
    pub explicitly_mapped_source_items: usize,
    pub source_items_with_suggestions: usize,
    pub source_items_without_suggestions: usize,
    pub target_items: usize,
    pub target_items_referenced: usize,
    pub unmatched_target_items: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceItem {
    pub name: String,
    pub numeric_id: i32,
    pub mapping: ItemMapping,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemMapping {
    Explicit {
        rule_id: String,
        /// The rule's declared projection, retained even when it names an alias.
        target_name: String,
        /// Canonical catalog identity when resolved, otherwise the declared name.
        target: ProjectedTarget,
        #[serde(skip_serializing_if = "Option::is_none")]
        diagnostic: Option<MappingDiagnostic>,
    },
    Prospective {
        candidates: Vec<Candidate>,
    },
    None,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectedTarget {
    pub name: String,
    pub numeric_id: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct TargetItem {
    pub name: String,
    pub numeric_id: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Candidate {
    pub name: String,
    pub numeric_id: i32,
    pub score: u32,
    pub evidence: Vec<MappingEvidence>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MappingEvidence {
    ExactIdentity,
    ExactNamespace,
    NamespaceFamily,
    NormalizedPath,
    PathTokenOverlap,
    PathTokenOrder,
    LegacyStructuralAffix,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MappingDiagnostic {
    InvalidTarget,
}

/// Cached lexical features. Schema v1 uses ASCII boundaries, not locale rules.
#[derive(Debug, Eq, PartialEq)]
struct Features {
    identity: String,
    namespace: String,
    namespace_family: String,
    normalized_path: String,
    tokens: Vec<String>,
    token_set: BTreeSet<String>,
    structural_affix: bool,
}

impl Features {
    fn new(identity: &str) -> Self {
        let (namespace, path) = identity
            .split_once(':')
            .expect("catalog names are namespaced");
        let mut tokens = path_tokens(path);
        let original_len = tokens.len();
        while tokens.len() > 1 && matches!(tokens[0].as_str(), "item" | "tile") {
            tokens.remove(0);
        }
        while tokens.len() > 1 && matches!(tokens.last().map(String::as_str), Some("item" | "tile"))
        {
            tokens.pop();
        }
        Self {
            identity: identity.to_owned(),
            namespace: namespace.to_owned(),
            namespace_family: namespace
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .map(|ch| ch.to_ascii_lowercase())
                .collect(),
            normalized_path: tokens.join("_"),
            token_set: tokens.iter().cloned().collect(),
            structural_affix: tokens.len() != original_len,
            tokens,
        }
    }
}

fn path_tokens(path: &str) -> Vec<String> {
    let chars: Vec<_> = path.chars().collect();
    let mut tokens = Vec::new();
    let mut token = String::new();
    for (index, &ch) in chars.iter().enumerate() {
        if !ch.is_ascii_alphanumeric() {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
            continue;
        }
        let previous = index.checked_sub(1).map(|i| chars[i]);
        let next = chars.get(index + 1);
        let boundary = previous.is_some_and(|previous| {
            previous.is_ascii_alphanumeric()
                && (previous.is_ascii_digit() != ch.is_ascii_digit()
                    || (previous.is_ascii_lowercase() && ch.is_ascii_uppercase())
                    || (previous.is_ascii_uppercase()
                        && ch.is_ascii_uppercase()
                        && next.is_some_and(char::is_ascii_lowercase)))
        });
        if boundary && !token.is_empty() {
            tokens.push(std::mem::take(&mut token));
        }
        token.push(ch.to_ascii_lowercase());
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

fn score(source: &Features, target: &Features) -> (u32, Vec<MappingEvidence>) {
    let mut score = 0;
    let mut evidence = Vec::new();
    let mut add = |condition, weight, signal| {
        if condition {
            score += weight;
            evidence.push(signal);
        }
    };
    add(
        source.identity == target.identity,
        1000,
        MappingEvidence::ExactIdentity,
    );
    add(
        source.namespace == target.namespace,
        120,
        MappingEvidence::ExactNamespace,
    );
    add(
        source.namespace != target.namespace
            && !source.namespace_family.is_empty()
            && source.namespace_family == target.namespace_family,
        100,
        MappingEvidence::NamespaceFamily,
    );
    add(
        !source.normalized_path.is_empty() && source.normalized_path == target.normalized_path,
        600,
        MappingEvidence::NormalizedPath,
    );
    let shared: BTreeSet<_> = source.token_set.intersection(&target.token_set).collect();
    if !shared.is_empty() {
        let union = source.token_set.len() + target.token_set.len() - shared.len();
        // Compute in u128 so even very long names cannot overflow the weight calculation.
        let overlap = u32::try_from(600 * shared.len() as u128 / union as u128)
            .expect("overlap weight is at most 600");
        add(true, overlap, MappingEvidence::PathTokenOverlap);
        let ordered = source
            .tokens
            .iter()
            .filter(|token| shared.contains(token))
            .eq(target.tokens.iter().filter(|token| shared.contains(token)));
        add(
            shared.len() >= 2 && ordered,
            100,
            MappingEvidence::PathTokenOrder,
        );
        add(
            source.structural_affix || target.structural_affix,
            20,
            MappingEvidence::LegacyStructuralAffix,
        );
    }
    (score, evidence)
}

fn candidates(source: &Features, targets: &[(TargetItem, Features)]) -> Vec<Candidate> {
    let mut result: Vec<_> = targets
        .iter()
        .filter_map(|(target, features)| {
            let (score, evidence) = score(source, features);
            (score >= CANDIDATE_THRESHOLD).then(|| Candidate {
                name: target.name.clone(),
                numeric_id: target.numeric_id,
                score,
                evidence,
            })
        })
        .collect();
    result.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.numeric_id.cmp(&b.numeric_id))
    });
    result
}

impl From<&RegistryEntry> for TargetItem {
    fn from(entry: &RegistryEntry) -> Self {
        Self {
            name: entry.name.to_string(),
            numeric_id: entry.numeric_id,
        }
    }
}

/// Build a worksheet without observing world objects or executing templates.
///
/// # Errors
/// Returns an internal-data error if source enumeration, counts, or target partition disagree.
pub fn analyze(
    source: &RegistryCatalog,
    target: &RegistryCatalog,
    loaded: &LoadedRules,
) -> crate::Result<ItemMappingReport> {
    let source_profile = WorldProfile::from(loaded.source_profile);
    let targets: Vec<_> = target
        .entries()
        .filter(|entry| {
            entry.kind == RegistryKind::Item
                && !WorldProfile::Forge1_12_2.is_stock_item(&entry.name)
        })
        .map(|entry| (TargetItem::from(entry), Features::new(entry.name.as_str())))
        .collect();
    let non_stock_targets: BTreeSet<_> = targets.iter().map(|(item, _)| item.clone()).collect();
    let all_targets: BTreeSet<_> = target
        .entries()
        .filter(|entry| entry.kind == RegistryKind::Item)
        .map(TargetItem::from)
        .collect();
    let mut referenced = BTreeSet::new();
    let mut source_items = Vec::new();
    let mut summary = MappingSummary {
        target_items: non_stock_targets.len(),
        ..MappingSummary::default()
    };
    for entry in source.entries().filter(|entry| {
        entry.kind == RegistryKind::Item && !source_profile.is_stock_item(&entry.name)
    }) {
        let mapping = if let ItemIdentityProjection::Explicit {
            rule_id,
            target_name,
            target,
        } =
            rules::assess_item_identity_projection(loaded, &entry.name, entry.numeric_id, target)
        {
            summary.explicitly_mapped_source_items += 1;
            if let Some(target) = target {
                referenced.insert(TargetItem::from(target));
            }
            ItemMapping::Explicit {
                rule_id: rule_id.to_owned(),
                target_name: target_name.to_owned(),
                target: ProjectedTarget {
                    name: target
                        .map_or_else(|| target_name.to_owned(), |entry| entry.name.to_string()),
                    numeric_id: target.map(|entry| entry.numeric_id),
                },
                diagnostic: target.is_none().then_some(MappingDiagnostic::InvalidTarget),
            }
        } else {
            let candidates = candidates(&Features::new(entry.name.as_str()), &targets);
            if candidates.is_empty() {
                summary.source_items_without_suggestions += 1;
                ItemMapping::None
            } else {
                summary.source_items_with_suggestions += 1;
                referenced.extend(candidates.iter().map(|candidate| TargetItem {
                    name: candidate.name.clone(),
                    numeric_id: candidate.numeric_id,
                }));
                ItemMapping::Prospective { candidates }
            }
        };
        source_items.push(SourceItem {
            name: entry.name.to_string(),
            numeric_id: entry.numeric_id,
            mapping,
        });
    }
    source_items.sort_by(|a, b| {
        a.numeric_id
            .cmp(&b.numeric_id)
            .then_with(|| a.name.cmp(&b.name))
    });
    let unmatched_target_items: Vec<_> =
        non_stock_targets.difference(&referenced).cloned().collect();
    summary.target_items = non_stock_targets.union(&referenced).count();
    summary.source_items = source_items.len();
    summary.target_items_referenced = referenced.len();
    summary.unmatched_target_items = unmatched_target_items.len();
    let report = ItemMappingReport {
        report_schema: ITEM_MAPPING_REPORT_SCHEMA,
        source_profile: loaded.source_profile,
        target_profile: "forge-1.12.2".into(),
        rule_sets: loaded
            .documents
            .iter()
            .map(|(_, document)| document.rule_set.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        summary,
        source_items,
        unmatched_target_items,
    };
    validate(&report, source, &all_targets)?;
    Ok(report)
}

fn validate(
    report: &ItemMappingReport,
    source: &RegistryCatalog,
    targets: &BTreeSet<TargetItem>,
) -> crate::Result<()> {
    let fail = || {
        crate::Error::InvalidData(
            "item-mapping report violates registry accounting invariants".into(),
        )
    };
    let expected_source: BTreeSet<_> = source
        .entries()
        .filter(|entry| {
            entry.kind == RegistryKind::Item
                && !WorldProfile::from(report.source_profile).is_stock_item(&entry.name)
        })
        .map(TargetItem::from)
        .collect();
    let actual_source: BTreeSet<_> = report
        .source_items
        .iter()
        .map(|entry| TargetItem {
            name: entry.name.clone(),
            numeric_id: entry.numeric_id,
        })
        .collect();
    let mut referenced = BTreeSet::new();
    let mut counts = [0; 3];
    for item in &report.source_items {
        match &item.mapping {
            ItemMapping::Explicit {
                target, diagnostic, ..
            } => {
                counts[0] += 1;
                if let Some(numeric_id) = target.numeric_id {
                    if diagnostic.is_some() {
                        return Err(fail());
                    }
                    referenced.insert(TargetItem {
                        name: target.name.clone(),
                        numeric_id,
                    });
                } else if *diagnostic != Some(MappingDiagnostic::InvalidTarget) {
                    return Err(fail());
                }
            }
            ItemMapping::Prospective { candidates } => {
                counts[1] += 1;
                if candidates.is_empty() {
                    return Err(fail());
                }
                for candidate in candidates {
                    if candidate.score < CANDIDATE_THRESHOLD
                        || candidate.evidence.is_empty()
                        || is_stock_target(&candidate.name)
                    {
                        return Err(fail());
                    }
                    referenced.insert(TargetItem {
                        name: candidate.name.clone(),
                        numeric_id: candidate.numeric_id,
                    });
                }
            }
            ItemMapping::None => counts[2] += 1,
        }
    }
    let unmatched: BTreeSet<_> = report.unmatched_target_items.iter().cloned().collect();
    let non_stock_targets: BTreeSet<_> = targets
        .iter()
        .filter(|entry| !is_stock_target(&entry.name))
        .cloned()
        .collect();
    let worksheet_targets: BTreeSet<_> = non_stock_targets.union(&referenced).cloned().collect();
    let expected_summary = MappingSummary {
        source_items: expected_source.len(),
        explicitly_mapped_source_items: counts[0],
        source_items_with_suggestions: counts[1],
        source_items_without_suggestions: counts[2],
        target_items: worksheet_targets.len(),
        target_items_referenced: referenced.len(),
        unmatched_target_items: unmatched.len(),
    };
    if actual_source != expected_source
        || report.source_items.len() != actual_source.len()
        || report.summary != expected_summary
        || unmatched.len() != report.unmatched_target_items.len()
        || !referenced.is_subset(targets)
        || unmatched != non_stock_targets.difference(&referenced).cloned().collect()
        || !referenced.is_disjoint(&unmatched)
        || referenced
            .union(&unmatched)
            .cloned()
            .collect::<BTreeSet<_>>()
            != worksheet_targets
    {
        return Err(fail());
    }
    Ok(())
}

fn is_stock_target(name: &str) -> bool {
    crate::registry::RegistryName::parse(name)
        .is_ok_and(|name| WorldProfile::Forge1_12_2.is_stock_item(&name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{Alias, Provenance, RegistryName};
    use serde_json::json;

    fn catalog(entries: &[(&str, i32)]) -> RegistryCatalog {
        let mut result = RegistryCatalog::default();
        for &(name, numeric_id) in entries {
            result
                .insert(RegistryEntry {
                    kind: RegistryKind::Item,
                    name: RegistryName::parse(name).unwrap(),
                    numeric_id,
                    provenance: Provenance::world("test", "fixture"),
                })
                .unwrap();
        }
        result
    }

    #[test]
    fn normalization_preserves_meaningful_tokens_and_splits_legacy_boundaries() {
        for (identity, namespace, path, affix) in [
            (
                "legacy:item.PipeItemsWood",
                "legacy",
                "pipe_items_wood",
                true,
            ),
            ("legacy:tile.machineBlock", "legacy", "machine_block", true),
            (
                "BiblioCraft:Typesetting Machine",
                "bibliocraft",
                "typesetting_machine",
                false,
            ),
            (
                "BuildCraft|Transport:item.XMLParser2Gear",
                "buildcrafttransport",
                "xml_parser_2_gear",
                true,
            ),
            ("mod:2DItem", "mod", "2_d", true),
            ("mod:gear_item", "mod", "gear", true),
            ("mod:item_tile", "mod", "tile", true),
            ("mod:item", "mod", "item", false),
            ("mod:pipe_item_wood", "mod", "pipe_item_wood", false),
            ("mod:XMLHTTPGear", "mod", "xmlhttp_gear", false),
            ("mod:iron//gear--2", "mod", "iron_gear_2", false),
            ("mod:___", "mod", "", false),
        ] {
            let features = Features::new(identity);
            assert_eq!(features.identity, identity);
            assert_eq!(features.namespace_family, namespace, "{identity}");
            assert_eq!(features.normalized_path, path, "{identity}");
            assert_eq!(features.structural_affix, affix, "{identity}");
        }
    }

    #[test]
    fn fixed_scores_and_evidence_are_auditable() {
        use MappingEvidence::{
            ExactIdentity, ExactNamespace, LegacyStructuralAffix, NamespaceFamily, NormalizedPath,
            PathTokenOrder, PathTokenOverlap,
        };
        for (source, target, expected_score, expected_evidence) in [
            (
                "mod:wood_gear",
                "mod:wood_gear",
                2420,
                vec![
                    ExactIdentity,
                    ExactNamespace,
                    NormalizedPath,
                    PathTokenOverlap,
                    PathTokenOrder,
                ],
            ),
            (
                "legacy:item.WoodGear",
                "buildcrafttransport:wood_gear",
                1320,
                vec![
                    NormalizedPath,
                    PathTokenOverlap,
                    PathTokenOrder,
                    LegacyStructuralAffix,
                ],
            ),
            (
                "BuildCraft|Transport:wood_gear",
                "buildcrafttransport:wood_gear",
                1400,
                vec![
                    NamespaceFamily,
                    NormalizedPath,
                    PathTokenOverlap,
                    PathTokenOrder,
                ],
            ),
            (
                "mod:wood_gear",
                "mod:wood_plate",
                320,
                vec![ExactNamespace, PathTokenOverlap],
            ),
            (
                "mod:iron_wood_gear",
                "mod:wood_gear",
                620,
                vec![ExactNamespace, PathTokenOverlap, PathTokenOrder],
            ),
            (
                "old:wood_gear",
                "new:gear_wood",
                600,
                vec![PathTokenOverlap],
            ),
            (
                "old:iron_wood_gear",
                "new:wood_gear",
                500,
                vec![PathTokenOverlap, PathTokenOrder],
            ),
            (
                "legacy:item.WoodGear",
                "buildcrafttransport:plug_pulsar",
                0,
                vec![],
            ),
            ("mod:___", "other:___", 0, vec![]),
        ] {
            assert_eq!(
                score(&Features::new(source), &Features::new(target)),
                (expected_score, expected_evidence),
                "{source} -> {target}"
            );
        }
    }

    #[test]
    fn candidates_apply_threshold_and_stable_tie_breaks() {
        let targets = [
            ("z:wood_gear", 2),
            ("a:wood_gear", 9),
            ("a:wood_gear", 3),
            ("old:wood_plate", 4),
            ("new:gear_wood", 5),
            ("new:iron_wood_gear", 6),
        ]
        .map(|(name, numeric_id)| {
            (
                TargetItem {
                    name: name.into(),
                    numeric_id,
                },
                Features::new(name),
            )
        });
        let source = Features::new("old:wood_gear");
        let first = candidates(&source, &targets);
        assert_eq!(
            first
                .iter()
                .map(|entry| (entry.name.as_str(), entry.numeric_id, entry.score))
                .collect::<Vec<_>>(),
            [
                ("a:wood_gear", 3, 1300),
                ("a:wood_gear", 9, 1300),
                ("z:wood_gear", 2, 1300),
                ("new:gear_wood", 5, 600)
            ]
        );
        let bytes = serde_json::to_vec_pretty(&first).unwrap();
        let mut reversed = targets.into_iter().rev().collect::<Vec<_>>();
        for _ in 0..3 {
            assert_eq!(
                serde_json::to_vec_pretty(&candidates(&source, &reversed)).unwrap(),
                bytes
            );
            reversed.rotate_left(1);
        }
    }

    fn loaded_rules() -> LoadedRules {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rules.yaml");
        std::fs::write(&path, json!({
            "schema_version":1,"rule_set":"fixture","source_profile":"forge-1.7.10",
            "rules":[
                {"id":"first","object":"item","matcher":{"name":"old:explicit"},"target_name":"new:alias","template":"{{ must_never_render() }}"},
                {"id":"second","object":"item","matcher":{"name":"old:other"},"target_name":"new:wood_gear","template":"{}"},
                {"id":"invalid","object":"item","matcher":{"name":"old:invalid"},"target_name":"new:absent","template":"{}"},
                {"id":"conditional","priority":10,"object":"item","matcher":{"name":"old:wood_gear","count":{"mode":"exact","value":1}},"target_name":"new:unrelated","template":"{}"},
                {"id":"later","object":"item","matcher":{"name":"old:wood_gear"},"target_name":"new:unrelated","template":"{}"},
                {"id":"no-projection","object":"item","matcher":{"name":"legacy:item.WoodGear"},"template":"{\"disposition\":\"drop\"}"}
            ]
        }).to_string()).unwrap();
        rules::load(&path).unwrap()
    }

    #[test]
    fn worksheet_accounts_for_every_identity_and_counts_distinct_targets() {
        let source = catalog(&[
            ("old:explicit", 7),
            ("old:other", 2),
            ("old:invalid", 3),
            ("old:wood_gear", 4),
            ("old:unmatchable", 5),
            ("legacy:item.WoodGear", 6),
        ]);
        let mut target = catalog(&[("new:wood_gear", 50), ("new:unrelated", 51)]);
        target
            .insert_alias(Alias {
                kind: RegistryKind::Item,
                from: RegistryName::parse("new:alias").unwrap(),
                to: RegistryName::parse("new:wood_gear").unwrap(),
                provenance: Provenance::world("test", "alias"),
            })
            .unwrap();
        let loaded = loaded_rules();
        let report = analyze(&source, &target, &loaded).unwrap();
        assert_eq!(
            report.summary,
            MappingSummary {
                source_items: 6,
                explicitly_mapped_source_items: 3,
                source_items_with_suggestions: 2,
                source_items_without_suggestions: 1,
                target_items: 2,
                target_items_referenced: 1,
                unmatched_target_items: 1,
            }
        );
        assert_eq!(
            report
                .source_items
                .iter()
                .map(|entry| entry.numeric_id)
                .collect::<Vec<_>>(),
            [2, 3, 4, 5, 6, 7]
        );
        assert_eq!(
            report.unmatched_target_items,
            vec![TargetItem {
                name: "new:unrelated".into(),
                numeric_id: 51
            }]
        );
        assert!(
            matches!(&report.source_items[1].mapping, ItemMapping::Explicit { target, diagnostic: Some(MappingDiagnostic::InvalidTarget), .. } if target.name == "new:absent" && target.numeric_id.is_none())
        );
        assert!(
            matches!(&report.source_items[5].mapping, ItemMapping::Explicit { target_name, target, .. } if target_name == "new:alias" && target.name == "new:wood_gear" && target.numeric_id == Some(50))
        );
        assert!(matches!(
            report.source_items[2].mapping,
            ItemMapping::Prospective { .. }
        ));
        assert!(matches!(report.source_items[3].mapping, ItemMapping::None));
        assert!(matches!(
            report.source_items[4].mapping,
            ItemMapping::Prospective { .. }
        ));
        assert_eq!(report.rule_sets, ["fixture"]);
        assert_eq!(
            serde_json::to_vec_pretty(&report).unwrap(),
            serde_json::to_vec_pretty(&analyze(&source, &target, &loaded).unwrap()).unwrap()
        );

        let all_targets = target.entries().map(TargetItem::from).collect();
        let mut corrupted = report.clone();
        corrupted.summary.target_items_referenced += 1;
        assert!(validate(&corrupted, &source, &all_targets).is_err());
        let mut corrupted = report.clone();
        corrupted.unmatched_target_items.clear();
        assert!(validate(&corrupted, &source, &all_targets).is_err());
        let mut corrupted = report;
        corrupted
            .source_items
            .push(corrupted.source_items[0].clone());
        assert!(validate(&corrupted, &source, &all_targets).is_err());
    }

    fn stock_mapping_rules() -> LoadedRules {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rules.yaml");
        std::fs::write(&path, json!({
            "schema_version":1,"rule_set":"stock-fixture","source_profile":"forge-1.7.10",
            "rules":[
                {"id":"stock-source-rule","object":"item","matcher":{"name":"minecraft:apple"},"target_name":"new:unused","template":"{}"},
                {"id":"to-stock","object":"item","matcher":{"name":"mod:to_stock"},"target_name":"minecraft:stone","template":"{}"},
                {"id":"also-to-stock","object":"item","matcher":{"name":"mod:also_to_stock"},"target_name":"minecraft:rock","template":"{}"},
                {"id":"missing-stock","object":"item","matcher":{"name":"mod:missing_stock"},"target_name":"minecraft:elytra","template":"{}"}
            ]
        }).to_string()).unwrap();
        rules::load(&path).unwrap()
    }

    fn stock_mapping_catalogs(provenance: Provenance) -> (RegistryCatalog, RegistryCatalog) {
        let mut source = catalog(&[
            ("mod:also_to_stock", 1),
            ("mod:missing_stock", 2),
            ("mod:to_stock", 3),
            ("old:item.Stone", 4),
            ("legacy:item.WoodGear", 5),
            // A modded identity in a familiar stock numeric slot stays non-stock.
            ("mod:stock_slot", 260),
        ]);
        source
            .insert(RegistryEntry {
                kind: RegistryKind::Item,
                name: RegistryName::parse("minecraft:apple").unwrap(),
                numeric_id: 9_999,
                provenance,
            })
            .unwrap();
        let mut target = catalog(&[
            ("minecraft:stone", 400),
            ("new:unused", 401),
            ("new:wood_gear", 402),
        ]);
        target
            .insert_alias(Alias {
                kind: RegistryKind::Item,
                from: RegistryName::parse("minecraft:rock").unwrap(),
                to: RegistryName::parse("minecraft:stone").unwrap(),
                provenance: Provenance::world("test", "stock alias"),
            })
            .unwrap();
        (source, target)
    }

    fn mapping<'a>(report: &'a ItemMappingReport, name: &str) -> &'a ItemMapping {
        &report
            .source_items
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
            .mapping
    }

    fn assert_stock_mapping_report(report: &ItemMappingReport) {
        assert_eq!(report.source_items.len(), 6);
        assert!(report
            .source_items
            .iter()
            .all(|entry| entry.name != "minecraft:apple"));
        assert!(matches!(
            mapping(report, "mod:to_stock"),
            ItemMapping::Explicit { target_name, target, diagnostic: None, .. }
                if target_name == "minecraft:stone"
                    && target.name == "minecraft:stone"
                    && target.numeric_id == Some(400)
        ));
        assert!(matches!(
            mapping(report, "mod:also_to_stock"),
            ItemMapping::Explicit { target_name, target, diagnostic: None, .. }
                if target_name == "minecraft:rock"
                    && target.name == "minecraft:stone"
                    && target.numeric_id == Some(400)
        ));
        assert!(matches!(
            mapping(report, "mod:missing_stock"),
            ItemMapping::Explicit { target_name, target, diagnostic: Some(MappingDiagnostic::InvalidTarget), .. }
                if target_name == "minecraft:elytra"
                    && target.name == "minecraft:elytra"
                    && target.numeric_id.is_none()
        ));
        // A strong lexical stock match is not a prospective candidate.
        assert!(matches!(
            mapping(report, "old:item.Stone"),
            ItemMapping::None
        ));
        assert!(matches!(
            mapping(report, "legacy:item.WoodGear"),
            ItemMapping::Prospective { candidates }
                if candidates.iter().map(|candidate| candidate.name.as_str()).eq(["new:wood_gear"])
        ));
        assert_eq!(
            report.unmatched_target_items,
            vec![TargetItem {
                name: "new:unused".into(),
                numeric_id: 401,
            }]
        );
        assert_eq!(
            report.summary,
            MappingSummary {
                source_items: 6,
                explicitly_mapped_source_items: 3,
                source_items_with_suggestions: 1,
                source_items_without_suggestions: 2,
                // N = {new:unused, new:wood_gear}; B = {minecraft:stone}.
                target_items: 3,
                // Repeated canonical stock references count once.
                target_items_referenced: 2,
                unmatched_target_items: 1,
            }
        );
    }

    #[test]
    fn worksheet_excludes_stock_and_accounts_for_explicit_stock_targets() {
        let loaded = stock_mapping_rules();
        for provenance in [
            Provenance::world("forge-1.7.10", "FML.ItemData"),
            Provenance::built_in("forge-1.7.10"),
        ] {
            let (source, target) = stock_mapping_catalogs(provenance);
            assert_stock_mapping_report(&analyze(&source, &target, &loaded).unwrap());
        }

        let stock_only_source = catalog(&[("minecraft:apple", 260)]);
        let stock_only_target = catalog(&[("minecraft:stone", 1)]);
        assert_eq!(
            analyze(&stock_only_source, &stock_only_target, &loaded)
                .unwrap()
                .summary,
            MappingSummary::default()
        );
    }

    #[test]
    fn empty_catalogs_and_no_suggestions_still_account_for_targets() {
        let empty = RegistryCatalog::default();
        let loaded = LoadedRules::empty(SourceProfile::Forge1_2_5);
        assert_eq!(
            analyze(&empty, &empty, &loaded).unwrap().summary,
            MappingSummary::default()
        );
        let source = catalog(&[("old:gear", 1)]);
        let target = catalog(&[("z:pulsar", 3), ("a:wrench", 8)]);
        let report = analyze(&source, &target, &loaded).unwrap();
        assert_eq!(report.summary.source_items_without_suggestions, 1);
        assert_eq!(report.summary.target_items_referenced, 0);
        assert_eq!(
            report
                .unmatched_target_items
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["a:wrench", "z:pulsar"]
        );
        assert_eq!(
            analyze(&source, &empty, &loaded)
                .unwrap()
                .summary
                .source_items_without_suggestions,
            1
        );
        assert_eq!(
            analyze(&empty, &target, &loaded)
                .unwrap()
                .summary
                .unmatched_target_items,
            2
        );
    }

    #[test]
    fn schema_and_variants_serialize_without_inapplicable_fields() {
        let report = ItemMappingReport {
            report_schema: ITEM_MAPPING_REPORT_SCHEMA,
            source_profile: SourceProfile::Forge1_2_5,
            target_profile: "forge-1.12.2".into(),
            rule_sets: vec!["example".into()],
            summary: MappingSummary::default(),
            source_items: vec![],
            unmatched_target_items: vec![],
        };
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["report_schema"], 1);
        assert_eq!(value["source_profile"], "forge-1.2.5");
        assert_eq!(
            serde_json::to_value(ItemMapping::None).unwrap(),
            json!({"kind":"none"})
        );
        let explicit = |numeric_id, diagnostic| ItemMapping::Explicit {
            rule_id: "map".into(),
            target_name: "mod:item".into(),
            target: ProjectedTarget {
                name: "mod:item".into(),
                numeric_id,
            },
            diagnostic,
        };
        assert_eq!(
            serde_json::to_value(explicit(None, Some(MappingDiagnostic::InvalidTarget))).unwrap(),
            json!({
                "kind":"explicit", "rule_id":"map", "target_name":"mod:item",
                "target":{"name":"mod:item", "numeric_id":null}, "diagnostic":"invalid-target"
            })
        );
        assert_eq!(
            serde_json::to_value(explicit(Some(12), None)).unwrap(),
            json!({
                "kind":"explicit", "rule_id":"map", "target_name":"mod:item",
                "target":{"name":"mod:item", "numeric_id":12}
            })
        );
        let prospective = ItemMapping::Prospective {
            candidates: vec![Candidate {
                name: "mod:item".into(),
                numeric_id: 12,
                score: 1000,
                evidence: vec![MappingEvidence::ExactIdentity],
            }],
        };
        assert_eq!(
            serde_json::to_value(prospective).unwrap(),
            json!({
                "kind":"prospective", "candidates":[{"name":"mod:item", "numeric_id":12,
                    "score":1000, "evidence":["exact-identity"]}]
            })
        );
    }
}
