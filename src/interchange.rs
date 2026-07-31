//! Strict Shape IR Interchange 0.1 decoding and stable encoding.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde_json::{Map as JsonMap, Value as JsonValue};

use crate::shape_ir::{
    self, AggregateDefinition, AggregateFunction, BinaryOperation, CaseArm, CollectionKind,
    Direction, Expression, ExpressionKind, Field, FieldId, Graph, GroupingKey, JoinType,
    LiteralValue, Node, NodeId, NodeKind, NullPlacement, OrderingItem, ProjectEntry,
    RankingFunction, SetOperation, SetQuantifier, UnaryOperation, ValidationError,
    WindowDefinition,
};
use crate::{RelationBinding, ScalarType, TypeDescriptor};

/// The Shape IR Interchange version implemented by this module.
pub const VERSION: &str = "0.1";

/// A stage-one JSON or stage-two structural mapping failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterchangeError {
    pub path: String,
    pub message: String,
}

impl InterchangeError {
    fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for InterchangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            formatter.write_str(&self.message)
        } else {
            write!(formatter, "{}: {}", self.path, self.message)
        }
    }
}

impl std::error::Error for InterchangeError {}

/// A producer-side failure that prevents a conforming document from being emitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeError {
    Validation(ValidationError),
    InvalidIdentifier(String),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => write!(formatter, "invalid Shape IR graph: {error}"),
            Self::InvalidIdentifier(identifier) => {
                write!(
                    formatter,
                    "identity `{identifier}` is not interchange-compatible"
                )
            }
        }
    }
}

impl std::error::Error for EncodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::InvalidIdentifier(_) => None,
        }
    }
}

impl From<ValidationError> for EncodeError {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error)
    }
}

/// Decodes exactly one Shape IR Interchange 0.1 JSON document.
///
/// This function performs only the normative `interchange` stages. Call
/// [`Graph::validate`] on the returned graph before evaluation.
pub fn decode(bytes: &[u8]) -> Result<Graph, InterchangeError> {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(InterchangeError::new("$", "a UTF-8 BOM is not permitted"));
    }

    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValue::deserialize(&mut deserializer)
        .map_err(|error| InterchangeError::new("$", format!("invalid JSON: {error}")))?;
    deserializer
        .end()
        .map_err(|error| InterchangeError::new("$", format!("invalid JSON: {error}")))?;
    decode_document(value)
}

/// Validates and encodes a graph using stable, human-readable JSON formatting.
pub fn encode(graph: &Graph) -> Result<String, EncodeError> {
    graph.validate()?;
    ensure_encodable_identities(graph)?;
    let mut encoded = serde_json::to_string_pretty(&encode_graph(graph))
        .expect("serializing a JSON value cannot fail");
    encoded.push('\n');
    Ok(encoded)
}

#[derive(Clone, Debug)]
enum StrictValue {
    Null,
    Bool(bool),
    Number,
    String(String),
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue::Bool(value))
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue::Number)
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue::Number)
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(StrictValue::Number)
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(StrictValue::String(value.into()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue::String(value))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element()? {
            values.push(value);
        }
        Ok(StrictValue::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some((key, value)) = map.next_entry::<String, StrictValue>()? {
            if values.insert(key.clone(), value).is_some() {
                return Err(de::Error::custom(format!(
                    "duplicate object member `{key}`"
                )));
            }
        }
        Ok(StrictValue::Object(values))
    }
}

struct Record {
    fields: BTreeMap<String, StrictValue>,
    path: String,
}

impl Record {
    fn new(
        value: StrictValue,
        path: impl Into<String>,
        allowed: &[&str],
        metadata_allowed: bool,
    ) -> Result<Self, InterchangeError> {
        let path = path.into();
        let mut fields = expect_object(value, &path)?;
        if let Some(metadata) = fields.remove("metadata") {
            if !metadata_allowed {
                return Err(InterchangeError::new(
                    member_path(&path, "metadata"),
                    "unknown member",
                ));
            }
            expect_object(metadata, &member_path(&path, "metadata"))?;
        }
        if let Some(member) = fields
            .keys()
            .find(|member| !allowed.contains(&member.as_str()))
        {
            return Err(InterchangeError::new(
                member_path(&path, member),
                "unknown member",
            ));
        }
        Ok(Self { fields, path })
    }

    fn required(&mut self, member: &str) -> Result<StrictValue, InterchangeError> {
        self.fields.remove(member).ok_or_else(|| {
            InterchangeError::new(member_path(&self.path, member), "missing required member")
        })
    }

    fn optional(&mut self, member: &str) -> Option<StrictValue> {
        self.fields.remove(member)
    }

    fn finish(self) {
        debug_assert!(self.fields.is_empty());
    }
}

fn decode_document(value: StrictValue) -> Result<Graph, InterchangeError> {
    let mut record = Record::new(
        value,
        "$",
        &["interchange_version", "shape_ir_version", "root", "nodes"],
        true,
    )?;
    require_version(
        record.required("interchange_version")?,
        "$.interchange_version",
        VERSION,
        "Shape IR Interchange",
    )?;
    require_version(
        record.required("shape_ir_version")?,
        "$.shape_ir_version",
        shape_ir::VERSION,
        "Shape IR",
    )?;
    let root = parse_node_id(record.required("root")?, "$.root")?;
    let node_values = expect_array(record.required("nodes")?, "$.nodes")?;
    if node_values.is_empty() {
        return Err(InterchangeError::new("$.nodes", "array must be nonempty"));
    }
    let nodes = node_values
        .into_iter()
        .enumerate()
        .map(|(index, value)| parse_node(value, &index_path("$.nodes", index)))
        .collect::<Result<Vec<_>, _>>()?;
    record.finish();
    Ok(Graph {
        version: shape_ir::VERSION.into(),
        root,
        nodes,
    })
}

fn require_version(
    value: StrictValue,
    path: &str,
    supported: &str,
    name: &str,
) -> Result<(), InterchangeError> {
    let value = expect_string(value, path)?;
    if value == supported {
        Ok(())
    } else {
        Err(InterchangeError::new(
            path,
            format!("unsupported {name} version `{value}`"),
        ))
    }
}

fn expect_object(
    value: StrictValue,
    path: &str,
) -> Result<BTreeMap<String, StrictValue>, InterchangeError> {
    match value {
        StrictValue::Object(value) => Ok(value),
        _ => Err(wrong_type(path, "object")),
    }
}

fn expect_array(value: StrictValue, path: &str) -> Result<Vec<StrictValue>, InterchangeError> {
    match value {
        StrictValue::Array(value) => Ok(value),
        _ => Err(wrong_type(path, "array")),
    }
}

fn expect_string(value: StrictValue, path: &str) -> Result<String, InterchangeError> {
    match value {
        StrictValue::String(value) => Ok(value),
        _ => Err(wrong_type(path, "string")),
    }
}

fn expect_bool(value: StrictValue, path: &str) -> Result<bool, InterchangeError> {
    match value {
        StrictValue::Bool(value) => Ok(value),
        _ => Err(wrong_type(path, "Boolean")),
    }
}

fn wrong_type(path: &str, expected: &str) -> InterchangeError {
    InterchangeError::new(path, format!("expected JSON {expected}"))
}

fn member_path(path: &str, member: &str) -> String {
    format!("{path}.{member}")
}

fn index_path(path: &str, index: usize) -> String {
    format!("{path}[{index}]")
}

fn parse_identifier(value: StrictValue, path: &str) -> Result<String, InterchangeError> {
    let value = expect_string(value, path)?;
    if is_identifier(&value) {
        Ok(value)
    } else {
        Err(InterchangeError::new(path, "invalid identifier"))
    }
}

fn is_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        && bytes.all(|byte| {
            matches!(
                byte,
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b':'
            )
        })
}

fn parse_node_id(value: StrictValue, path: &str) -> Result<NodeId, InterchangeError> {
    parse_identifier(value, path).map(NodeId::new)
}

fn parse_field_id(value: StrictValue, path: &str) -> Result<FieldId, InterchangeError> {
    parse_identifier(value, path).map(FieldId::new)
}

fn parse_i64(value: StrictValue, path: &str) -> Result<i64, InterchangeError> {
    let spelling = expect_string(value, path)?;
    let parsed = spelling
        .parse::<i64>()
        .map_err(|_| InterchangeError::new(path, "invalid canonical INT64 spelling"))?;
    if parsed.to_string() != spelling {
        return Err(InterchangeError::new(
            path,
            "invalid canonical INT64 spelling",
        ));
    }
    Ok(parsed)
}

fn parse_scalar(value: StrictValue, path: &str) -> Result<ScalarType, InterchangeError> {
    match expect_string(value, path)?.as_str() {
        "boolean" => Ok(ScalarType::Boolean),
        "int64" => Ok(ScalarType::Int64),
        "text" => Ok(ScalarType::Text),
        _ => Err(InterchangeError::new(path, "unknown scalar type")),
    }
}

fn parse_type(value: StrictValue, path: &str) -> Result<TypeDescriptor, InterchangeError> {
    let mut record = Record::new(value, path, &["scalar", "nullable"], false)?;
    let scalar = parse_scalar(record.required("scalar")?, &member_path(path, "scalar"))?;
    let nullable = expect_bool(record.required("nullable")?, &member_path(path, "nullable"))?;
    record.finish();
    Ok(TypeDescriptor { scalar, nullable })
}

fn parse_field(value: StrictValue, path: &str) -> Result<Field, InterchangeError> {
    let mut record = Record::new(value, path, &["id", "name", "type"], true)?;
    let id = parse_field_id(record.required("id")?, &member_path(path, "id"))?;
    let name = expect_string(record.required("name")?, &member_path(path, "name"))?;
    let descriptor = parse_type(record.required("type")?, &member_path(path, "type"))?;
    record.finish();
    Ok(Field {
        id,
        name,
        descriptor,
    })
}

fn parse_schema(value: StrictValue, path: &str) -> Result<Vec<Field>, InterchangeError> {
    expect_array(value, path)?
        .into_iter()
        .enumerate()
        .map(|(index, value)| parse_field(value, &index_path(path, index)))
        .collect()
}

fn take_discriminated_object(
    value: StrictValue,
    path: &str,
) -> Result<(String, BTreeMap<String, StrictValue>), InterchangeError> {
    let fields = expect_object(value, path)?;
    let kind_path = member_path(path, "kind");
    let kind = fields
        .get("kind")
        .ok_or_else(|| InterchangeError::new(&kind_path, "missing required member"))?;
    let StrictValue::String(kind) = kind else {
        return Err(wrong_type(&kind_path, "string"));
    };
    Ok((kind.clone(), fields))
}

fn parse_node(value: StrictValue, path: &str) -> Result<Node, InterchangeError> {
    const COMMON: &[&str] = &["id", "kind", "output_schema", "collection"];
    let (kind, fields) = take_discriminated_object(value, path)?;
    let specific: &[&str] = match kind.as_str() {
        "input" => &["binding"],
        "empty" => &[],
        "project" => &["input", "entries"],
        "filter" => &["input", "predicate"],
        "join" => &["left", "right", "join_type", "condition"],
        "aggregate" => &["input", "grouping_keys", "aggregates"],
        "window" => &["input", "definitions"],
        "distinct" => &["input"],
        "set" => &["left", "right", "operation", "quantifier"],
        "order" => &["input", "items"],
        "slice" => &["input", "offset", "limit"],
        "forget_order" => &["input"],
        _ => {
            return Err(InterchangeError::new(
                member_path(path, "kind"),
                "unknown node kind",
            ));
        }
    };
    let allowed = COMMON
        .iter()
        .copied()
        .chain(specific.iter().copied())
        .collect::<Vec<_>>();
    let mut record = Record::new(StrictValue::Object(fields), path, &allowed, true)?;
    let id = parse_node_id(record.required("id")?, &member_path(path, "id"))?;
    let decoded_kind = expect_string(record.required("kind")?, &member_path(path, "kind"))?;
    debug_assert_eq!(decoded_kind, kind);
    let output_schema = parse_schema(
        record.required("output_schema")?,
        &member_path(path, "output_schema"),
    )?;
    let collection = parse_collection(
        record.required("collection")?,
        &member_path(path, "collection"),
    )?;

    let node_kind = match kind.as_str() {
        "input" => {
            let binding_path = member_path(path, "binding");
            let binding = expect_string(record.required("binding")?, &binding_path)?;
            if binding.is_empty() {
                return Err(InterchangeError::new(
                    binding_path,
                    "binding must be nonempty",
                ));
            }
            NodeKind::Input {
                binding: RelationBinding::new(binding),
            }
        }
        "empty" => NodeKind::Empty,
        "project" => NodeKind::Project {
            input: parse_node_id(record.required("input")?, &member_path(path, "input"))?,
            entries: parse_project_entries(
                record.required("entries")?,
                &member_path(path, "entries"),
            )?,
        },
        "filter" => NodeKind::Filter {
            input: parse_node_id(record.required("input")?, &member_path(path, "input"))?,
            predicate: parse_expression(
                record.required("predicate")?,
                &member_path(path, "predicate"),
            )?,
        },
        "join" => parse_join_node(&mut record, path)?,
        "aggregate" => NodeKind::Aggregate {
            input: parse_node_id(record.required("input")?, &member_path(path, "input"))?,
            grouping_keys: parse_grouping_keys(
                record.required("grouping_keys")?,
                &member_path(path, "grouping_keys"),
            )?,
            aggregates: parse_aggregates(
                record.required("aggregates")?,
                &member_path(path, "aggregates"),
            )?,
        },
        "window" => {
            let definitions_path = member_path(path, "definitions");
            let definitions =
                parse_window_definitions(record.required("definitions")?, &definitions_path)?;
            if definitions.is_empty() {
                return Err(InterchangeError::new(
                    definitions_path,
                    "array must be nonempty",
                ));
            }
            NodeKind::Window {
                input: parse_node_id(record.required("input")?, &member_path(path, "input"))?,
                definitions,
            }
        }
        "distinct" => NodeKind::Distinct {
            input: parse_node_id(record.required("input")?, &member_path(path, "input"))?,
        },
        "set" => NodeKind::Set {
            left: parse_node_id(record.required("left")?, &member_path(path, "left"))?,
            right: parse_node_id(record.required("right")?, &member_path(path, "right"))?,
            operation: parse_set_operation(
                record.required("operation")?,
                &member_path(path, "operation"),
            )?,
            quantifier: parse_set_quantifier(
                record.required("quantifier")?,
                &member_path(path, "quantifier"),
            )?,
        },
        "order" => {
            let items_path = member_path(path, "items");
            let items = parse_ordering_items(record.required("items")?, &items_path)?;
            if items.is_empty() {
                return Err(InterchangeError::new(items_path, "array must be nonempty"));
            }
            NodeKind::Order {
                input: parse_node_id(record.required("input")?, &member_path(path, "input"))?,
                items,
            }
        }
        "slice" => {
            let limit_path = member_path(path, "limit");
            let limit = match record.required("limit")? {
                StrictValue::Null => None,
                value => Some(parse_i64(value, &limit_path)?),
            };
            NodeKind::Slice {
                input: parse_node_id(record.required("input")?, &member_path(path, "input"))?,
                offset: parse_i64(record.required("offset")?, &member_path(path, "offset"))?,
                limit,
            }
        }
        "forget_order" => NodeKind::ForgetOrder {
            input: parse_node_id(record.required("input")?, &member_path(path, "input"))?,
        },
        _ => unreachable!("node kind checked above"),
    };
    record.finish();
    Ok(Node {
        id,
        kind: node_kind,
        output_schema,
        collection,
    })
}

fn parse_collection(value: StrictValue, path: &str) -> Result<CollectionKind, InterchangeError> {
    match expect_string(value, path)?.as_str() {
        "bag" => Ok(CollectionKind::Bag),
        "ordered" => Ok(CollectionKind::Ordered),
        _ => Err(InterchangeError::new(path, "unknown collection kind")),
    }
}

fn parse_join_node(record: &mut Record, path: &str) -> Result<NodeKind, InterchangeError> {
    let join_type_path = member_path(path, "join_type");
    let join_type = match expect_string(record.required("join_type")?, &join_type_path)?.as_str() {
        "cross" => JoinType::Cross,
        "inner" => JoinType::Inner,
        "left" => JoinType::Left,
        "right" => JoinType::Right,
        "full" => JoinType::Full,
        _ => return Err(InterchangeError::new(join_type_path, "unknown join type")),
    };
    let condition_path = member_path(path, "condition");
    let condition = match (join_type, record.optional("condition")) {
        (JoinType::Cross, None) => None,
        (JoinType::Cross, Some(_)) => {
            return Err(InterchangeError::new(
                condition_path,
                "condition is prohibited for a cross join",
            ));
        }
        (_, Some(value)) => Some(parse_expression(value, &condition_path)?),
        (_, None) => {
            return Err(InterchangeError::new(
                condition_path,
                "missing required member",
            ));
        }
    };
    Ok(NodeKind::Join {
        left: parse_node_id(record.required("left")?, &member_path(path, "left"))?,
        right: parse_node_id(record.required("right")?, &member_path(path, "right"))?,
        join_type,
        condition,
    })
}

fn parse_set_operation(value: StrictValue, path: &str) -> Result<SetOperation, InterchangeError> {
    match expect_string(value, path)?.as_str() {
        "union" => Ok(SetOperation::Union),
        "intersect" => Ok(SetOperation::Intersect),
        "except" => Ok(SetOperation::Except),
        _ => Err(InterchangeError::new(path, "unknown set operation")),
    }
}

fn parse_set_quantifier(value: StrictValue, path: &str) -> Result<SetQuantifier, InterchangeError> {
    match expect_string(value, path)?.as_str() {
        "all" => Ok(SetQuantifier::All),
        "distinct" => Ok(SetQuantifier::Distinct),
        _ => Err(InterchangeError::new(path, "unknown set quantifier")),
    }
}

fn parse_project_entries(
    value: StrictValue,
    path: &str,
) -> Result<Vec<ProjectEntry>, InterchangeError> {
    expect_array(value, path)?
        .into_iter()
        .enumerate()
        .map(|(index, value)| parse_project_entry(value, &index_path(path, index)))
        .collect()
}

fn parse_project_entry(value: StrictValue, path: &str) -> Result<ProjectEntry, InterchangeError> {
    let (kind, fields) = take_discriminated_object(value, path)?;
    let allowed: &[&str] = match kind.as_str() {
        "keep" => &["kind", "field"],
        "compute" => &["kind", "output", "expression"],
        _ => {
            return Err(InterchangeError::new(
                member_path(path, "kind"),
                "unknown project-entry kind",
            ));
        }
    };
    let mut record = Record::new(StrictValue::Object(fields), path, allowed, false)?;
    record.required("kind")?;
    let entry = match kind.as_str() {
        "keep" => ProjectEntry::Keep(parse_field_id(
            record.required("field")?,
            &member_path(path, "field"),
        )?),
        "compute" => ProjectEntry::Compute {
            output: parse_field_id(record.required("output")?, &member_path(path, "output"))?,
            expression: parse_expression(
                record.required("expression")?,
                &member_path(path, "expression"),
            )?,
        },
        _ => unreachable!("project-entry kind checked above"),
    };
    record.finish();
    Ok(entry)
}

fn parse_grouping_keys(
    value: StrictValue,
    path: &str,
) -> Result<Vec<GroupingKey>, InterchangeError> {
    expect_array(value, path)?
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let item_path = index_path(path, index);
            let mut record = Record::new(value, &item_path, &["output", "expression"], false)?;
            let key = GroupingKey {
                output: parse_field_id(
                    record.required("output")?,
                    &member_path(&item_path, "output"),
                )?,
                expression: parse_expression(
                    record.required("expression")?,
                    &member_path(&item_path, "expression"),
                )?,
            };
            record.finish();
            Ok(key)
        })
        .collect()
}

fn parse_aggregates(
    value: StrictValue,
    path: &str,
) -> Result<Vec<AggregateDefinition>, InterchangeError> {
    expect_array(value, path)?
        .into_iter()
        .enumerate()
        .map(|(index, value)| parse_aggregate(value, &index_path(path, index)))
        .collect()
}

fn parse_aggregate(
    value: StrictValue,
    path: &str,
) -> Result<AggregateDefinition, InterchangeError> {
    let mut record = Record::new(value, path, &["output", "function", "argument"], false)?;
    let output = parse_field_id(record.required("output")?, &member_path(path, "output"))?;
    let function =
        parse_aggregate_function(record.required("function")?, &member_path(path, "function"))?;
    let argument = parse_aggregate_argument(&mut record, path, function)?;
    record.finish();
    Ok(AggregateDefinition {
        output,
        function,
        argument,
    })
}

fn parse_aggregate_function(
    value: StrictValue,
    path: &str,
) -> Result<AggregateFunction, InterchangeError> {
    match expect_string(value, path)?.as_str() {
        "count_all" => Ok(AggregateFunction::CountAll),
        "count" => Ok(AggregateFunction::Count),
        "sum" => Ok(AggregateFunction::Sum),
        "min" => Ok(AggregateFunction::Min),
        "max" => Ok(AggregateFunction::Max),
        "bool_and" => Ok(AggregateFunction::BoolAnd),
        "bool_or" => Ok(AggregateFunction::BoolOr),
        _ => Err(InterchangeError::new(path, "unknown aggregate function")),
    }
}

fn parse_aggregate_argument(
    record: &mut Record,
    path: &str,
    function: AggregateFunction,
) -> Result<Option<Expression>, InterchangeError> {
    let argument_path = member_path(path, "argument");
    match (function, record.optional("argument")) {
        (AggregateFunction::CountAll, None) => Ok(None),
        (AggregateFunction::CountAll, Some(_)) => Err(InterchangeError::new(
            argument_path,
            "argument is prohibited for count_all",
        )),
        (_, Some(value)) => parse_expression(value, &argument_path).map(Some),
        (_, None) => Err(InterchangeError::new(
            argument_path,
            "missing required member",
        )),
    }
}

fn parse_window_definitions(
    value: StrictValue,
    path: &str,
) -> Result<Vec<WindowDefinition>, InterchangeError> {
    expect_array(value, path)?
        .into_iter()
        .enumerate()
        .map(|(index, value)| parse_window_definition(value, &index_path(path, index)))
        .collect()
}

fn parse_window_definition(
    value: StrictValue,
    path: &str,
) -> Result<WindowDefinition, InterchangeError> {
    let (kind, fields) = take_discriminated_object(value, path)?;
    let allowed: &[&str] = match kind.as_str() {
        "partitioned_aggregate" => &["kind", "output", "function", "argument", "partition_by"],
        "ranking" => &["kind", "output", "function", "partition_by", "order_by"],
        _ => {
            return Err(InterchangeError::new(
                member_path(path, "kind"),
                "unknown window-definition kind",
            ));
        }
    };
    let mut record = Record::new(StrictValue::Object(fields), path, allowed, false)?;
    record.required("kind")?;
    let output = parse_field_id(record.required("output")?, &member_path(path, "output"))?;
    let partition_by = parse_expression_array(
        record.required("partition_by")?,
        &member_path(path, "partition_by"),
    )?;
    let definition = match kind.as_str() {
        "partitioned_aggregate" => {
            let function = parse_aggregate_function(
                record.required("function")?,
                &member_path(path, "function"),
            )?;
            let argument = parse_aggregate_argument(&mut record, path, function)?;
            WindowDefinition::PartitionedAggregate {
                output,
                function,
                argument,
                partition_by,
            }
        }
        "ranking" => {
            let function = parse_ranking_function(
                record.required("function")?,
                &member_path(path, "function"),
            )?;
            let order_path = member_path(path, "order_by");
            let order_by = parse_ordering_items(record.required("order_by")?, &order_path)?;
            if order_by.is_empty() {
                return Err(InterchangeError::new(order_path, "array must be nonempty"));
            }
            WindowDefinition::Ranking {
                output,
                function,
                partition_by,
                order_by,
            }
        }
        _ => unreachable!("window-definition kind checked above"),
    };
    record.finish();
    Ok(definition)
}

fn parse_ranking_function(
    value: StrictValue,
    path: &str,
) -> Result<RankingFunction, InterchangeError> {
    match expect_string(value, path)?.as_str() {
        "row_number" => Ok(RankingFunction::RowNumber),
        "rank" => Ok(RankingFunction::Rank),
        "dense_rank" => Ok(RankingFunction::DenseRank),
        _ => Err(InterchangeError::new(path, "unknown ranking function")),
    }
}

fn parse_ordering_items(
    value: StrictValue,
    path: &str,
) -> Result<Vec<OrderingItem>, InterchangeError> {
    expect_array(value, path)?
        .into_iter()
        .enumerate()
        .map(|(index, value)| parse_ordering_item(value, &index_path(path, index)))
        .collect()
}

fn parse_ordering_item(value: StrictValue, path: &str) -> Result<OrderingItem, InterchangeError> {
    let mut record = Record::new(
        value,
        path,
        &["expression", "direction", "null_placement"],
        false,
    )?;
    let expression = parse_expression(
        record.required("expression")?,
        &member_path(path, "expression"),
    )?;
    let direction_path = member_path(path, "direction");
    let direction = match expect_string(record.required("direction")?, &direction_path)?.as_str() {
        "ascending" => Direction::Ascending,
        "descending" => Direction::Descending,
        _ => return Err(InterchangeError::new(direction_path, "unknown direction")),
    };
    let placement_path = member_path(path, "null_placement");
    let null_placement =
        match expect_string(record.required("null_placement")?, &placement_path)?.as_str() {
            "first" => NullPlacement::First,
            "last" => NullPlacement::Last,
            "not_applicable" => NullPlacement::NotApplicable,
            _ => {
                return Err(InterchangeError::new(
                    placement_path,
                    "unknown null placement",
                ));
            }
        };
    record.finish();
    Ok(OrderingItem {
        expression,
        direction,
        null_placement,
    })
}

fn parse_expression_array(
    value: StrictValue,
    path: &str,
) -> Result<Vec<Expression>, InterchangeError> {
    expect_array(value, path)?
        .into_iter()
        .enumerate()
        .map(|(index, value)| parse_expression(value, &index_path(path, index)))
        .collect()
}

fn parse_expression(value: StrictValue, path: &str) -> Result<Expression, InterchangeError> {
    const COMMON: &[&str] = &["kind", "type"];
    let (kind, fields) = take_discriminated_object(value, path)?;
    let specific: &[&str] = match kind.as_str() {
        "literal" => &["value"],
        "field" => &["field"],
        "unary" => &["operation", "operand"],
        "binary" => &["operation", "left", "right"],
        "is_null" => &["operand", "negated"],
        "case" => &["arms", "fallback"],
        "cast" => &["operand", "target"],
        "in_list" => &["value", "candidates"],
        "exists" => &["query"],
        "in_query" => &["value", "query", "field"],
        _ => {
            return Err(InterchangeError::new(
                member_path(path, "kind"),
                "unknown expression kind",
            ));
        }
    };
    let allowed = COMMON
        .iter()
        .copied()
        .chain(specific.iter().copied())
        .collect::<Vec<_>>();
    let mut record = Record::new(StrictValue::Object(fields), path, &allowed, true)?;
    record.required("kind")?;
    let descriptor = parse_type(record.required("type")?, &member_path(path, "type"))?;
    let expression_kind = match kind.as_str() {
        "literal" => ExpressionKind::Literal(parse_literal(
            record.required("value")?,
            &member_path(path, "value"),
            descriptor.scalar,
        )?),
        "field" => ExpressionKind::Field(parse_field_id(
            record.required("field")?,
            &member_path(path, "field"),
        )?),
        "unary" => ExpressionKind::Unary {
            operation: parse_unary_operation(
                record.required("operation")?,
                &member_path(path, "operation"),
            )?,
            operand: Box::new(parse_expression(
                record.required("operand")?,
                &member_path(path, "operand"),
            )?),
        },
        "binary" => ExpressionKind::Binary {
            operation: parse_binary_operation(
                record.required("operation")?,
                &member_path(path, "operation"),
            )?,
            left: Box::new(parse_expression(
                record.required("left")?,
                &member_path(path, "left"),
            )?),
            right: Box::new(parse_expression(
                record.required("right")?,
                &member_path(path, "right"),
            )?),
        },
        "is_null" => ExpressionKind::IsNull {
            operand: Box::new(parse_expression(
                record.required("operand")?,
                &member_path(path, "operand"),
            )?),
            negated: expect_bool(record.required("negated")?, &member_path(path, "negated"))?,
        },
        "case" => {
            let arms_path = member_path(path, "arms");
            let arms = parse_case_arms(record.required("arms")?, &arms_path)?;
            if arms.is_empty() {
                return Err(InterchangeError::new(arms_path, "array must be nonempty"));
            }
            ExpressionKind::Case {
                arms,
                fallback: Box::new(parse_expression(
                    record.required("fallback")?,
                    &member_path(path, "fallback"),
                )?),
            }
        }
        "cast" => ExpressionKind::Cast {
            operand: Box::new(parse_expression(
                record.required("operand")?,
                &member_path(path, "operand"),
            )?),
            target: parse_scalar(record.required("target")?, &member_path(path, "target"))?,
        },
        "in_list" => {
            let candidates_path = member_path(path, "candidates");
            let candidates =
                parse_expression_array(record.required("candidates")?, &candidates_path)?;
            if candidates.is_empty() {
                return Err(InterchangeError::new(
                    candidates_path,
                    "array must be nonempty",
                ));
            }
            ExpressionKind::InList {
                value: Box::new(parse_expression(
                    record.required("value")?,
                    &member_path(path, "value"),
                )?),
                candidates,
            }
        }
        "exists" => ExpressionKind::Exists {
            query: parse_node_id(record.required("query")?, &member_path(path, "query"))?,
        },
        "in_query" => ExpressionKind::InQuery {
            value: Box::new(parse_expression(
                record.required("value")?,
                &member_path(path, "value"),
            )?),
            query: parse_node_id(record.required("query")?, &member_path(path, "query"))?,
            field: parse_field_id(record.required("field")?, &member_path(path, "field"))?,
        },
        _ => unreachable!("expression kind checked above"),
    };
    record.finish();
    Ok(Expression {
        kind: expression_kind,
        descriptor,
    })
}

fn parse_literal(
    value: StrictValue,
    path: &str,
    scalar: ScalarType,
) -> Result<LiteralValue, InterchangeError> {
    if matches!(value, StrictValue::Null) {
        return Ok(LiteralValue::Null);
    }
    match scalar {
        ScalarType::Boolean => expect_bool(value, path).map(LiteralValue::Boolean),
        ScalarType::Int64 => parse_i64(value, path).map(LiteralValue::Int64),
        ScalarType::Text => expect_string(value, path).map(LiteralValue::Text),
    }
}

fn parse_unary_operation(
    value: StrictValue,
    path: &str,
) -> Result<UnaryOperation, InterchangeError> {
    match expect_string(value, path)?.as_str() {
        "positive" => Ok(UnaryOperation::Positive),
        "negative" => Ok(UnaryOperation::Negative),
        "not" => Ok(UnaryOperation::Not),
        _ => Err(InterchangeError::new(path, "unknown unary operation")),
    }
}

fn parse_binary_operation(
    value: StrictValue,
    path: &str,
) -> Result<BinaryOperation, InterchangeError> {
    match expect_string(value, path)?.as_str() {
        "add" => Ok(BinaryOperation::Add),
        "subtract" => Ok(BinaryOperation::Subtract),
        "multiply" => Ok(BinaryOperation::Multiply),
        "divide" => Ok(BinaryOperation::Divide),
        "remainder" => Ok(BinaryOperation::Remainder),
        "concatenate" => Ok(BinaryOperation::Concatenate),
        "equal" => Ok(BinaryOperation::Equal),
        "not_equal" => Ok(BinaryOperation::NotEqual),
        "less" => Ok(BinaryOperation::Less),
        "less_or_equal" => Ok(BinaryOperation::LessOrEqual),
        "greater" => Ok(BinaryOperation::Greater),
        "greater_or_equal" => Ok(BinaryOperation::GreaterOrEqual),
        "and" => Ok(BinaryOperation::And),
        "or" => Ok(BinaryOperation::Or),
        _ => Err(InterchangeError::new(path, "unknown binary operation")),
    }
}

fn parse_case_arms(value: StrictValue, path: &str) -> Result<Vec<CaseArm>, InterchangeError> {
    expect_array(value, path)?
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let arm_path = index_path(path, index);
            let mut record = Record::new(value, &arm_path, &["when", "then"], false)?;
            let arm = CaseArm {
                when: parse_expression(record.required("when")?, &member_path(&arm_path, "when"))?,
                then: parse_expression(record.required("then")?, &member_path(&arm_path, "then"))?,
            };
            record.finish();
            Ok(arm)
        })
        .collect()
}

fn ensure_encodable_identities(graph: &Graph) -> Result<(), EncodeError> {
    if !is_identifier(graph.root.as_str()) {
        return Err(EncodeError::InvalidIdentifier(graph.root.as_str().into()));
    }
    for node in &graph.nodes {
        if !is_identifier(node.id.as_str()) {
            return Err(EncodeError::InvalidIdentifier(node.id.as_str().into()));
        }
        for field in &node.output_schema {
            if !is_identifier(field.id.as_str()) {
                return Err(EncodeError::InvalidIdentifier(field.id.as_str().into()));
            }
        }
    }
    Ok(())
}

macro_rules! json_object {
    ($($key:literal => $value:expr),* $(,)?) => {{
        let mut members = JsonMap::new();
        $(members.insert($key.into(), $value);)*
        JsonValue::Object(members)
    }};
}

fn json_string(value: impl Into<String>) -> JsonValue {
    JsonValue::String(value.into())
}

fn encode_graph(graph: &Graph) -> JsonValue {
    json_object! {
        "interchange_version" => json_string(VERSION),
        "shape_ir_version" => json_string(&graph.version),
        "root" => json_string(graph.root.as_str()),
        "nodes" => JsonValue::Array(graph.nodes.iter().map(encode_node).collect()),
    }
}

fn encode_node(node: &Node) -> JsonValue {
    let mut members = JsonMap::new();
    members.insert("id".into(), json_string(node.id.as_str()));
    members.insert("kind".into(), json_string(node_kind_name(&node.kind)));
    match &node.kind {
        NodeKind::Input { binding } => {
            members.insert("binding".into(), json_string(binding.as_str()));
        }
        NodeKind::Empty => {}
        NodeKind::Project { input, entries } => {
            members.insert("input".into(), json_string(input.as_str()));
            members.insert(
                "entries".into(),
                JsonValue::Array(entries.iter().map(encode_project_entry).collect()),
            );
        }
        NodeKind::Filter { input, predicate } => {
            members.insert("input".into(), json_string(input.as_str()));
            members.insert("predicate".into(), encode_expression(predicate));
        }
        NodeKind::Join {
            left,
            right,
            join_type,
            condition,
        } => {
            members.insert("left".into(), json_string(left.as_str()));
            members.insert("right".into(), json_string(right.as_str()));
            members.insert("join_type".into(), json_string(join_type_name(*join_type)));
            if let Some(condition) = condition {
                members.insert("condition".into(), encode_expression(condition));
            }
        }
        NodeKind::Aggregate {
            input,
            grouping_keys,
            aggregates,
        } => {
            members.insert("input".into(), json_string(input.as_str()));
            members.insert(
                "grouping_keys".into(),
                JsonValue::Array(grouping_keys.iter().map(encode_grouping_key).collect()),
            );
            members.insert(
                "aggregates".into(),
                JsonValue::Array(aggregates.iter().map(encode_aggregate).collect()),
            );
        }
        NodeKind::Window { input, definitions } => {
            members.insert("input".into(), json_string(input.as_str()));
            members.insert(
                "definitions".into(),
                JsonValue::Array(definitions.iter().map(encode_window_definition).collect()),
            );
        }
        NodeKind::Distinct { input } | NodeKind::ForgetOrder { input } => {
            members.insert("input".into(), json_string(input.as_str()));
        }
        NodeKind::Set {
            left,
            right,
            operation,
            quantifier,
        } => {
            members.insert("left".into(), json_string(left.as_str()));
            members.insert("right".into(), json_string(right.as_str()));
            members.insert(
                "operation".into(),
                json_string(set_operation_name(*operation)),
            );
            members.insert(
                "quantifier".into(),
                json_string(set_quantifier_name(*quantifier)),
            );
        }
        NodeKind::Order { input, items } => {
            members.insert("input".into(), json_string(input.as_str()));
            members.insert(
                "items".into(),
                JsonValue::Array(items.iter().map(encode_ordering_item).collect()),
            );
        }
        NodeKind::Slice {
            input,
            offset,
            limit,
        } => {
            members.insert("input".into(), json_string(input.as_str()));
            members.insert("offset".into(), json_string(offset.to_string()));
            members.insert(
                "limit".into(),
                limit
                    .map(|value| json_string(value.to_string()))
                    .unwrap_or(JsonValue::Null),
            );
        }
    }
    members.insert(
        "output_schema".into(),
        JsonValue::Array(node.output_schema.iter().map(encode_field).collect()),
    );
    members.insert(
        "collection".into(),
        json_string(collection_name(node.collection)),
    );
    JsonValue::Object(members)
}

fn node_kind_name(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Input { .. } => "input",
        NodeKind::Empty => "empty",
        NodeKind::Project { .. } => "project",
        NodeKind::Filter { .. } => "filter",
        NodeKind::Join { .. } => "join",
        NodeKind::Aggregate { .. } => "aggregate",
        NodeKind::Window { .. } => "window",
        NodeKind::Distinct { .. } => "distinct",
        NodeKind::Set { .. } => "set",
        NodeKind::Order { .. } => "order",
        NodeKind::Slice { .. } => "slice",
        NodeKind::ForgetOrder { .. } => "forget_order",
    }
}

fn collection_name(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Bag => "bag",
        CollectionKind::Ordered => "ordered",
    }
}

fn join_type_name(join_type: JoinType) -> &'static str {
    match join_type {
        JoinType::Cross => "cross",
        JoinType::Inner => "inner",
        JoinType::Left => "left",
        JoinType::Right => "right",
        JoinType::Full => "full",
    }
}

fn set_operation_name(operation: SetOperation) -> &'static str {
    match operation {
        SetOperation::Union => "union",
        SetOperation::Intersect => "intersect",
        SetOperation::Except => "except",
    }
}

fn set_quantifier_name(quantifier: SetQuantifier) -> &'static str {
    match quantifier {
        SetQuantifier::All => "all",
        SetQuantifier::Distinct => "distinct",
    }
}

fn encode_field(field: &Field) -> JsonValue {
    json_object! {
        "id" => json_string(field.id.as_str()),
        "name" => json_string(&field.name),
        "type" => encode_type(field.descriptor),
    }
}

fn encode_type(descriptor: TypeDescriptor) -> JsonValue {
    json_object! {
        "scalar" => json_string(scalar_name(descriptor.scalar)),
        "nullable" => JsonValue::Bool(descriptor.nullable),
    }
}

fn scalar_name(scalar: ScalarType) -> &'static str {
    match scalar {
        ScalarType::Boolean => "boolean",
        ScalarType::Int64 => "int64",
        ScalarType::Text => "text",
    }
}

fn encode_project_entry(entry: &ProjectEntry) -> JsonValue {
    match entry {
        ProjectEntry::Keep(field) => json_object! {
            "kind" => json_string("keep"),
            "field" => json_string(field.as_str()),
        },
        ProjectEntry::Compute { output, expression } => json_object! {
            "kind" => json_string("compute"),
            "output" => json_string(output.as_str()),
            "expression" => encode_expression(expression),
        },
    }
}

fn encode_grouping_key(key: &GroupingKey) -> JsonValue {
    json_object! {
        "output" => json_string(key.output.as_str()),
        "expression" => encode_expression(&key.expression),
    }
}

fn encode_aggregate(aggregate: &AggregateDefinition) -> JsonValue {
    let mut members = JsonMap::new();
    members.insert("output".into(), json_string(aggregate.output.as_str()));
    members.insert(
        "function".into(),
        json_string(aggregate_function_name(aggregate.function)),
    );
    if let Some(argument) = &aggregate.argument {
        members.insert("argument".into(), encode_expression(argument));
    }
    JsonValue::Object(members)
}

fn aggregate_function_name(function: AggregateFunction) -> &'static str {
    match function {
        AggregateFunction::CountAll => "count_all",
        AggregateFunction::Count => "count",
        AggregateFunction::Sum => "sum",
        AggregateFunction::Min => "min",
        AggregateFunction::Max => "max",
        AggregateFunction::BoolAnd => "bool_and",
        AggregateFunction::BoolOr => "bool_or",
    }
}

fn encode_window_definition(definition: &WindowDefinition) -> JsonValue {
    match definition {
        WindowDefinition::PartitionedAggregate {
            output,
            function,
            argument,
            partition_by,
        } => {
            let mut members = JsonMap::new();
            members.insert("kind".into(), json_string("partitioned_aggregate"));
            members.insert("output".into(), json_string(output.as_str()));
            members.insert(
                "function".into(),
                json_string(aggregate_function_name(*function)),
            );
            if let Some(argument) = argument {
                members.insert("argument".into(), encode_expression(argument));
            }
            members.insert(
                "partition_by".into(),
                JsonValue::Array(partition_by.iter().map(encode_expression).collect()),
            );
            JsonValue::Object(members)
        }
        WindowDefinition::Ranking {
            output,
            function,
            partition_by,
            order_by,
        } => json_object! {
            "kind" => json_string("ranking"),
            "output" => json_string(output.as_str()),
            "function" => json_string(ranking_function_name(*function)),
            "partition_by" => JsonValue::Array(
                partition_by.iter().map(encode_expression).collect()
            ),
            "order_by" => JsonValue::Array(
                order_by.iter().map(encode_ordering_item).collect()
            ),
        },
    }
}

fn ranking_function_name(function: RankingFunction) -> &'static str {
    match function {
        RankingFunction::RowNumber => "row_number",
        RankingFunction::Rank => "rank",
        RankingFunction::DenseRank => "dense_rank",
    }
}

fn encode_ordering_item(item: &OrderingItem) -> JsonValue {
    json_object! {
        "expression" => encode_expression(&item.expression),
        "direction" => json_string(match item.direction {
            Direction::Ascending => "ascending",
            Direction::Descending => "descending",
        }),
        "null_placement" => json_string(match item.null_placement {
            NullPlacement::First => "first",
            NullPlacement::Last => "last",
            NullPlacement::NotApplicable => "not_applicable",
        }),
    }
}

fn encode_expression(expression: &Expression) -> JsonValue {
    let mut members = JsonMap::new();
    members.insert(
        "kind".into(),
        json_string(expression_kind_name(&expression.kind)),
    );
    members.insert("type".into(), encode_type(expression.descriptor));
    match &expression.kind {
        ExpressionKind::Literal(value) => {
            members.insert("value".into(), encode_literal(value));
        }
        ExpressionKind::Field(field) => {
            members.insert("field".into(), json_string(field.as_str()));
        }
        ExpressionKind::Unary { operation, operand } => {
            members.insert(
                "operation".into(),
                json_string(unary_operation_name(*operation)),
            );
            members.insert("operand".into(), encode_expression(operand));
        }
        ExpressionKind::Binary {
            operation,
            left,
            right,
        } => {
            members.insert(
                "operation".into(),
                json_string(binary_operation_name(*operation)),
            );
            members.insert("left".into(), encode_expression(left));
            members.insert("right".into(), encode_expression(right));
        }
        ExpressionKind::IsNull { operand, negated } => {
            members.insert("operand".into(), encode_expression(operand));
            members.insert("negated".into(), JsonValue::Bool(*negated));
        }
        ExpressionKind::Case { arms, fallback } => {
            members.insert(
                "arms".into(),
                JsonValue::Array(arms.iter().map(encode_case_arm).collect()),
            );
            members.insert("fallback".into(), encode_expression(fallback));
        }
        ExpressionKind::Cast { operand, target } => {
            members.insert("operand".into(), encode_expression(operand));
            members.insert("target".into(), json_string(scalar_name(*target)));
        }
        ExpressionKind::InList { value, candidates } => {
            members.insert("value".into(), encode_expression(value));
            members.insert(
                "candidates".into(),
                JsonValue::Array(candidates.iter().map(encode_expression).collect()),
            );
        }
        ExpressionKind::Exists { query } => {
            members.insert("query".into(), json_string(query.as_str()));
        }
        ExpressionKind::InQuery {
            value,
            query,
            field,
        } => {
            members.insert("value".into(), encode_expression(value));
            members.insert("query".into(), json_string(query.as_str()));
            members.insert("field".into(), json_string(field.as_str()));
        }
    }
    JsonValue::Object(members)
}

fn expression_kind_name(kind: &ExpressionKind) -> &'static str {
    match kind {
        ExpressionKind::Literal(_) => "literal",
        ExpressionKind::Field(_) => "field",
        ExpressionKind::Unary { .. } => "unary",
        ExpressionKind::Binary { .. } => "binary",
        ExpressionKind::IsNull { .. } => "is_null",
        ExpressionKind::Case { .. } => "case",
        ExpressionKind::Cast { .. } => "cast",
        ExpressionKind::InList { .. } => "in_list",
        ExpressionKind::Exists { .. } => "exists",
        ExpressionKind::InQuery { .. } => "in_query",
    }
}

fn encode_literal(value: &LiteralValue) -> JsonValue {
    match value {
        LiteralValue::Boolean(value) => JsonValue::Bool(*value),
        LiteralValue::Int64(value) => json_string(value.to_string()),
        LiteralValue::Text(value) => json_string(value),
        LiteralValue::Null => JsonValue::Null,
    }
}

fn unary_operation_name(operation: UnaryOperation) -> &'static str {
    match operation {
        UnaryOperation::Positive => "positive",
        UnaryOperation::Negative => "negative",
        UnaryOperation::Not => "not",
    }
}

fn binary_operation_name(operation: BinaryOperation) -> &'static str {
    match operation {
        BinaryOperation::Add => "add",
        BinaryOperation::Subtract => "subtract",
        BinaryOperation::Multiply => "multiply",
        BinaryOperation::Divide => "divide",
        BinaryOperation::Remainder => "remainder",
        BinaryOperation::Concatenate => "concatenate",
        BinaryOperation::Equal => "equal",
        BinaryOperation::NotEqual => "not_equal",
        BinaryOperation::Less => "less",
        BinaryOperation::LessOrEqual => "less_or_equal",
        BinaryOperation::Greater => "greater",
        BinaryOperation::GreaterOrEqual => "greater_or_equal",
        BinaryOperation::And => "and",
        BinaryOperation::Or => "or",
    }
}

fn encode_case_arm(arm: &CaseArm) -> JsonValue {
    json_object! {
        "when" => encode_expression(&arm.when),
        "then" => encode_expression(&arm.then),
    }
}
