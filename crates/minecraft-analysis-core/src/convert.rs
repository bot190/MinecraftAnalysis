//! Conversion orchestration. Storage, rules, and reporting meet here.

use crate::nbt::Value;
use crate::report::Disposition;
use std::collections::BTreeMap;

use crate::rules::{
    apply_patches_with_maps, Decision, ExecutionDecision, NumericTransform, ObjectAction,
    SelectedAction, ValueMap,
};

#[derive(Clone, Debug, PartialEq)]
pub struct TransformObject {
    pub identity: String,
    pub numeric: i64,
    pub nbt: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppliedDecision {
    pub object: Option<TransformObject>,
    pub disposition: Disposition,
    pub responsible_rules: Vec<String>,
    pub map_outcomes: Vec<AppliedMapOutcome>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AppliedMapOutcome {
    pub rule_id: String,
    #[serde(flatten)]
    pub mapping: crate::rules::MapOutcome,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("object {identity} has no target mapping and no explicit loss policy")]
    Unresolved { identity: String },
    #[error("rule {rule_id} cannot patch absent NBT")]
    MissingNbt { rule_id: String },
    #[error("rule {rule_id} failed to patch NBT: {source}")]
    Patch {
        rule_id: String,
        source: Box<crate::rules::Error>,
    },
}

/// Apply a precomputed immutable decision with explicit loss semantics.
///
/// `target_exists` checks semantic identities against the target catalog. With
/// no selected action, identity preservation succeeds only when that identity
/// is representable; otherwise the default is an unresolved error.
///
/// # Errors
///
/// Returns an unresolved error for absent mappings without an explicit policy,
/// or a contextual error when a selected patch cannot be applied.
pub fn apply_decision(
    decision: &Decision,
    object: TransformObject,
    target_exists: impl Fn(&str) -> bool,
) -> Result<AppliedDecision, Error> {
    apply_decision_with_maps(decision, object, &BTreeMap::new(), target_exists)
}

/// Apply a decision with reusable typed value maps.
///
/// # Errors
///
/// Returns unresolved-target, missing-NBT, or typed-patch errors with the
/// responsible rule identifier where applicable.
pub fn apply_decision_with_maps(
    decision: &Decision,
    object: TransformObject,
    value_maps: &BTreeMap<String, ValueMap>,
    target_exists: impl Fn(&str) -> bool,
) -> Result<AppliedDecision, Error> {
    let actions = decision
        .actions
        .iter()
        .map(|(rule_id, action)| SelectedAction { rule_id, action })
        .collect::<Vec<_>>();
    apply_selected_actions(&actions, object, value_maps, target_exists)
}

/// Apply a borrowed execution decision with reusable typed value maps.
///
/// # Errors
///
/// Returns unresolved-target, missing-NBT, or typed-patch errors with the
/// responsible rule identifier where applicable.
pub fn apply_execution_decision_with_maps(
    decision: &ExecutionDecision<'_>,
    object: TransformObject,
    value_maps: &BTreeMap<String, ValueMap>,
    target_exists: impl Fn(&str) -> bool,
) -> Result<AppliedDecision, Error> {
    apply_selected_actions(&decision.actions, object, value_maps, target_exists)
}

fn apply_selected_actions(
    actions: &[SelectedAction<'_>],
    mut object: TransformObject,
    value_maps: &BTreeMap<String, ValueMap>,
    target_exists: impl Fn(&str) -> bool,
) -> Result<AppliedDecision, Error> {
    if actions.is_empty() {
        if !target_exists(&object.identity) {
            return Err(Error::Unresolved {
                identity: object.identity,
            });
        }
        return Ok(AppliedDecision {
            object: Some(object),
            disposition: Disposition::Unchanged,
            responsible_rules: Vec::new(),
            map_outcomes: Vec::new(),
        });
    }

    let mut disposition = Disposition::Transformed;
    let mut responsible_rules = Vec::new();
    let mut map_outcomes = Vec::new();
    for selected in actions {
        let rule_id = selected.rule_id;
        let action = selected.action;
        responsible_rules.push(rule_id.to_owned());
        match action {
            ObjectAction::Transform {
                target,
                numeric,
                patches,
                ..
            } => {
                if let Some(target) = target {
                    object.identity.clone_from(target);
                }
                apply_numeric(&mut object.numeric, numeric.as_ref());
                if !patches.is_empty() {
                    let nbt = object.nbt.as_mut().ok_or_else(|| Error::MissingNbt {
                        rule_id: rule_id.to_owned(),
                    })?;
                    let outcomes =
                        apply_patches_with_maps(nbt, patches, value_maps).map_err(|source| {
                            Error::Patch {
                                rule_id: rule_id.to_owned(),
                                source: Box::new(source),
                            }
                        })?;
                    map_outcomes.extend(outcomes.into_iter().map(|mapping| AppliedMapOutcome {
                        rule_id: rule_id.to_owned(),
                        mapping,
                    }));
                }
            }
            ObjectAction::Delete => {
                return Ok(loss_result(Disposition::Deleted, responsible_rules));
            }
            ObjectAction::ReplaceWithAir => {
                object.identity = "minecraft:air".into();
                object.numeric = 0;
                object.nbt = None;
                disposition = Disposition::ReplacedWithAir;
            }
            ObjectAction::DropItem => {
                return Ok(loss_result(Disposition::Dropped, responsible_rules));
            }
            ObjectAction::DiscardNbt => {
                object.nbt = None;
                disposition = Disposition::Transformed;
            }
            ObjectAction::Substitute { target } => {
                object.identity.clone_from(target);
                disposition = Disposition::Transformed;
            }
        }
    }
    if !target_exists(&object.identity) {
        return Err(Error::Unresolved {
            identity: object.identity,
        });
    }
    Ok(AppliedDecision {
        object: Some(object),
        disposition,
        responsible_rules,
        map_outcomes,
    })
}

fn apply_numeric(value: &mut i64, transform: Option<&NumericTransform>) {
    match transform {
        Some(NumericTransform::Set { value: replacement }) => *value = *replacement,
        Some(NumericTransform::Clamp { min, max }) => *value = (*value).clamp(*min, *max),
        None => {}
    }
}

fn loss_result(disposition: Disposition, responsible_rules: Vec<String>) -> AppliedDecision {
    AppliedDecision {
        object: None,
        disposition,
        responsible_rules,
        map_outcomes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::rules::ObjectAction;

    fn object() -> TransformObject {
        TransformObject {
            identity: "mod:old".into(),
            numeric: 12,
            nbt: Some(Value::Compound(BTreeMap::default())),
        }
    }

    #[test]
    fn unresolved_without_explicit_policy_fails_by_default() {
        assert!(matches!(
            apply_decision(
                &Decision {
                    actions: vec![],
                    trace: vec![]
                },
                object(),
                |_| false
            ),
            Err(Error::Unresolved { .. })
        ));
    }

    #[test]
    fn explicit_loss_and_clamp_policies_are_auditable() {
        let decision = Decision {
            actions: vec![(
                "clamp".into(),
                ObjectAction::Transform {
                    target: Some("mod:new".into()),
                    numeric: Some(NumericTransform::Clamp { min: 0, max: 7 }),
                    patches: vec![],
                    nested_items: vec![],
                },
            )],
            trace: vec![],
        };
        let applied = apply_decision(&decision, object(), |name| name == "mod:new").unwrap();
        assert_eq!(applied.object.unwrap().numeric, 7);
        assert_eq!(applied.responsible_rules, ["clamp"]);

        let deleted = apply_decision(
            &Decision {
                actions: vec![("delete".into(), ObjectAction::Delete)],
                trace: vec![],
            },
            object(),
            |_| false,
        )
        .unwrap();
        assert_eq!(deleted.disposition, Disposition::Deleted);
        assert_eq!(deleted.responsible_rules, ["delete"]);
    }

    #[test]
    fn air_drop_discard_and_substitution_require_explicit_actions() {
        let cases = [
            (ObjectAction::ReplaceWithAir, Disposition::ReplacedWithAir),
            (ObjectAction::DropItem, Disposition::Dropped),
            (ObjectAction::DiscardNbt, Disposition::Transformed),
            (
                ObjectAction::Substitute {
                    target: "mod:new".into(),
                },
                Disposition::Transformed,
            ),
        ];
        for (action, expected) in cases {
            let applied = apply_decision(
                &Decision {
                    actions: vec![("policy".into(), action)],
                    trace: vec![],
                },
                object(),
                |name| matches!(name, "minecraft:air" | "mod:new" | "mod:old"),
            )
            .unwrap();
            assert_eq!(applied.disposition, expected);
        }
    }
}
