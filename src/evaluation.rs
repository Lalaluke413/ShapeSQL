//! Fully materialized reference evaluation for valid Shape IR 0.1 graphs.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::shape_ir::{
    AggregateDefinition, AggregateFunction, BinaryOperation, CollectionKind, Direction, Expression,
    ExpressionKind, Field, FieldId, Graph, JoinType, LiteralValue, Node, NodeId, NodeKind,
    NullPlacement, OrderingItem, ProjectEntry, RankingFunction, SetOperation, SetQuantifier,
    UnaryOperation, ValidationError, WindowDefinition,
};
use crate::{RelationBinding, ScalarType, TypeDescriptor};

/// One runtime scalar value.
///
/// A null value receives its scalar type from its field or expression descriptor.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Value {
    Boolean(bool),
    Int64(i64),
    Text(String),
    Null,
}

impl Value {
    /// Returns the scalar type of a non-null value.
    pub const fn scalar_type(&self) -> Option<ScalarType> {
        match self {
            Self::Boolean(_) => Some(ScalarType::Boolean),
            Self::Int64(_) => Some(ScalarType::Int64),
            Self::Text(_) => Some(ScalarType::Text),
            Self::Null => None,
        }
    }

    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Int64(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}

/// One row occurrence. Duplicate `Row` values represent bag multiplicity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Row {
    pub values: Vec<Value>,
}

impl Row {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }
}

impl From<Vec<Value>> for Row {
    fn from(values: Vec<Value>) -> Self {
        Self::new(values)
    }
}

/// One host-facing field declaration, without a graph-local field identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputField {
    pub name: String,
    pub descriptor: TypeDescriptor,
}

impl InputField {
    pub fn new(name: impl Into<String>, descriptor: TypeDescriptor) -> Self {
        Self {
            name: name.into(),
            descriptor,
        }
    }
}

/// A complete finite host relation supplied through one opaque binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputRelation {
    pub schema: Vec<InputField>,
    pub rows: Vec<Row>,
}

impl InputRelation {
    pub fn new(schema: Vec<InputField>, rows: Vec<Row>) -> Self {
        Self { schema, rows }
    }
}

/// An immutable set of host relations available to one evaluation.
///
/// Relations are checked only when their `input` node is demanded. An absent or
/// malformed relation referenced exclusively by an unselected conditional
/// dependency therefore remains unobservable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    relations: HashMap<RelationBinding, InputRelation>,
}

impl Snapshot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_relations<I, B>(relations: I) -> Result<Self, SnapshotError>
    where
        I: IntoIterator<Item = (B, InputRelation)>,
        B: Into<RelationBinding>,
    {
        let mut snapshot = Self::new();
        for (binding, relation) in relations {
            snapshot.insert(binding, relation)?;
        }
        Ok(snapshot)
    }

    pub fn insert(
        &mut self,
        binding: impl Into<RelationBinding>,
        relation: InputRelation,
    ) -> Result<(), SnapshotError> {
        let binding = binding.into();
        if self.relations.contains_key(&binding) {
            return Err(SnapshotError::DuplicateBinding(binding));
        }
        self.relations.insert(binding, relation);
        Ok(())
    }

    pub fn get(&self, binding: &RelationBinding) -> Option<&InputRelation> {
        self.relations.get(binding)
    }

    pub fn len(&self) -> usize {
        self.relations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.relations.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotError {
    DuplicateBinding(RelationBinding),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateBinding(binding) => {
                write!(
                    formatter,
                    "duplicate snapshot binding `{}`",
                    binding.as_str()
                )
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

/// The complete successful result of evaluating one graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvaluationResult {
    pub schema: Vec<Field>,
    pub collection: CollectionKind,
    pub rows: Vec<Row>,
}

/// A demanded host relation did not conform to its `input` node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputError {
    pub node: NodeId,
    pub binding: RelationBinding,
    pub kind: InputErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputErrorKind {
    MissingBinding,
    SchemaArity {
        expected: usize,
        actual: usize,
    },
    FieldName {
        index: usize,
        expected: String,
        actual: String,
    },
    FieldDescriptor {
        index: usize,
        expected: TypeDescriptor,
        actual: TypeDescriptor,
    },
    RowArity {
        row: usize,
        expected: usize,
        actual: usize,
    },
    InvalidValue {
        row: usize,
        field: usize,
        expected: TypeDescriptor,
        actual: Option<ScalarType>,
    },
}

impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        use InputErrorKind as K;
        write!(
            formatter,
            "host binding `{}` for input node `{}` ",
            self.binding.as_str(),
            self.node
        )?;
        match &self.kind {
            K::MissingBinding => formatter.write_str("is missing"),
            K::SchemaArity { expected, actual } => write!(
                formatter,
                "has {actual} fields but the node requires {expected}"
            ),
            K::FieldName {
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "has field name `{actual}` at position {index}, expected `{expected}`"
            ),
            K::FieldDescriptor {
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "has descriptor {actual:?} at position {index}, expected {expected:?}"
            ),
            K::RowArity {
                row,
                expected,
                actual,
            } => write!(
                formatter,
                "has {actual} values in row {row}, expected {expected}"
            ),
            K::InvalidValue {
                row,
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "has value type {actual:?} at row {row}, field {field}, expected {expected:?}"
            ),
        }
    }
}

impl std::error::Error for InputError {}

/// A semantic failure encountered while evaluating valid Shape IR over valid
/// demanded inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvaluationError {
    pub node: NodeId,
    pub kind: EvaluationErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluationErrorKind {
    IntegerOverflow,
    DivisionByZero,
    RemainderByZero,
    InvalidTextCast { target: ScalarType },
}

impl fmt::Display for EvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        use EvaluationErrorKind as K;
        write!(formatter, "evaluation failed at node `{}`: ", self.node)?;
        match self.kind {
            K::IntegerOverflow => formatter.write_str("INT64 overflow"),
            K::DivisionByZero => formatter.write_str("division by zero"),
            K::RemainderByZero => formatter.write_str("remainder by zero"),
            K::InvalidTextCast { target } => {
                write!(formatter, "invalid TEXT to {target:?} cast")
            }
        }
    }
}

impl std::error::Error for EvaluationError {}

/// A failure at one of the three boundaries traversed by [`evaluate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvaluateError {
    Validation(ValidationError),
    Input(InputError),
    Evaluation(EvaluationError),
}

impl fmt::Display for EvaluateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => fmt::Display::fmt(error, formatter),
            Self::Input(error) => fmt::Display::fmt(error, formatter),
            Self::Evaluation(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for EvaluateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::Input(error) => Some(error),
            Self::Evaluation(error) => Some(error),
        }
    }
}

impl From<InputError> for EvaluateError {
    fn from(error: InputError) -> Self {
        Self::Input(error)
    }
}

impl From<EvaluationError> for EvaluateError {
    fn from(error: EvaluationError) -> Self {
        Self::Evaluation(error)
    }
}

/// Validates and fully evaluates one Shape IR graph over an immutable snapshot.
pub fn evaluate(graph: &Graph, snapshot: &Snapshot) -> Result<EvaluationResult, EvaluateError> {
    let summary = graph.validate().map_err(EvaluateError::Validation)?;
    let mut evaluator = Evaluator::new(graph, snapshot);
    let root = evaluator.evaluate_node(&graph.root)?;
    Ok(EvaluationResult {
        schema: summary.root_schema,
        collection: summary.root_collection,
        rows: root.rows.clone(),
    })
}

#[derive(Clone, Debug)]
struct NodeValue {
    rows: Vec<Row>,
}

#[derive(Clone, Debug)]
struct RowGroup {
    key: Vec<Value>,
    indices: Vec<usize>,
}

struct Evaluator<'a> {
    graph: &'a Graph,
    snapshot: &'a Snapshot,
    nodes: HashMap<NodeId, usize>,
    cache: HashMap<NodeId, Rc<NodeValue>>,
}

impl<'a> Evaluator<'a> {
    fn new(graph: &'a Graph, snapshot: &'a Snapshot) -> Self {
        let nodes = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id.clone(), index))
            .collect();
        Self {
            graph,
            snapshot,
            nodes,
            cache: HashMap::new(),
        }
    }

    fn node(&self, id: &NodeId) -> &Node {
        &self.graph.nodes[*self
            .nodes
            .get(id)
            .expect("validated graph contains every referenced node")]
    }

    fn schema(&self, id: &NodeId) -> &[Field] {
        &self.node(id).output_schema
    }

    fn evaluate_node(&mut self, id: &NodeId) -> Result<Rc<NodeValue>, EvaluateError> {
        if let Some(value) = self.cache.get(id) {
            return Ok(Rc::clone(value));
        }

        let node = self.node(id).clone();
        let rows = match node.kind {
            NodeKind::Input { binding } => {
                self.evaluate_input(&node.id, &binding, &node.output_schema)?
            }
            NodeKind::Empty => Vec::new(),
            NodeKind::Project { input, entries } => {
                self.evaluate_project(&node.id, &input, &entries)?
            }
            NodeKind::Filter { input, predicate } => {
                self.evaluate_filter(&node.id, &input, &predicate)?
            }
            NodeKind::Join {
                left,
                right,
                join_type,
                condition,
            } => self.evaluate_join(&node.id, &left, &right, join_type, condition.as_ref())?,
            NodeKind::Aggregate {
                input,
                grouping_keys,
                aggregates,
            } => self.evaluate_aggregate_node(&node.id, &input, &grouping_keys, &aggregates)?,
            NodeKind::Window { input, definitions } => {
                self.evaluate_window(&node.id, &input, &definitions)?
            }
            NodeKind::Distinct { input } => self.evaluate_distinct(&input)?,
            NodeKind::Set {
                left,
                right,
                operation,
                quantifier,
            } => self.evaluate_set(&left, &right, operation, quantifier)?,
            NodeKind::Order { input, items } => self.evaluate_order(&node.id, &input, &items)?,
            NodeKind::Slice {
                input,
                offset,
                limit,
            } => self.evaluate_slice(&input, offset, limit)?,
            NodeKind::ForgetOrder { input } => self.evaluate_node(&input)?.rows.clone(),
        };

        let value = Rc::new(NodeValue { rows });
        self.cache.insert(id.clone(), Rc::clone(&value));
        Ok(value)
    }

    fn evaluate_input(
        &self,
        node: &NodeId,
        binding: &RelationBinding,
        expected: &[Field],
    ) -> Result<Vec<Row>, InputError> {
        let relation = self.snapshot.get(binding).ok_or_else(|| InputError {
            node: node.clone(),
            binding: binding.clone(),
            kind: InputErrorKind::MissingBinding,
        })?;
        let error = |kind| InputError {
            node: node.clone(),
            binding: binding.clone(),
            kind,
        };

        if relation.schema.len() != expected.len() {
            return Err(error(InputErrorKind::SchemaArity {
                expected: expected.len(),
                actual: relation.schema.len(),
            }));
        }
        for (index, (actual, expected)) in relation.schema.iter().zip(expected).enumerate() {
            if actual.name != expected.name {
                return Err(error(InputErrorKind::FieldName {
                    index,
                    expected: expected.name.clone(),
                    actual: actual.name.clone(),
                }));
            }
            if actual.descriptor != expected.descriptor {
                return Err(error(InputErrorKind::FieldDescriptor {
                    index,
                    expected: expected.descriptor,
                    actual: actual.descriptor,
                }));
            }
        }
        for (row_index, row) in relation.rows.iter().enumerate() {
            if row.values.len() != expected.len() {
                return Err(error(InputErrorKind::RowArity {
                    row: row_index,
                    expected: expected.len(),
                    actual: row.values.len(),
                }));
            }
            for (field_index, (value, field)) in row.values.iter().zip(expected).enumerate() {
                let valid = match value.scalar_type() {
                    Some(actual) => actual == field.descriptor.scalar,
                    None => field.descriptor.nullable,
                };
                if !valid {
                    return Err(error(InputErrorKind::InvalidValue {
                        row: row_index,
                        field: field_index,
                        expected: field.descriptor,
                        actual: value.scalar_type(),
                    }));
                }
            }
        }
        Ok(relation.rows.clone())
    }

    fn evaluate_project(
        &mut self,
        node: &NodeId,
        input: &NodeId,
        entries: &[ProjectEntry],
    ) -> Result<Vec<Row>, EvaluateError> {
        let schema = self.schema(input).to_vec();
        let input = self.evaluate_node(input)?;
        let mut rows = Vec::with_capacity(input.rows.len());
        for row in &input.rows {
            let mut values = Vec::with_capacity(entries.len());
            for entry in entries {
                match entry {
                    ProjectEntry::Keep(field) => {
                        values.push(row.values[field_position(&schema, field)].clone());
                    }
                    ProjectEntry::Compute { expression, .. } => {
                        values.push(self.evaluate_expression(node, expression, &schema, row)?);
                    }
                }
            }
            rows.push(Row::new(values));
        }
        Ok(rows)
    }

    fn evaluate_filter(
        &mut self,
        node: &NodeId,
        input: &NodeId,
        predicate: &Expression,
    ) -> Result<Vec<Row>, EvaluateError> {
        let schema = self.schema(input).to_vec();
        let input = self.evaluate_node(input)?;
        let mut rows = Vec::new();
        for row in &input.rows {
            if self.evaluate_expression(node, predicate, &schema, row)? == Value::Boolean(true) {
                rows.push(row.clone());
            }
        }
        Ok(rows)
    }

    fn evaluate_join(
        &mut self,
        node: &NodeId,
        left_id: &NodeId,
        right_id: &NodeId,
        join_type: JoinType,
        condition: Option<&Expression>,
    ) -> Result<Vec<Row>, EvaluateError> {
        let mut environment = self.schema(left_id).to_vec();
        let left_width = environment.len();
        let right_width = self.schema(right_id).len();
        environment.extend_from_slice(self.schema(right_id));

        // Both ordinary inputs remain strict, including when one is empty.
        let left = self.evaluate_node(left_id)?;
        let right = self.evaluate_node(right_id)?;
        let mut left_matched = vec![false; left.rows.len()];
        let mut right_matched = vec![false; right.rows.len()];
        let mut rows = Vec::new();

        for (left_index, left_row) in left.rows.iter().enumerate() {
            for (right_index, right_row) in right.rows.iter().enumerate() {
                let candidate = concatenate_rows(left_row, right_row);
                let matches = match condition {
                    None => true,
                    Some(condition) => {
                        self.evaluate_expression(node, condition, &environment, &candidate)?
                            == Value::Boolean(true)
                    }
                };
                if matches {
                    left_matched[left_index] = true;
                    right_matched[right_index] = true;
                    rows.push(candidate);
                }
            }
        }

        if matches!(join_type, JoinType::Left | JoinType::Full) {
            for (matched, row) in left_matched.iter().zip(&left.rows) {
                if !matched {
                    let mut values = row.values.clone();
                    values.extend(std::iter::repeat_n(Value::Null, right_width));
                    rows.push(Row::new(values));
                }
            }
        }
        if matches!(join_type, JoinType::Right | JoinType::Full) {
            for (matched, row) in right_matched.iter().zip(&right.rows) {
                if !matched {
                    let mut values = vec![Value::Null; left_width];
                    values.extend(row.values.iter().cloned());
                    rows.push(Row::new(values));
                }
            }
        }
        Ok(rows)
    }

    fn evaluate_aggregate_node(
        &mut self,
        node: &NodeId,
        input_id: &NodeId,
        grouping_keys: &[crate::shape_ir::GroupingKey],
        aggregates: &[AggregateDefinition],
    ) -> Result<Vec<Row>, EvaluateError> {
        let schema = self.schema(input_id).to_vec();
        let input = self.evaluate_node(input_id)?;
        let expressions = grouping_keys
            .iter()
            .map(|key| key.expression.clone())
            .collect::<Vec<_>>();
        let groups = if expressions.is_empty() {
            vec![RowGroup {
                key: Vec::new(),
                indices: (0..input.rows.len()).collect(),
            }]
        } else {
            self.group_rows(node, &expressions, &schema, &input.rows)?
        };

        let mut rows = Vec::with_capacity(groups.len());
        for group in groups {
            let mut values = group.key;
            for aggregate in aggregates {
                values.push(self.evaluate_aggregate(
                    node,
                    aggregate.function,
                    aggregate.argument.as_ref(),
                    &schema,
                    &input.rows,
                    &group.indices,
                )?);
            }
            rows.push(Row::new(values));
        }
        Ok(rows)
    }

    fn evaluate_window(
        &mut self,
        node: &NodeId,
        input_id: &NodeId,
        definitions: &[WindowDefinition],
    ) -> Result<Vec<Row>, EvaluateError> {
        let schema = self.schema(input_id).to_vec();
        let input = self.evaluate_node(input_id)?;
        let mut columns = Vec::with_capacity(definitions.len());

        for definition in definitions {
            let column = match definition {
                WindowDefinition::PartitionedAggregate {
                    function,
                    argument,
                    partition_by,
                    ..
                } => {
                    let groups = self.group_rows(node, partition_by, &schema, &input.rows)?;
                    let mut column = vec![Value::Null; input.rows.len()];
                    for group in groups {
                        let value = self.evaluate_aggregate(
                            node,
                            *function,
                            argument.as_ref(),
                            &schema,
                            &input.rows,
                            &group.indices,
                        )?;
                        for index in group.indices {
                            column[index] = value.clone();
                        }
                    }
                    column
                }
                WindowDefinition::Ranking {
                    function,
                    partition_by,
                    order_by,
                    ..
                } => self.evaluate_ranking(
                    node,
                    *function,
                    partition_by,
                    order_by,
                    &schema,
                    &input.rows,
                )?,
            };
            columns.push(column);
        }

        let mut rows = Vec::with_capacity(input.rows.len());
        for (index, row) in input.rows.iter().enumerate() {
            let mut values = row.values.clone();
            for column in &columns {
                values.push(column[index].clone());
            }
            rows.push(Row::new(values));
        }
        Ok(rows)
    }

    fn evaluate_ranking(
        &mut self,
        node: &NodeId,
        function: RankingFunction,
        partition_by: &[Expression],
        order_by: &[OrderingItem],
        schema: &[Field],
        rows: &[Row],
    ) -> Result<Vec<Value>, EvaluateError> {
        let groups = self.group_rows(node, partition_by, schema, rows)?;
        let order_keys = self.evaluate_order_keys(node, order_by, schema, rows)?;
        let mut output = vec![Value::Null; rows.len()];

        for group in groups {
            let mut indices = group.indices;
            indices.sort_by(|left, right| {
                compare_order_keys(&order_keys[*left], &order_keys[*right], order_by)
            });
            let mut rank = 0_i64;
            let mut dense_rank = 0_i64;
            for (position, row_index) in indices.iter().copied().enumerate() {
                let new_peer = position == 0
                    || !order_keys_equal(
                        &order_keys[row_index],
                        &order_keys[indices[position - 1]],
                    );
                if new_peer {
                    rank = position_to_int64(node, position)?;
                    dense_rank = dense_rank.checked_add(1).ok_or_else(|| {
                        evaluation_error(node, EvaluationErrorKind::IntegerOverflow)
                    })?;
                }
                let value = match function {
                    RankingFunction::RowNumber => position_to_int64(node, position)?,
                    RankingFunction::Rank => rank,
                    RankingFunction::DenseRank => dense_rank,
                };
                output[row_index] = Value::Int64(value);
            }
        }
        Ok(output)
    }

    fn evaluate_distinct(&mut self, input: &NodeId) -> Result<Vec<Row>, EvaluateError> {
        let input = self.evaluate_node(input)?;
        let mut rows = Vec::new();
        for row in &input.rows {
            if !rows.contains(row) {
                rows.push(row.clone());
            }
        }
        Ok(rows)
    }

    fn evaluate_set(
        &mut self,
        left: &NodeId,
        right: &NodeId,
        operation: SetOperation,
        quantifier: SetQuantifier,
    ) -> Result<Vec<Row>, EvaluateError> {
        // Both ordinary inputs remain strict for every set operation.
        let left = self.evaluate_node(left)?;
        let right = self.evaluate_node(right)?;
        let mut rows = Vec::new();

        match (operation, quantifier) {
            (SetOperation::Union, SetQuantifier::All) => {
                rows.extend(left.rows.iter().cloned());
                rows.extend(right.rows.iter().cloned());
            }
            (SetOperation::Union, SetQuantifier::Distinct) => {
                for row in left.rows.iter().chain(&right.rows) {
                    if !rows.contains(row) {
                        rows.push(row.clone());
                    }
                }
            }
            (SetOperation::Intersect, SetQuantifier::All) => {
                let mut remaining = row_counts(&right.rows);
                for row in &left.rows {
                    if take_one(&mut remaining, row) {
                        rows.push(row.clone());
                    }
                }
            }
            (SetOperation::Intersect, SetQuantifier::Distinct) => {
                for row in &left.rows {
                    if right.rows.contains(row) && !rows.contains(row) {
                        rows.push(row.clone());
                    }
                }
            }
            (SetOperation::Except, SetQuantifier::All) => {
                let mut remaining = row_counts(&right.rows);
                for row in &left.rows {
                    if !take_one(&mut remaining, row) {
                        rows.push(row.clone());
                    }
                }
            }
            (SetOperation::Except, SetQuantifier::Distinct) => {
                for row in &left.rows {
                    if !right.rows.contains(row) && !rows.contains(row) {
                        rows.push(row.clone());
                    }
                }
            }
        }
        Ok(rows)
    }

    fn evaluate_order(
        &mut self,
        node: &NodeId,
        input_id: &NodeId,
        items: &[OrderingItem],
    ) -> Result<Vec<Row>, EvaluateError> {
        let schema = self.schema(input_id).to_vec();
        let input = self.evaluate_node(input_id)?;
        // Materializing every key before sorting ensures a distinguishing early
        // item cannot suppress an error in a later item.
        let keys = self.evaluate_order_keys(node, items, &schema, &input.rows)?;
        let mut indices = (0..input.rows.len()).collect::<Vec<_>>();
        indices.sort_by(|left, right| compare_order_keys(&keys[*left], &keys[*right], items));
        Ok(indices
            .into_iter()
            .map(|index| input.rows[index].clone())
            .collect())
    }

    fn evaluate_slice(
        &mut self,
        input: &NodeId,
        offset: i64,
        limit: Option<i64>,
    ) -> Result<Vec<Row>, EvaluateError> {
        // The complete ordered input is evaluated before applying either bound.
        let input = self.evaluate_node(input)?;
        // On a target whose `usize` is narrower than INT64, an unrepresentable
        // bound necessarily exceeds the number of materializable row
        // occurrences.
        let offset = usize::try_from(offset).unwrap_or(usize::MAX);
        let limit = limit.map(|limit| usize::try_from(limit).unwrap_or(usize::MAX));
        let rows = input.rows.iter().skip(offset);
        Ok(match limit {
            Some(limit) => rows.take(limit).cloned().collect(),
            None => rows.cloned().collect(),
        })
    }

    fn group_rows(
        &mut self,
        node: &NodeId,
        expressions: &[Expression],
        schema: &[Field],
        rows: &[Row],
    ) -> Result<Vec<RowGroup>, EvaluateError> {
        let mut groups: Vec<RowGroup> = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            let mut key = Vec::with_capacity(expressions.len());
            for expression in expressions {
                key.push(self.evaluate_expression(node, expression, schema, row)?);
            }
            if let Some(group) = groups.iter_mut().find(|group| group.key == key) {
                group.indices.push(index);
            } else {
                groups.push(RowGroup {
                    key,
                    indices: vec![index],
                });
            }
        }
        Ok(groups)
    }

    fn evaluate_order_keys(
        &mut self,
        node: &NodeId,
        items: &[OrderingItem],
        schema: &[Field],
        rows: &[Row],
    ) -> Result<Vec<Vec<Value>>, EvaluateError> {
        let mut keys = Vec::with_capacity(rows.len());
        for row in rows {
            let mut key = Vec::with_capacity(items.len());
            for item in items {
                key.push(self.evaluate_expression(node, &item.expression, schema, row)?);
            }
            keys.push(key);
        }
        Ok(keys)
    }

    fn evaluate_aggregate(
        &mut self,
        node: &NodeId,
        function: AggregateFunction,
        argument: Option<&Expression>,
        schema: &[Field],
        rows: &[Row],
        indices: &[usize],
    ) -> Result<Value, EvaluateError> {
        match function {
            AggregateFunction::CountAll => {
                let count = i64::try_from(indices.len())
                    .map_err(|_| evaluation_error(node, EvaluationErrorKind::IntegerOverflow))?;
                Ok(Value::Int64(count))
            }
            AggregateFunction::Count => {
                let argument = argument.expect("validated COUNT has an argument");
                let mut count = 0_i64;
                for index in indices {
                    let value = self.evaluate_expression(node, argument, schema, &rows[*index])?;
                    if !value.is_null() {
                        count = count.checked_add(1).ok_or_else(|| {
                            evaluation_error(node, EvaluationErrorKind::IntegerOverflow)
                        })?;
                    }
                }
                Ok(Value::Int64(count))
            }
            AggregateFunction::Sum => {
                let argument = argument.expect("validated SUM has an argument");
                let mut exact = 0_i128;
                let mut present = false;
                for index in indices {
                    match self.evaluate_expression(node, argument, schema, &rows[*index])? {
                        Value::Int64(value) => {
                            exact += i128::from(value);
                            present = true;
                        }
                        Value::Null => {}
                        _ => unreachable!("validated SUM argument is INT64"),
                    }
                }
                if !present {
                    return Ok(Value::Null);
                }
                let value = i64::try_from(exact)
                    .map_err(|_| evaluation_error(node, EvaluationErrorKind::IntegerOverflow))?;
                Ok(Value::Int64(value))
            }
            AggregateFunction::Min | AggregateFunction::Max => {
                let argument = argument.expect("validated MIN or MAX has an argument");
                let mut result: Option<Value> = None;
                for index in indices {
                    let value = self.evaluate_expression(node, argument, schema, &rows[*index])?;
                    if value.is_null() {
                        continue;
                    }
                    let replace = result.as_ref().is_none_or(|current| {
                        let ordering = compare_non_null(&value, current);
                        match function {
                            AggregateFunction::Min => ordering == Ordering::Less,
                            AggregateFunction::Max => ordering == Ordering::Greater,
                            _ => unreachable!(),
                        }
                    });
                    if replace {
                        result = Some(value);
                    }
                }
                Ok(result.unwrap_or(Value::Null))
            }
            AggregateFunction::BoolAnd | AggregateFunction::BoolOr => {
                let argument = argument.expect("validated Boolean aggregate has an argument");
                let mut present = false;
                let mut result = function == AggregateFunction::BoolAnd;
                for index in indices {
                    match self.evaluate_expression(node, argument, schema, &rows[*index])? {
                        Value::Boolean(value) => {
                            present = true;
                            match function {
                                AggregateFunction::BoolAnd => result &= value,
                                AggregateFunction::BoolOr => result |= value,
                                _ => unreachable!(),
                            }
                        }
                        Value::Null => {}
                        _ => unreachable!("validated Boolean aggregate argument is BOOLEAN"),
                    }
                }
                Ok(if present {
                    Value::Boolean(result)
                } else {
                    Value::Null
                })
            }
        }
    }

    fn evaluate_expression(
        &mut self,
        node: &NodeId,
        expression: &Expression,
        schema: &[Field],
        row: &Row,
    ) -> Result<Value, EvaluateError> {
        use ExpressionKind as K;
        match &expression.kind {
            K::Literal(value) => Ok(match value {
                LiteralValue::Boolean(value) => Value::Boolean(*value),
                LiteralValue::Int64(value) => Value::Int64(*value),
                LiteralValue::Text(value) => Value::Text(value.clone()),
                LiteralValue::Null => Value::Null,
            }),
            K::Field(field) => Ok(row.values[field_position(schema, field)].clone()),
            K::Unary { operation, operand } => {
                let value = self.evaluate_expression(node, operand, schema, row)?;
                self.evaluate_unary(node, *operation, value)
            }
            K::Binary {
                operation,
                left,
                right,
            } => self.evaluate_binary(node, *operation, left, right, schema, row),
            K::IsNull { operand, negated } => {
                let is_null = self
                    .evaluate_expression(node, operand, schema, row)?
                    .is_null();
                Ok(Value::Boolean(if *negated { !is_null } else { is_null }))
            }
            K::Case { arms, fallback } => {
                for arm in arms {
                    if self.evaluate_expression(node, &arm.when, schema, row)?
                        == Value::Boolean(true)
                    {
                        return self.evaluate_expression(node, &arm.then, schema, row);
                    }
                }
                self.evaluate_expression(node, fallback, schema, row)
            }
            K::Cast { operand, target } => {
                let value = self.evaluate_expression(node, operand, schema, row)?;
                self.evaluate_cast(node, value, *target)
            }
            K::InList { value, candidates } => {
                let value = self.evaluate_expression(node, value, schema, row)?;
                let mut matched = false;
                let mut unknown = false;
                for candidate in candidates {
                    let candidate = self.evaluate_expression(node, candidate, schema, row)?;
                    match ordinary_equal(&value, &candidate) {
                        Some(true) => matched = true,
                        Some(false) => {}
                        None => unknown = true,
                    }
                }
                Ok(if matched {
                    Value::Boolean(true)
                } else if unknown {
                    Value::Null
                } else {
                    Value::Boolean(false)
                })
            }
            K::Exists { query } => {
                let query = self.evaluate_node(query)?;
                Ok(Value::Boolean(!query.rows.is_empty()))
            }
            K::InQuery {
                value,
                query,
                field: _,
            } => {
                let value = self.evaluate_expression(node, value, schema, row)?;
                let query = self.evaluate_node(query)?;
                let mut matched = false;
                let mut unknown = false;
                for candidate in &query.rows {
                    match ordinary_equal(&value, &candidate.values[0]) {
                        Some(true) => matched = true,
                        Some(false) => {}
                        None => unknown = true,
                    }
                }
                Ok(if matched {
                    Value::Boolean(true)
                } else if unknown {
                    Value::Null
                } else {
                    Value::Boolean(false)
                })
            }
        }
    }

    fn evaluate_unary(
        &self,
        node: &NodeId,
        operation: UnaryOperation,
        value: Value,
    ) -> Result<Value, EvaluateError> {
        if value.is_null() {
            return Ok(Value::Null);
        }
        match (operation, value) {
            (UnaryOperation::Positive, Value::Int64(value)) => Ok(Value::Int64(value)),
            (UnaryOperation::Negative, Value::Int64(value)) => value
                .checked_neg()
                .map(Value::Int64)
                .ok_or_else(|| evaluation_error(node, EvaluationErrorKind::IntegerOverflow)),
            (UnaryOperation::Not, Value::Boolean(value)) => Ok(Value::Boolean(!value)),
            _ => unreachable!("validated unary expression has the required value type"),
        }
    }

    fn evaluate_binary(
        &mut self,
        node: &NodeId,
        operation: BinaryOperation,
        left: &Expression,
        right: &Expression,
        schema: &[Field],
        row: &Row,
    ) -> Result<Value, EvaluateError> {
        if operation == BinaryOperation::And {
            let left = self.evaluate_expression(node, left, schema, row)?;
            return match left {
                Value::Boolean(false) => Ok(Value::Boolean(false)),
                Value::Boolean(true) => self.evaluate_expression(node, right, schema, row),
                Value::Null => {
                    let right = self.evaluate_expression(node, right, schema, row)?;
                    Ok(match right {
                        Value::Boolean(false) => Value::Boolean(false),
                        Value::Boolean(true) | Value::Null => Value::Null,
                        _ => unreachable!("validated AND operand is BOOLEAN"),
                    })
                }
                _ => unreachable!("validated AND operand is BOOLEAN"),
            };
        }
        if operation == BinaryOperation::Or {
            let left = self.evaluate_expression(node, left, schema, row)?;
            return match left {
                Value::Boolean(true) => Ok(Value::Boolean(true)),
                Value::Boolean(false) => self.evaluate_expression(node, right, schema, row),
                Value::Null => {
                    let right = self.evaluate_expression(node, right, schema, row)?;
                    Ok(match right {
                        Value::Boolean(true) => Value::Boolean(true),
                        Value::Boolean(false) | Value::Null => Value::Null,
                        _ => unreachable!("validated OR operand is BOOLEAN"),
                    })
                }
                _ => unreachable!("validated OR operand is BOOLEAN"),
            };
        }

        // Every operand of a strict binary operator is required, even when the
        // first value is NULL.
        let left = self.evaluate_expression(node, left, schema, row)?;
        let right = self.evaluate_expression(node, right, schema, row)?;
        if left.is_null() || right.is_null() {
            return Ok(Value::Null);
        }

        let value =
            match operation {
                BinaryOperation::Add => {
                    let (left, right) = int_pair(left, right);
                    Value::Int64(left.checked_add(right).ok_or_else(|| {
                        evaluation_error(node, EvaluationErrorKind::IntegerOverflow)
                    })?)
                }
                BinaryOperation::Subtract => {
                    let (left, right) = int_pair(left, right);
                    Value::Int64(left.checked_sub(right).ok_or_else(|| {
                        evaluation_error(node, EvaluationErrorKind::IntegerOverflow)
                    })?)
                }
                BinaryOperation::Multiply => {
                    let (left, right) = int_pair(left, right);
                    Value::Int64(left.checked_mul(right).ok_or_else(|| {
                        evaluation_error(node, EvaluationErrorKind::IntegerOverflow)
                    })?)
                }
                BinaryOperation::Divide => {
                    let (left, right) = int_pair(left, right);
                    if right == 0 {
                        return Err(evaluation_error(node, EvaluationErrorKind::DivisionByZero));
                    }
                    Value::Int64(left.checked_div(right).ok_or_else(|| {
                        evaluation_error(node, EvaluationErrorKind::IntegerOverflow)
                    })?)
                }
                BinaryOperation::Remainder => {
                    let (left, right) = int_pair(left, right);
                    if right == 0 {
                        return Err(evaluation_error(node, EvaluationErrorKind::RemainderByZero));
                    }
                    // The exact remainder is zero even though MIN / -1 has an
                    // unrepresentable quotient.
                    Value::Int64(if right == -1 { 0 } else { left % right })
                }
                BinaryOperation::Concatenate => {
                    let (mut left, right) = text_pair(left, right);
                    left.push_str(&right);
                    Value::Text(left)
                }
                BinaryOperation::Equal => Value::Boolean(compare_non_null(&left, &right).is_eq()),
                BinaryOperation::NotEqual => {
                    Value::Boolean(!compare_non_null(&left, &right).is_eq())
                }
                BinaryOperation::Less => Value::Boolean(compare_non_null(&left, &right).is_lt()),
                BinaryOperation::LessOrEqual => {
                    Value::Boolean(!compare_non_null(&left, &right).is_gt())
                }
                BinaryOperation::Greater => Value::Boolean(compare_non_null(&left, &right).is_gt()),
                BinaryOperation::GreaterOrEqual => {
                    Value::Boolean(!compare_non_null(&left, &right).is_lt())
                }
                BinaryOperation::And | BinaryOperation::Or => unreachable!(),
            };
        Ok(value)
    }

    fn evaluate_cast(
        &self,
        node: &NodeId,
        value: Value,
        target: ScalarType,
    ) -> Result<Value, EvaluateError> {
        if value.is_null() {
            return Ok(Value::Null);
        }
        match (value, target) {
            (Value::Boolean(value), ScalarType::Boolean) => Ok(Value::Boolean(value)),
            (Value::Int64(value), ScalarType::Int64) => Ok(Value::Int64(value)),
            (Value::Text(value), ScalarType::Text) => Ok(Value::Text(value)),
            (Value::Int64(value), ScalarType::Text) => Ok(Value::Text(value.to_string())),
            (Value::Boolean(value), ScalarType::Text) => {
                Ok(Value::Text(if value { "TRUE" } else { "FALSE" }.into()))
            }
            (Value::Text(value), ScalarType::Int64) => parse_text_int64(&value)
                .map(Value::Int64)
                .ok_or_else(|| invalid_text_cast(node, ScalarType::Int64)),
            (Value::Text(value), ScalarType::Boolean) => {
                if value.eq_ignore_ascii_case("TRUE") {
                    Ok(Value::Boolean(true))
                } else if value.eq_ignore_ascii_case("FALSE") {
                    Ok(Value::Boolean(false))
                } else {
                    Err(invalid_text_cast(node, ScalarType::Boolean))
                }
            }
            _ => unreachable!("validated cast has a portable source-target pair"),
        }
    }
}

fn field_position(schema: &[Field], field: &FieldId) -> usize {
    schema
        .iter()
        .position(|candidate| candidate.id == *field)
        .expect("validated field reference occurs in its environment")
}

fn concatenate_rows(left: &Row, right: &Row) -> Row {
    let mut values = Vec::with_capacity(left.values.len() + right.values.len());
    values.extend(left.values.iter().cloned());
    values.extend(right.values.iter().cloned());
    Row::new(values)
}

fn row_counts(rows: &[Row]) -> Vec<(Row, usize)> {
    let mut counts: Vec<(Row, usize)> = Vec::new();
    for row in rows {
        if let Some((_, count)) = counts.iter_mut().find(|(candidate, _)| candidate == row) {
            *count += 1;
        } else {
            counts.push((row.clone(), 1));
        }
    }
    counts
}

fn take_one(counts: &mut [(Row, usize)], row: &Row) -> bool {
    let Some((_, count)) = counts
        .iter_mut()
        .find(|(candidate, count)| candidate == row && *count != 0)
    else {
        return false;
    };
    *count -= 1;
    true
}

fn compare_order_keys(left: &[Value], right: &[Value], items: &[OrderingItem]) -> Ordering {
    for ((left, right), item) in left.iter().zip(right).zip(items) {
        let ordering = compare_order_value(left, right, item);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn compare_order_value(left: &Value, right: &Value, item: &OrderingItem) -> Ordering {
    match (left, right) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => match item.null_placement {
            NullPlacement::First => Ordering::Less,
            NullPlacement::Last => Ordering::Greater,
            NullPlacement::NotApplicable => {
                unreachable!("a non-nullable ordering expression produced NULL")
            }
        },
        (_, Value::Null) => match item.null_placement {
            NullPlacement::First => Ordering::Greater,
            NullPlacement::Last => Ordering::Less,
            NullPlacement::NotApplicable => {
                unreachable!("a non-nullable ordering expression produced NULL")
            }
        },
        _ => {
            let ordering = compare_non_null(left, right);
            match item.direction {
                Direction::Ascending => ordering,
                Direction::Descending => ordering.reverse(),
            }
        }
    }
}

fn order_keys_equal(left: &[Value], right: &[Value]) -> bool {
    left == right
}

fn compare_non_null(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Boolean(left), Value::Boolean(right)) => left.cmp(right),
        (Value::Int64(left), Value::Int64(right)) => left.cmp(right),
        (Value::Text(left), Value::Text(right)) => left.as_bytes().cmp(right.as_bytes()),
        _ => unreachable!("validated comparison has equal non-null scalar types"),
    }
}

fn ordinary_equal(left: &Value, right: &Value) -> Option<bool> {
    if left.is_null() || right.is_null() {
        None
    } else {
        Some(compare_non_null(left, right) == Ordering::Equal)
    }
}

fn int_pair(left: Value, right: Value) -> (i64, i64) {
    match (left, right) {
        (Value::Int64(left), Value::Int64(right)) => (left, right),
        _ => unreachable!("validated arithmetic operands are INT64"),
    }
}

fn text_pair(left: Value, right: Value) -> (String, String) {
    match (left, right) {
        (Value::Text(left), Value::Text(right)) => (left, right),
        _ => unreachable!("validated concatenation operands are TEXT"),
    }
}

fn parse_text_int64(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    let digits = match bytes.first() {
        Some(b'+' | b'-') => &bytes[1..],
        Some(_) => bytes,
        None => return None,
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    value.parse().ok()
}

fn position_to_int64(node: &NodeId, zero_based: usize) -> Result<i64, EvaluateError> {
    zero_based
        .checked_add(1)
        .and_then(|position| i64::try_from(position).ok())
        .ok_or_else(|| evaluation_error(node, EvaluationErrorKind::IntegerOverflow))
}

fn invalid_text_cast(node: &NodeId, target: ScalarType) -> EvaluateError {
    evaluation_error(node, EvaluationErrorKind::InvalidTextCast { target })
}

fn evaluation_error(node: &NodeId, kind: EvaluationErrorKind) -> EvaluateError {
    EvaluationError {
        node: node.clone(),
        kind,
    }
    .into()
}
