//! Lossless values exchanged with transformation templates.

use serde::{Deserialize, Serialize};

use crate::rules::{TypedNbt, ValueMap};

/// Resource limits applied independently to every root template rendering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemplateLimits {
    pub fuel: u64,
    pub recursion: usize,
    pub output_bytes: usize,
}

impl Default for TemplateLimits {
    fn default() -> Self {
        Self {
            fuel: 100_000,
            recursion: 64,
            output_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("template `{name}` failed to compile: {source}")]
    Compile {
        name: String,
        #[source]
        source: minijinja::Error,
    },
    #[error("template `{name}` failed to render: {source}")]
    Render {
        name: String,
        #[source]
        source: minijinja::Error,
    },
    #[error("template `{name}` produced invalid typed output: {source}")]
    Decode {
        name: String,
        #[source]
        source: serde_json::Error,
    },
}

/// A precompiled, immutable collection of inline templates.
pub struct TemplateRuntime {
    environment: minijinja::Environment<'static>,
    output_bytes: usize,
}

impl std::fmt::Debug for TemplateRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TemplateRuntime")
            .field("output_bytes", &self.output_bytes)
            .finish_non_exhaustive()
    }
}

impl TemplateRuntime {
    /// Compile a complete set of `(name, source)` pairs into an isolated environment.
    ///
    /// # Errors
    /// Returns a contextual syntax error when any inline template cannot compile.
    pub fn compile(
        templates: impl IntoIterator<Item = (String, String)>,
        limits: TemplateLimits,
    ) -> Result<Self, TemplateError> {
        Self::compile_with_value_maps(templates, limits, std::collections::BTreeMap::new())
    }

    /// Compile templates with the complete immutable typed value-map registry.
    ///
    /// # Errors
    /// Returns a contextual syntax error when any inline template cannot compile.
    pub fn compile_with_value_maps(
        templates: impl IntoIterator<Item = (String, String)>,
        limits: TemplateLimits,
        value_maps: std::collections::BTreeMap<String, ValueMap>,
    ) -> Result<Self, TemplateError> {
        let mut environment = minijinja::Environment::new();
        environment.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        environment.set_fuel(Some(limits.fuel));
        environment.set_recursion_limit(limits.recursion);
        let value_maps = std::sync::Arc::new(value_maps);
        environment.add_function(
            "value_map",
            move |state: &minijinja::State<'_, '_>, map_id: String, input: minijinja::Value| {
                let input_json = serde_json::to_value(&input).map_err(template_call_error)?;
                let input: TypedNbt =
                    serde_json::from_value(input_json).map_err(template_call_error)?;
                let outcome = lookup_value_map(&value_maps, &map_id, input);
                if let Some(callbacks) = callbacks_optional(state) {
                    callbacks.record_value_map(outcome.clone())?;
                }
                match outcome {
                    ValueMapCallOutcome::Mapped { destination, .. } => {
                        Ok(minijinja::Value::from_serialize(destination))
                    }
                    failure => Err(minijinja::Error::new(
                        minijinja::ErrorKind::InvalidOperation,
                        failure.to_string(),
                    )),
                }
            },
        );
        environment.add_function(
            "transform_item",
            |state: &minijinja::State<'_, '_>, item: minijinja::Value| {
                callbacks(state)?.transform_item(item)
            },
        );
        environment.add_function(
            "transform_items",
            |state: &minijinja::State<'_, '_>, items: minijinja::Value| {
                let callbacks = callbacks(state)?;
                let iterator = items.try_iter().map_err(template_call_error)?;
                let mut transformed = Vec::new();
                for item in iterator {
                    let value = callbacks.transform_item(item)?;
                    if !value.is_none() {
                        transformed.push(value);
                    }
                }
                Ok(minijinja::Value::from_serialize(transformed))
            },
        );
        environment.add_function(
            "map_item_id",
            |state: &minijinja::State<'_, '_>, id: i32| callbacks(state)?.map_item_id(id),
        );
        for (name, source) in templates {
            environment
                .add_template_owned(name.clone(), source)
                .map_err(|source| TemplateError::Compile { name, source })?;
        }
        Ok(Self {
            environment,
            output_bytes: limits.output_bytes,
        })
    }

    /// Render and decode exactly one precompiled template.
    ///
    /// # Errors
    /// Returns a contextual render, resource-limit, or typed-decode error.
    pub fn render<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        name: &str,
        context: T,
    ) -> Result<R, TemplateError> {
        self.render_value(name, minijinja::Value::from_serialize(context))
    }

    /// Render with bounded project callbacks available to the template.
    ///
    /// # Errors
    /// Returns a contextual render, callback, resource-limit, or typed-decode error.
    pub fn render_with_callbacks<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        name: &str,
        original: T,
        callbacks: TemplateCallbacks,
    ) -> Result<R, TemplateError> {
        self.render_value(
            name,
            minijinja::context! {
                original => original,
                __callbacks => minijinja::Value::from_object(callbacks),
            },
        )
    }

    fn render_value<R: for<'de> Deserialize<'de>>(
        &self,
        name: &str,
        context: minijinja::Value,
    ) -> Result<R, TemplateError> {
        let template =
            self.environment
                .get_template(name)
                .map_err(|source| TemplateError::Render {
                    name: name.to_owned(),
                    source,
                })?;
        let mut output = LimitedOutput::new(self.output_bytes);
        template
            .render_captured_to(context, &mut output)
            .map_err(|source| TemplateError::Render {
                name: name.to_owned(),
                source,
            })?;
        serde_json::from_slice(&output.bytes).map_err(|source| TemplateError::Decode {
            name: name.to_owned(),
            source,
        })
    }
}

#[derive(Clone)]
pub struct TemplateCallbacks {
    transform: std::sync::Arc<
        dyn Fn(minijinja::Value) -> Result<minijinja::Value, minijinja::Error> + Send + Sync,
    >,
    map_id: std::sync::Arc<dyn Fn(i32) -> Result<minijinja::Value, minijinja::Error> + Send + Sync>,
    record_map:
        std::sync::Arc<dyn Fn(ValueMapCallOutcome) -> Result<(), minijinja::Error> + Send + Sync>,
}

impl TemplateCallbacks {
    pub fn new(
        transform: impl Fn(minijinja::Value) -> Result<minijinja::Value, minijinja::Error>
            + Send
            + Sync
            + 'static,
        map_id: impl Fn(i32) -> Result<minijinja::Value, minijinja::Error> + Send + Sync + 'static,
        record_map: impl Fn(ValueMapCallOutcome) -> Result<(), minijinja::Error> + Send + Sync + 'static,
    ) -> Self {
        Self {
            transform: std::sync::Arc::new(transform),
            map_id: std::sync::Arc::new(map_id),
            record_map: std::sync::Arc::new(record_map),
        }
    }

    fn transform_item(&self, item: minijinja::Value) -> Result<minijinja::Value, minijinja::Error> {
        (self.transform)(item)
    }

    fn map_item_id(&self, id: i32) -> Result<minijinja::Value, minijinja::Error> {
        (self.map_id)(id)
    }

    fn record_value_map(&self, outcome: ValueMapCallOutcome) -> Result<(), minijinja::Error> {
        (self.record_map)(outcome)
    }
}

impl std::fmt::Debug for TemplateCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TemplateCallbacks")
    }
}

impl std::fmt::Display for TemplateCallbacks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("template callbacks")
    }
}

impl minijinja::value::Object for TemplateCallbacks {}

fn callbacks(
    state: &minijinja::State<'_, '_>,
) -> Result<std::sync::Arc<TemplateCallbacks>, minijinja::Error> {
    callbacks_optional(state).ok_or_else(|| {
        minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "template item functions are unavailable in this execution context",
        )
    })
}

fn callbacks_optional(
    state: &minijinja::State<'_, '_>,
) -> Option<std::sync::Arc<TemplateCallbacks>> {
    state
        .lookup("__callbacks")
        .and_then(|value| value.downcast_object::<TemplateCallbacks>())
}

fn template_call_error(error: impl std::fmt::Display) -> minijinja::Error {
    minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, error.to_string())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ValueMapCallOutcome {
    Mapped {
        map_id: String,
        source: TypedNbt,
        destination: TypedNbt,
        coerced: bool,
    },
    UnknownMap {
        map_id: String,
        source: TypedNbt,
    },
    Unmapped {
        map_id: String,
        source: TypedNbt,
    },
}

impl std::fmt::Display for ValueMapCallOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mapped { map_id, .. } => write!(f, "value map `{map_id}` matched"),
            Self::UnknownMap { map_id, source } => {
                write!(f, "unknown value map `{map_id}` for {source:?}")
            }
            Self::Unmapped { map_id, source } => {
                write!(f, "value map `{map_id}` has no entry for {source:?}")
            }
        }
    }
}

#[must_use]
pub fn lookup_value_map(
    maps: &std::collections::BTreeMap<String, ValueMap>,
    map_id: &str,
    source: TypedNbt,
) -> ValueMapCallOutcome {
    let Some(map) = maps.get(map_id) else {
        return ValueMapCallOutcome::UnknownMap {
            map_id: map_id.into(),
            source,
        };
    };
    let source_value = crate::nbt::Value::from(source.clone());
    let Some(entry) = map.entries.iter().find(|entry| {
        entry.from == source
            || (map.coerce_numeric
                && crate::rules::numeric_equal(
                    &source_value,
                    &crate::nbt::Value::from(entry.from.clone()),
                ))
    }) else {
        return ValueMapCallOutcome::Unmapped {
            map_id: map_id.into(),
            source,
        };
    };
    ValueMapCallOutcome::Mapped {
        map_id: map_id.into(),
        source: source.clone(),
        destination: entry.to.clone(),
        coerced: entry.from != source,
    }
}

struct LimitedOutput {
    bytes: Vec<u8>,
    limit: usize,
}

impl LimitedOutput {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit.min(8192)),
            limit,
        }
    }
}

impl std::io::Write for LimitedOutput {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        if input.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("template output limit exceeded"));
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Produce a stable globally unique inline-template name from canonical graph order.
#[must_use]
pub fn template_name(document: &str, rule_id: &str, ordinal: usize) -> String {
    format!("{document}::{ordinal}::{rule_id}")
}

/// The immutable source block and its coordinate-owned block entity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockContext {
    pub original: BlockOriginal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockOriginal {
    pub name: String,
    pub numeric_id: i32,
    pub metadata: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbt: Option<TypedNbt>,
    pub block_entity: Option<TypedNbt>,
}

/// The immutable source item stack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemContext {
    pub original: ItemOriginal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemOriginal {
    pub name: String,
    pub numeric_id: i32,
    pub count: i8,
    pub damage: i16,
    pub nbt: TypedNbt,
}

/// The immutable source standalone entity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityContext {
    pub original: EntityOriginal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntityOriginal {
    pub name: String,
    pub nbt: TypedNbt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetBlock {
    pub name: String,
    pub metadata: u8,
}

/// Complete coordinated result of a block template.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
pub enum BlockResult {
    Unchanged,
    Transform {
        block: TargetBlock,
        block_entity: Option<TypedNbt>,
    },
    ReplaceWithAir,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetItem {
    pub name: String,
    pub count: i8,
    pub damage: i16,
    pub nbt: TypedNbt,
}

/// Complete result of an item template.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
pub enum ItemResult {
    Unchanged,
    Transform { item: TargetItem },
    Drop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetEntity {
    pub name: String,
    pub nbt: TypedNbt,
}

/// Complete result of an entity template.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
pub enum EntityResult {
    Unchanged,
    Transform { entity: TargetEntity },
    Delete,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::rules::{NbtType, TypedList, ValueMapEntry};

    fn all_nbt_tags() -> TypedNbt {
        TypedNbt::Compound(BTreeMap::from([
            ("byte".into(), TypedNbt::Byte(-1)),
            ("short".into(), TypedNbt::Short(-2)),
            ("int".into(), TypedNbt::Int(-3)),
            ("long".into(), TypedNbt::Long(-4)),
            ("float".into(), TypedNbt::Float(1.25)),
            ("double".into(), TypedNbt::Double(2.5)),
            ("byte_array".into(), TypedNbt::ByteArray(vec![-1, 0, 1])),
            ("string".into(), TypedNbt::String("value".into())),
            (
                "list".into(),
                TypedNbt::List(TypedList {
                    element_type: NbtType::Long,
                    values: vec![TypedNbt::Long(7)],
                }),
            ),
            (
                "empty_list".into(),
                TypedNbt::List(TypedList {
                    element_type: NbtType::Compound,
                    values: vec![],
                }),
            ),
            ("compound".into(), TypedNbt::Compound(BTreeMap::new())),
            (
                "int_array".into(),
                TypedNbt::IntArray(vec![i32::MIN, i32::MAX]),
            ),
            (
                "long_array".into(),
                TypedNbt::LongArray(vec![i64::MIN, i64::MAX]),
            ),
        ]))
    }

    fn json_round_trip<T>(value: &T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).unwrap();
        assert_eq!(&serde_json::from_str::<T>(&json).unwrap(), value);
    }

    #[test]
    fn contexts_round_trip_every_nbt_tag_and_optional_block_entities() {
        let nbt = all_nbt_tags();
        for block_entity in [None, Some(nbt.clone())] {
            json_round_trip(&BlockContext {
                original: BlockOriginal {
                    name: "source:block".into(),
                    numeric_id: 100,
                    metadata: 3,
                    nbt: Some(nbt.clone()),
                    block_entity,
                },
            });
        }
        json_round_trip(&ItemContext {
            original: ItemOriginal {
                name: "source:item".into(),
                numeric_id: 101,
                count: 2,
                damage: 4,
                nbt: nbt.clone(),
            },
        });
        json_round_trip(&EntityContext {
            original: EntityOriginal {
                name: "source:entity".into(),
                nbt,
            },
        });
    }

    #[test]
    fn result_envelopes_round_trip_all_dispositions() {
        let nbt = all_nbt_tags();
        for result in [
            BlockResult::Unchanged,
            BlockResult::Transform {
                block: TargetBlock {
                    name: "target:block".into(),
                    metadata: 9,
                },
                block_entity: None,
            },
            BlockResult::Transform {
                block: TargetBlock {
                    name: "target:block".into(),
                    metadata: 9,
                },
                block_entity: Some(nbt.clone()),
            },
            BlockResult::ReplaceWithAir,
        ] {
            json_round_trip(&result);
        }
        for result in [
            ItemResult::Unchanged,
            ItemResult::Transform {
                item: TargetItem {
                    name: "target:item".into(),
                    count: 2,
                    damage: 4,
                    nbt: nbt.clone(),
                },
            },
            ItemResult::Drop,
        ] {
            json_round_trip(&result);
        }
        for result in [
            EntityResult::Unchanged,
            EntityResult::Transform {
                entity: TargetEntity {
                    name: "target:entity".into(),
                    nbt: nbt.clone(),
                },
            },
            EntityResult::Delete,
        ] {
            json_round_trip(&result);
        }
    }

    #[test]
    fn runtime_rejects_syntax_errors_and_undefined_values() {
        let limits = TemplateLimits::default();
        assert!(matches!(
            TemplateRuntime::compile([("broken".into(), "{% if".into())], limits),
            Err(TemplateError::Compile { .. })
        ));
        let runtime = TemplateRuntime::compile(
            [("strict".into(), r"{{ missing.field|tojson }}".into())],
            limits,
        )
        .unwrap();
        assert!(matches!(
            runtime.render::<_, serde_json::Value>("strict", ()),
            Err(TemplateError::Render { .. })
        ));
    }

    #[test]
    fn runtime_enforces_computation_recursion_and_output_limits() {
        let fuel_runtime = TemplateRuntime::compile(
            [(
                "fuel".into(),
                r"{% for value in values %}{{ value }}{% endfor %}".into(),
            )],
            TemplateLimits {
                fuel: 10,
                ..TemplateLimits::default()
            },
        )
        .unwrap();
        assert!(matches!(
            fuel_runtime.render::<_, serde_json::Value>(
                "fuel",
                serde_json::json!({"values": vec![0; 100]})
            ),
            Err(TemplateError::Render { .. })
        ));

        let recursion_runtime = TemplateRuntime::compile(
            [(
                "recursion".into(),
                r"{% macro recurse() %}{{ recurse() }}{% endmacro %}{{ recurse() }}".into(),
            )],
            TemplateLimits {
                recursion: 10,
                ..TemplateLimits::default()
            },
        )
        .unwrap();
        assert!(matches!(
            recursion_runtime.render::<_, serde_json::Value>("recursion", ()),
            Err(TemplateError::Render { .. })
        ));

        let output_runtime = TemplateRuntime::compile(
            [("output".into(), r"{{ value|tojson }}".into())],
            TemplateLimits {
                output_bytes: 8,
                ..TemplateLimits::default()
            },
        )
        .unwrap();
        assert!(matches!(
            output_runtime.render::<_, serde_json::Value>(
                "output",
                serde_json::json!({"value": "a long value"})
            ),
            Err(TemplateError::Render { .. })
        ));
    }

    #[test]
    fn runtime_is_deterministic_and_names_include_global_order() {
        let name = template_name("rules/imported.yaml", "machine", 7);
        assert_eq!(name, "rules/imported.yaml::7::machine");
        let runtime = TemplateRuntime::compile(
            [(name.clone(), r"{{ original|tojson }}".into())],
            TemplateLimits::default(),
        )
        .unwrap();
        let context = serde_json::json!({"original": {"b": 2, "a": 1}});
        let first: serde_json::Value = runtime.render(&name, &context).unwrap();
        let second: serde_json::Value = runtime.render(&name, &context).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
    }

    #[test]
    fn value_map_function_preserves_destination_type_and_reports_outcomes() {
        let maps = std::collections::BTreeMap::from([(
            "direction".into(),
            ValueMap {
                id: "direction".into(),
                coerce_numeric: true,
                entries: vec![ValueMapEntry {
                    from: TypedNbt::Byte(1),
                    to: TypedNbt::Long(99),
                }],
            },
        )]);
        assert_eq!(
            lookup_value_map(&maps, "direction", TypedNbt::Int(1)),
            ValueMapCallOutcome::Mapped {
                map_id: "direction".into(),
                source: TypedNbt::Int(1),
                destination: TypedNbt::Long(99),
                coerced: true,
            }
        );
        assert!(matches!(
            lookup_value_map(&maps, "missing", TypedNbt::Byte(1)),
            ValueMapCallOutcome::UnknownMap { .. }
        ));
        assert!(matches!(
            lookup_value_map(&maps, "direction", TypedNbt::Byte(2)),
            ValueMapCallOutcome::Unmapped { .. }
        ));

        let runtime = TemplateRuntime::compile_with_value_maps(
            [(
                "map".into(),
                r#"{{ value_map("direction", original)|tojson }}"#.into(),
            )],
            TemplateLimits::default(),
            maps,
        )
        .unwrap();
        let result: TypedNbt = runtime
            .render(
                "map",
                serde_json::json!({"original": {"type": "int", "value": 1}}),
            )
            .unwrap();
        assert_eq!(result, TypedNbt::Long(99));
    }
}
