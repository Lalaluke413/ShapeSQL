//! Shape IR 0.1 abstract graph and semantic validation.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::{RelationBinding, ScalarType, TypeDescriptor};

/// The Shape IR version implemented by this crate.
pub const VERSION: &str = "0.1";

macro_rules! string_identity {
    ($(#[$metadata:meta])* $name:ident) => {
        $(#[$metadata])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

string_identity!(
    /// A graph-wide relational node identity.
    NodeId
);
string_identity!(
    /// A graph-wide Shape IR field identity.
    FieldId
);

/// One complete Shape IR graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Graph {
    pub version: String,
    pub root: NodeId,
    pub nodes: Vec<Node>,
}

impl Graph {
    pub fn new(root: impl Into<NodeId>, nodes: Vec<Node>) -> Self {
        Self {
            version: VERSION.into(),
            root: root.into(),
            nodes,
        }
    }

    /// Validates every graph, node, schema, and expression invariant.
    pub fn validate(&self) -> Result<ValidationSummary, ValidationError> {
        validate(self)
    }
}

/// An ordered Shape IR schema field descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: FieldId,
    pub name: String,
    pub descriptor: TypeDescriptor,
}

impl Field {
    pub fn new(
        id: impl Into<FieldId>,
        name: impl Into<String>,
        descriptor: TypeDescriptor,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            descriptor,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionKind {
    Bag,
    Ordered,
}

/// One relational node with checked output annotations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub output_schema: Vec<Field>,
    pub collection: CollectionKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Input {
        binding: RelationBinding,
    },
    Empty,
    Project {
        input: NodeId,
        entries: Vec<ProjectEntry>,
    },
    Filter {
        input: NodeId,
        predicate: Expression,
    },
    Join {
        left: NodeId,
        right: NodeId,
        join_type: JoinType,
        condition: Option<Expression>,
    },
    Aggregate {
        input: NodeId,
        grouping_keys: Vec<GroupingKey>,
        aggregates: Vec<AggregateDefinition>,
    },
    Window {
        input: NodeId,
        definitions: Vec<WindowDefinition>,
    },
    Distinct {
        input: NodeId,
    },
    Set {
        left: NodeId,
        right: NodeId,
        operation: SetOperation,
        quantifier: SetQuantifier,
    },
    Order {
        input: NodeId,
        items: Vec<OrderingItem>,
    },
    Slice {
        input: NodeId,
        offset: i64,
        limit: Option<i64>,
    },
    ForgetOrder {
        input: NodeId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinType {
    Cross,
    Inner,
    Left,
    Right,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetOperation {
    Union,
    Intersect,
    Except,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetQuantifier {
    All,
    Distinct,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectEntry {
    Keep(FieldId),
    Compute {
        output: FieldId,
        expression: Expression,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupingKey {
    pub output: FieldId,
    pub expression: Expression,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AggregateDefinition {
    pub output: FieldId,
    pub function: AggregateFunction,
    pub argument: Option<Expression>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregateFunction {
    CountAll,
    Count,
    Sum,
    Min,
    Max,
    BoolAnd,
    BoolOr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowDefinition {
    PartitionedAggregate {
        output: FieldId,
        function: AggregateFunction,
        argument: Option<Expression>,
        partition_by: Vec<Expression>,
    },
    Ranking {
        output: FieldId,
        function: RankingFunction,
        partition_by: Vec<Expression>,
        order_by: Vec<OrderingItem>,
    },
}

impl WindowDefinition {
    pub fn output(&self) -> &FieldId {
        match self {
            Self::PartitionedAggregate { output, .. } | Self::Ranking { output, .. } => output,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankingFunction {
    RowNumber,
    Rank,
    DenseRank,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderingItem {
    pub expression: Expression,
    pub direction: Direction,
    pub null_placement: NullPlacement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NullPlacement {
    First,
    Last,
    NotApplicable,
}

/// A nested scalar expression carrying its checked descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub descriptor: TypeDescriptor,
}

impl Expression {
    pub fn new(kind: ExpressionKind, descriptor: TypeDescriptor) -> Self {
        Self { kind, descriptor }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpressionKind {
    Literal(LiteralValue),
    Field(FieldId),
    Unary {
        operation: UnaryOperation,
        operand: Box<Expression>,
    },
    Binary {
        operation: BinaryOperation,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    IsNull {
        operand: Box<Expression>,
        negated: bool,
    },
    Case {
        arms: Vec<CaseArm>,
        fallback: Box<Expression>,
    },
    Cast {
        operand: Box<Expression>,
        target: ScalarType,
    },
    InList {
        value: Box<Expression>,
        candidates: Vec<Expression>,
    },
    Exists {
        query: NodeId,
    },
    InQuery {
        value: Box<Expression>,
        query: NodeId,
        field: FieldId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiteralValue {
    Boolean(bool),
    Int64(i64),
    Text(String),
    Null,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOperation {
    Positive,
    Negative,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOperation {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Concatenate,
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    And,
    Or,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseArm {
    pub when: Expression,
    pub then: Expression,
}

/// Root properties derived by successful graph validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationSummary {
    pub root_schema: Vec<Field>,
    pub root_collection: CollectionKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub kind: ValidationErrorKind,
    pub node: Option<NodeId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationErrorKind {
    UnsupportedVersion(String),
    EmptyGraph,
    DuplicateNode(NodeId),
    MissingNode(NodeId),
    CyclicGraph(NodeId),
    UnreachableNode(NodeId),
    DuplicateField(FieldId),
    InvalidFieldReference(FieldId),
    IncorrectOutputSchema,
    IncorrectCollectionKind,
    IncorrectExpressionDescriptor,
    InvalidExpression,
    InvalidNode,
    InvalidAggregateSignature,
    InvalidOrdering,
    IncompleteRowNumberKey,
    IncompleteSliceKey,
    NegativeSliceBound,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ValidationErrorKind as K;
        match &self.kind {
            K::UnsupportedVersion(version) => {
                write!(formatter, "unsupported Shape IR version `{version}`")
            }
            K::EmptyGraph => formatter.write_str("Shape IR graph has no nodes"),
            K::DuplicateNode(node) => write!(formatter, "duplicate node `{node}`"),
            K::MissingNode(node) => write!(formatter, "missing node `{node}`"),
            K::CyclicGraph(node) => write!(formatter, "cycle through node `{node}`"),
            K::UnreachableNode(node) => write!(formatter, "unreachable node `{node}`"),
            K::DuplicateField(field) => write!(formatter, "duplicate field `{field}`"),
            K::InvalidFieldReference(field) => write!(formatter, "invalid field `{field}`"),
            K::IncorrectOutputSchema => formatter.write_str("incorrect output schema"),
            K::IncorrectCollectionKind => formatter.write_str("incorrect collection kind"),
            K::IncorrectExpressionDescriptor => {
                formatter.write_str("incorrect expression descriptor")
            }
            K::InvalidExpression => formatter.write_str("invalid scalar expression"),
            K::InvalidNode => formatter.write_str("invalid relational node"),
            K::InvalidAggregateSignature => formatter.write_str("invalid aggregate signature"),
            K::InvalidOrdering => formatter.write_str("invalid ordering item"),
            K::IncompleteRowNumberKey => {
                formatter.write_str("ROW_NUMBER ordering covers no input value key")
            }
            K::IncompleteSliceKey => formatter.write_str("slice peers cover no input value key"),
            K::NegativeSliceBound => formatter.write_str("slice bound is negative"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Validates one abstract Shape IR graph.
pub fn validate(graph: &Graph) -> Result<ValidationSummary, ValidationError> {
    Validator::new(graph)?.validate()
}

#[derive(Clone)]
struct NodeProperties {
    schema: Vec<Field>,
    collection: CollectionKind,
    value_keys: Vec<HashSet<FieldId>>,
    peer_constant: HashSet<FieldId>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Complete,
}

struct Validator<'a> {
    graph: &'a Graph,
    nodes: HashMap<NodeId, &'a Node>,
    states: HashMap<NodeId, VisitState>,
    properties: HashMap<NodeId, NodeProperties>,
    defined_fields: HashSet<FieldId>,
}

impl<'a> Validator<'a> {
    fn new(graph: &'a Graph) -> Result<Self, ValidationError> {
        if graph.version != VERSION {
            return Err(ValidationError {
                kind: ValidationErrorKind::UnsupportedVersion(graph.version.clone()),
                node: None,
            });
        }
        if graph.nodes.is_empty() {
            return Err(ValidationError {
                kind: ValidationErrorKind::EmptyGraph,
                node: None,
            });
        }
        let mut nodes = HashMap::new();
        for node in &graph.nodes {
            if nodes.insert(node.id.clone(), node).is_some() {
                return Err(ValidationError {
                    kind: ValidationErrorKind::DuplicateNode(node.id.clone()),
                    node: Some(node.id.clone()),
                });
            }
        }
        Ok(Self {
            graph,
            nodes,
            states: HashMap::new(),
            properties: HashMap::new(),
            defined_fields: HashSet::new(),
        })
    }

    fn validate(mut self) -> Result<ValidationSummary, ValidationError> {
        self.visit(&self.graph.root)?;
        for node in &self.graph.nodes {
            if !self.states.contains_key(&node.id) {
                return Err(ValidationError {
                    kind: ValidationErrorKind::UnreachableNode(node.id.clone()),
                    node: Some(node.id.clone()),
                });
            }
        }
        let root = self
            .properties
            .get(&self.graph.root)
            .expect("visited root has properties");
        Ok(ValidationSummary {
            root_schema: root.schema.clone(),
            root_collection: root.collection,
        })
    }

    fn visit(&mut self, id: &NodeId) -> Result<(), ValidationError> {
        match self.states.get(id) {
            Some(VisitState::Complete) => return Ok(()),
            Some(VisitState::Visiting) => {
                return Err(self.error(id, ValidationErrorKind::CyclicGraph(id.clone())));
            }
            None => {}
        }
        let node = self.nodes.get(id).copied().ok_or_else(|| ValidationError {
            kind: ValidationErrorKind::MissingNode(id.clone()),
            node: None,
        })?;
        self.states.insert(id.clone(), VisitState::Visiting);
        let dependencies = node_dependencies(node);
        for dependency in dependencies {
            self.visit(&dependency)?;
        }
        let properties = self.validate_node(node)?;
        self.properties.insert(id.clone(), properties);
        self.states.insert(id.clone(), VisitState::Complete);
        Ok(())
    }

    fn validate_node(&mut self, node: &Node) -> Result<NodeProperties, ValidationError> {
        ensure_unique_schema(&node.output_schema).map_err(|kind| self.error(&node.id, kind))?;

        let mut properties = match &node.kind {
            NodeKind::Input { binding } => {
                if binding.as_str().is_empty() {
                    return Err(self.error(&node.id, ValidationErrorKind::InvalidNode));
                }
                self.define_fields(&node.id, &node.output_schema)?;
                source_properties(node.output_schema.clone())
            }
            NodeKind::Empty => {
                self.define_fields(&node.id, &node.output_schema)?;
                source_properties(node.output_schema.clone())
            }
            NodeKind::Project { input, entries } => self.validate_project(node, input, entries)?,
            NodeKind::Filter { input, predicate } => {
                let input = self.property(input, &node.id)?.clone();
                let descriptor = self.validate_expression(predicate, &input.schema, &node.id)?;
                if descriptor.scalar != ScalarType::Boolean {
                    return Err(self.error(&node.id, ValidationErrorKind::InvalidExpression));
                }
                input
            }
            NodeKind::Join {
                left,
                right,
                join_type,
                condition,
            } => self.validate_join(node, left, right, *join_type, condition.as_ref())?,
            NodeKind::Aggregate {
                input,
                grouping_keys,
                aggregates,
            } => self.validate_aggregate(node, input, grouping_keys, aggregates)?,
            NodeKind::Window { input, definitions } => {
                self.validate_window(node, input, definitions)?
            }
            NodeKind::Distinct { input } => {
                let input = self.property(input, &node.id)?;
                NodeProperties {
                    schema: input.schema.clone(),
                    collection: CollectionKind::Bag,
                    value_keys: complete_key(&input.schema),
                    peer_constant: HashSet::new(),
                }
            }
            NodeKind::Set {
                left,
                right,
                operation: _,
                quantifier: _,
            } => self.validate_set(node, left, right)?,
            NodeKind::Order { input, items } => self.validate_order(node, input, items)?,
            NodeKind::Slice {
                input,
                offset,
                limit,
            } => self.validate_slice(node, input, *offset, *limit)?,
            NodeKind::ForgetOrder { input } => {
                let input = self.property(input, &node.id)?;
                if input.collection != CollectionKind::Ordered {
                    return Err(self.error(&node.id, ValidationErrorKind::InvalidNode));
                }
                NodeProperties {
                    schema: input.schema.clone(),
                    collection: CollectionKind::Bag,
                    value_keys: input.value_keys.clone(),
                    peer_constant: HashSet::new(),
                }
            }
        };

        if properties.schema != node.output_schema {
            return Err(self.error(&node.id, ValidationErrorKind::IncorrectOutputSchema));
        }
        if properties.collection != node.collection {
            return Err(self.error(&node.id, ValidationErrorKind::IncorrectCollectionKind));
        }
        add_complete_key(&mut properties.value_keys, &properties.schema);
        Ok(properties)
    }

    fn validate_project(
        &mut self,
        node: &Node,
        input_id: &NodeId,
        entries: &[ProjectEntry],
    ) -> Result<NodeProperties, ValidationError> {
        let input = self.property(input_id, &node.id)?.clone();
        if entries.len() != node.output_schema.len() {
            return Err(self.error(&node.id, ValidationErrorKind::IncorrectOutputSchema));
        }
        let input_map = schema_map(&input.schema);
        let mut kept = HashSet::new();
        let mut schema = Vec::with_capacity(entries.len());
        let mut peer_constant = HashSet::new();
        for (entry, declared) in entries.iter().zip(&node.output_schema) {
            match entry {
                ProjectEntry::Keep(field) => {
                    if !kept.insert(field.clone()) {
                        return Err(self.error(&node.id, ValidationErrorKind::InvalidNode));
                    }
                    let source = input_map.get(field).ok_or_else(|| {
                        self.error(
                            &node.id,
                            ValidationErrorKind::InvalidFieldReference(field.clone()),
                        )
                    })?;
                    schema.push((*source).clone());
                    if input.peer_constant.contains(field) {
                        peer_constant.insert(field.clone());
                    }
                }
                ProjectEntry::Compute { output, expression } => {
                    if declared.id != *output {
                        return Err(
                            self.error(&node.id, ValidationErrorKind::IncorrectOutputSchema)
                        );
                    }
                    let descriptor =
                        self.validate_expression(expression, &input.schema, &node.id)?;
                    let field = Field {
                        id: output.clone(),
                        name: declared.name.clone(),
                        descriptor,
                    };
                    self.define_field(&node.id, &field.id)?;
                    if referenced_fields(expression)
                        .iter()
                        .all(|field| input.peer_constant.contains(field))
                    {
                        peer_constant.insert(output.clone());
                    }
                    schema.push(field);
                }
            }
        }

        let mut value_keys = Vec::new();
        for key in &input.value_keys {
            if key.iter().all(|field| kept.contains(field)) {
                value_keys.push(key.clone());
            }
        }
        Ok(NodeProperties {
            schema,
            collection: input.collection,
            value_keys,
            peer_constant,
        })
    }

    fn validate_join(
        &mut self,
        node: &Node,
        left_id: &NodeId,
        right_id: &NodeId,
        join_type: JoinType,
        condition: Option<&Expression>,
    ) -> Result<NodeProperties, ValidationError> {
        let left = self.property(left_id, &node.id)?.clone();
        let right = self.property(right_id, &node.id)?.clone();
        let left_ids = field_ids(&left.schema);
        if right
            .schema
            .iter()
            .any(|field| left_ids.contains(&field.id))
        {
            return Err(self.error(
                &node.id,
                ValidationErrorKind::DuplicateField(
                    right
                        .schema
                        .iter()
                        .find(|field| left_ids.contains(&field.id))
                        .expect("overlap")
                        .id
                        .clone(),
                ),
            ));
        }
        let mut environment = left.schema.clone();
        environment.extend(right.schema.iter().cloned());
        match (join_type, condition) {
            (JoinType::Cross, None) => {}
            (JoinType::Cross, Some(_)) | (_, None) => {
                return Err(self.error(&node.id, ValidationErrorKind::InvalidNode));
            }
            (_, Some(condition)) => {
                let descriptor = self.validate_expression(condition, &environment, &node.id)?;
                if descriptor.scalar != ScalarType::Boolean {
                    return Err(self.error(&node.id, ValidationErrorKind::InvalidExpression));
                }
            }
        }

        let mut schema = match join_type {
            JoinType::Cross | JoinType::Inner => environment,
            JoinType::Left => {
                let mut schema = left.schema;
                schema.extend(nullable_fields(right.schema));
                schema
            }
            JoinType::Right => {
                let mut schema = nullable_fields(left.schema);
                schema.extend(right.schema);
                schema
            }
            JoinType::Full => {
                let mut schema = nullable_fields(left.schema);
                schema.extend(nullable_fields(right.schema));
                schema
            }
        };
        // `environment` may have moved above; every branch has the same order.
        schema.shrink_to_fit();
        Ok(NodeProperties {
            value_keys: complete_key(&schema),
            schema,
            collection: CollectionKind::Bag,
            peer_constant: HashSet::new(),
        })
    }

    fn validate_aggregate(
        &mut self,
        node: &Node,
        input_id: &NodeId,
        grouping_keys: &[GroupingKey],
        aggregates: &[AggregateDefinition],
    ) -> Result<NodeProperties, ValidationError> {
        let input = self.property(input_id, &node.id)?.clone();
        if node.output_schema.len() != grouping_keys.len() + aggregates.len() {
            return Err(self.error(&node.id, ValidationErrorKind::IncorrectOutputSchema));
        }
        let mut schema = Vec::with_capacity(node.output_schema.len());
        let mut grouping_ids = HashSet::new();
        for (definition, declared) in grouping_keys.iter().zip(&node.output_schema) {
            if definition.output != declared.id {
                return Err(self.error(&node.id, ValidationErrorKind::IncorrectOutputSchema));
            }
            let descriptor =
                self.validate_expression(&definition.expression, &input.schema, &node.id)?;
            self.define_field(&node.id, &definition.output)?;
            grouping_ids.insert(definition.output.clone());
            schema.push(Field {
                id: definition.output.clone(),
                name: declared.name.clone(),
                descriptor,
            });
        }
        for (definition, declared) in aggregates
            .iter()
            .zip(&node.output_schema[grouping_keys.len()..])
        {
            if definition.output != declared.id {
                return Err(self.error(&node.id, ValidationErrorKind::IncorrectOutputSchema));
            }
            let descriptor = self.validate_aggregate_definition(
                definition.function,
                definition.argument.as_ref(),
                &input.schema,
                &node.id,
            )?;
            self.define_field(&node.id, &definition.output)?;
            schema.push(Field {
                id: definition.output.clone(),
                name: declared.name.clone(),
                descriptor,
            });
        }
        Ok(NodeProperties {
            schema,
            collection: CollectionKind::Bag,
            value_keys: vec![grouping_ids],
            peer_constant: HashSet::new(),
        })
    }

    fn validate_window(
        &mut self,
        node: &Node,
        input_id: &NodeId,
        definitions: &[WindowDefinition],
    ) -> Result<NodeProperties, ValidationError> {
        let input = self.property(input_id, &node.id)?.clone();
        if definitions.is_empty()
            || node.output_schema.len() != input.schema.len() + definitions.len()
        {
            return Err(self.error(&node.id, ValidationErrorKind::InvalidNode));
        }
        let mut schema = input.schema.clone();
        for (definition, declared) in definitions
            .iter()
            .zip(&node.output_schema[input.schema.len()..])
        {
            if definition.output() != &declared.id {
                return Err(self.error(&node.id, ValidationErrorKind::IncorrectOutputSchema));
            }
            let descriptor = match definition {
                WindowDefinition::PartitionedAggregate {
                    function,
                    argument,
                    partition_by,
                    ..
                } => {
                    for expression in partition_by {
                        self.validate_expression(expression, &input.schema, &node.id)?;
                    }
                    self.validate_aggregate_definition(
                        *function,
                        argument.as_ref(),
                        &input.schema,
                        &node.id,
                    )?
                }
                WindowDefinition::Ranking {
                    function,
                    partition_by,
                    order_by,
                    ..
                } => {
                    for expression in partition_by {
                        self.validate_expression(expression, &input.schema, &node.id)?;
                    }
                    if order_by.is_empty() {
                        return Err(self.error(&node.id, ValidationErrorKind::InvalidOrdering));
                    }
                    self.validate_ordering_items(order_by, &input.schema, &node.id)?;
                    if *function == RankingFunction::RowNumber {
                        let direct = order_by
                            .iter()
                            .filter_map(|item| direct_expression_field(&item.expression))
                            .collect::<HashSet<_>>();
                        if !covers_value_key(&direct, &input.value_keys) {
                            return Err(
                                self.error(&node.id, ValidationErrorKind::IncompleteRowNumberKey)
                            );
                        }
                    }
                    TypeDescriptor::non_nullable(ScalarType::Int64)
                }
            };
            self.define_field(&node.id, definition.output())?;
            schema.push(Field {
                id: definition.output().clone(),
                name: declared.name.clone(),
                descriptor,
            });
        }
        Ok(NodeProperties {
            value_keys: complete_key(&schema),
            schema,
            collection: CollectionKind::Bag,
            peer_constant: HashSet::new(),
        })
    }

    fn validate_set(
        &mut self,
        node: &Node,
        left_id: &NodeId,
        right_id: &NodeId,
    ) -> Result<NodeProperties, ValidationError> {
        let left = self.property(left_id, &node.id)?.clone();
        let right = self.property(right_id, &node.id)?.clone();
        if left.schema.len() != right.schema.len() || left.schema.len() != node.output_schema.len()
        {
            return Err(self.error(&node.id, ValidationErrorKind::InvalidNode));
        }
        let mut schema = Vec::with_capacity(left.schema.len());
        for ((left, right), declared) in left
            .schema
            .iter()
            .zip(&right.schema)
            .zip(&node.output_schema)
        {
            if left.descriptor.scalar != right.descriptor.scalar {
                return Err(self.error(&node.id, ValidationErrorKind::InvalidNode));
            }
            let field = Field {
                id: declared.id.clone(),
                name: left.name.clone(),
                descriptor: TypeDescriptor::new(
                    left.descriptor.scalar,
                    left.descriptor.nullable || right.descriptor.nullable,
                ),
            };
            self.define_field(&node.id, &field.id)?;
            schema.push(field);
        }
        Ok(NodeProperties {
            value_keys: complete_key(&schema),
            schema,
            collection: CollectionKind::Bag,
            peer_constant: HashSet::new(),
        })
    }

    fn validate_order(
        &mut self,
        node: &Node,
        input_id: &NodeId,
        items: &[OrderingItem],
    ) -> Result<NodeProperties, ValidationError> {
        let input = self.property(input_id, &node.id)?.clone();
        if items.is_empty() {
            return Err(self.error(&node.id, ValidationErrorKind::InvalidOrdering));
        }
        self.validate_ordering_items(items, &input.schema, &node.id)?;
        let peer_constant = items
            .iter()
            .filter_map(|item| direct_expression_field(&item.expression))
            .collect();
        Ok(NodeProperties {
            schema: input.schema,
            collection: CollectionKind::Ordered,
            value_keys: input.value_keys,
            peer_constant,
        })
    }

    fn validate_slice(
        &self,
        node: &Node,
        input_id: &NodeId,
        offset: i64,
        limit: Option<i64>,
    ) -> Result<NodeProperties, ValidationError> {
        let input = self.property(input_id, &node.id)?;
        if input.collection != CollectionKind::Ordered {
            return Err(self.error(&node.id, ValidationErrorKind::InvalidNode));
        }
        if offset < 0 || limit.is_some_and(|limit| limit < 0) {
            return Err(self.error(&node.id, ValidationErrorKind::NegativeSliceBound));
        }
        if !covers_value_key(&input.peer_constant, &input.value_keys) {
            return Err(self.error(&node.id, ValidationErrorKind::IncompleteSliceKey));
        }
        Ok(input.clone())
    }

    fn validate_ordering_items(
        &mut self,
        items: &[OrderingItem],
        environment: &[Field],
        node: &NodeId,
    ) -> Result<(), ValidationError> {
        for item in items {
            let descriptor = self.validate_expression(&item.expression, environment, node)?;
            if descriptor.nullable && item.null_placement == NullPlacement::NotApplicable {
                return Err(self.error(node, ValidationErrorKind::InvalidOrdering));
            }
        }
        Ok(())
    }

    fn validate_aggregate_definition(
        &mut self,
        function: AggregateFunction,
        argument: Option<&Expression>,
        environment: &[Field],
        node: &NodeId,
    ) -> Result<TypeDescriptor, ValidationError> {
        if function == AggregateFunction::CountAll {
            if argument.is_some() {
                return Err(self.error(node, ValidationErrorKind::InvalidAggregateSignature));
            }
            return Ok(TypeDescriptor::non_nullable(ScalarType::Int64));
        }
        let argument = argument
            .ok_or_else(|| self.error(node, ValidationErrorKind::InvalidAggregateSignature))?;
        let descriptor = self.validate_expression(argument, environment, node)?;
        let result = match function {
            AggregateFunction::CountAll => unreachable!(),
            AggregateFunction::Count => TypeDescriptor::non_nullable(ScalarType::Int64),
            AggregateFunction::Sum if descriptor.scalar == ScalarType::Int64 => {
                TypeDescriptor::nullable(ScalarType::Int64)
            }
            AggregateFunction::Min | AggregateFunction::Max => {
                TypeDescriptor::nullable(descriptor.scalar)
            }
            AggregateFunction::BoolAnd | AggregateFunction::BoolOr
                if descriptor.scalar == ScalarType::Boolean =>
            {
                TypeDescriptor::nullable(ScalarType::Boolean)
            }
            _ => {
                return Err(self.error(node, ValidationErrorKind::InvalidAggregateSignature));
            }
        };
        Ok(result)
    }

    fn validate_expression(
        &mut self,
        expression: &Expression,
        environment: &[Field],
        node: &NodeId,
    ) -> Result<TypeDescriptor, ValidationError> {
        use ExpressionKind as K;
        let environment_map = schema_map(environment);
        let derived = match &expression.kind {
            K::Literal(value) => match value {
                LiteralValue::Boolean(_) => TypeDescriptor::non_nullable(ScalarType::Boolean),
                LiteralValue::Int64(_) => TypeDescriptor::non_nullable(ScalarType::Int64),
                LiteralValue::Text(_) => TypeDescriptor::non_nullable(ScalarType::Text),
                LiteralValue::Null => TypeDescriptor::nullable(expression.descriptor.scalar),
            },
            K::Field(field) => environment_map
                .get(field)
                .map(|field| field.descriptor)
                .ok_or_else(|| {
                    self.error(
                        node,
                        ValidationErrorKind::InvalidFieldReference(field.clone()),
                    )
                })?,
            K::Unary { operation, operand } => {
                let operand = self.validate_expression(operand, environment, node)?;
                let required = match operation {
                    UnaryOperation::Positive | UnaryOperation::Negative => ScalarType::Int64,
                    UnaryOperation::Not => ScalarType::Boolean,
                };
                if operand.scalar != required {
                    return Err(self.error(node, ValidationErrorKind::InvalidExpression));
                }
                TypeDescriptor::new(required, operand.nullable)
            }
            K::Binary {
                operation,
                left,
                right,
            } => {
                let left = self.validate_expression(left, environment, node)?;
                let right = self.validate_expression(right, environment, node)?;
                validate_binary(*operation, left, right).map_err(|kind| self.error(node, kind))?
            }
            K::IsNull { operand, .. } => {
                self.validate_expression(operand, environment, node)?;
                TypeDescriptor::non_nullable(ScalarType::Boolean)
            }
            K::Case { arms, fallback } => {
                if arms.is_empty() {
                    return Err(self.error(node, ValidationErrorKind::InvalidExpression));
                }
                let fallback = self.validate_expression(fallback, environment, node)?;
                let mut nullable = fallback.nullable;
                for arm in arms {
                    let predicate = self.validate_expression(&arm.when, environment, node)?;
                    if predicate.scalar != ScalarType::Boolean {
                        return Err(self.error(node, ValidationErrorKind::InvalidExpression));
                    }
                    let result = self.validate_expression(&arm.then, environment, node)?;
                    if result.scalar != fallback.scalar {
                        return Err(self.error(node, ValidationErrorKind::InvalidExpression));
                    }
                    nullable |= result.nullable;
                }
                TypeDescriptor::new(fallback.scalar, nullable)
            }
            K::Cast { operand, target } => {
                let operand = self.validate_expression(operand, environment, node)?;
                if !cast_permitted(operand.scalar, *target) {
                    return Err(self.error(node, ValidationErrorKind::InvalidExpression));
                }
                TypeDescriptor::new(*target, operand.nullable)
            }
            K::InList { value, candidates } => {
                if candidates.is_empty() {
                    return Err(self.error(node, ValidationErrorKind::InvalidExpression));
                }
                let value = self.validate_expression(value, environment, node)?;
                let mut nullable = value.nullable;
                for candidate in candidates {
                    let candidate = self.validate_expression(candidate, environment, node)?;
                    if candidate.scalar != value.scalar {
                        return Err(self.error(node, ValidationErrorKind::InvalidExpression));
                    }
                    nullable |= candidate.nullable;
                }
                TypeDescriptor::new(ScalarType::Boolean, nullable)
            }
            K::Exists { query } => {
                self.property(query, node)?;
                TypeDescriptor::non_nullable(ScalarType::Boolean)
            }
            K::InQuery {
                value,
                query,
                field,
            } => {
                let value = self.validate_expression(value, environment, node)?;
                let query = self.property(query, node)?;
                if query.schema.len() != 1
                    || query.schema[0].id != *field
                    || query.schema[0].descriptor.scalar != value.scalar
                {
                    return Err(self.error(node, ValidationErrorKind::InvalidExpression));
                }
                TypeDescriptor::new(
                    ScalarType::Boolean,
                    value.nullable || query.schema[0].descriptor.nullable,
                )
            }
        };
        if derived != expression.descriptor {
            return Err(self.error(node, ValidationErrorKind::IncorrectExpressionDescriptor));
        }
        Ok(derived)
    }

    fn property(
        &self,
        dependency: &NodeId,
        current: &NodeId,
    ) -> Result<&NodeProperties, ValidationError> {
        self.properties.get(dependency).ok_or_else(|| {
            if self.nodes.contains_key(dependency) {
                self.error(
                    current,
                    ValidationErrorKind::CyclicGraph(dependency.clone()),
                )
            } else {
                self.error(
                    current,
                    ValidationErrorKind::MissingNode(dependency.clone()),
                )
            }
        })
    }

    fn define_fields(&mut self, node: &NodeId, fields: &[Field]) -> Result<(), ValidationError> {
        for field in fields {
            self.define_field(node, &field.id)?;
        }
        Ok(())
    }

    fn define_field(&mut self, node: &NodeId, field: &FieldId) -> Result<(), ValidationError> {
        if !self.defined_fields.insert(field.clone()) {
            return Err(self.error(node, ValidationErrorKind::DuplicateField(field.clone())));
        }
        Ok(())
    }

    fn error(&self, node: &NodeId, kind: ValidationErrorKind) -> ValidationError {
        ValidationError {
            kind,
            node: Some(node.clone()),
        }
    }
}

fn source_properties(schema: Vec<Field>) -> NodeProperties {
    NodeProperties {
        value_keys: complete_key(&schema),
        schema,
        collection: CollectionKind::Bag,
        peer_constant: HashSet::new(),
    }
}

fn complete_key(schema: &[Field]) -> Vec<HashSet<FieldId>> {
    vec![field_ids(schema)]
}

fn add_complete_key(keys: &mut Vec<HashSet<FieldId>>, schema: &[Field]) {
    let complete = field_ids(schema);
    if !keys.contains(&complete) {
        keys.push(complete);
    }
}

fn covers_value_key(fields: &HashSet<FieldId>, keys: &[HashSet<FieldId>]) -> bool {
    keys.iter().any(|key| key.is_subset(fields))
}

fn field_ids(schema: &[Field]) -> HashSet<FieldId> {
    schema.iter().map(|field| field.id.clone()).collect()
}

fn schema_map(schema: &[Field]) -> HashMap<FieldId, &Field> {
    schema
        .iter()
        .map(|field| (field.id.clone(), field))
        .collect()
}

fn ensure_unique_schema(schema: &[Field]) -> Result<(), ValidationErrorKind> {
    let mut fields = HashSet::new();
    for field in schema {
        if !fields.insert(field.id.clone()) {
            return Err(ValidationErrorKind::DuplicateField(field.id.clone()));
        }
    }
    Ok(())
}

fn nullable_fields(fields: Vec<Field>) -> Vec<Field> {
    fields
        .into_iter()
        .map(|field| Field {
            descriptor: field.descriptor.with_nullable(true),
            ..field
        })
        .collect()
}

fn validate_binary(
    operation: BinaryOperation,
    left: TypeDescriptor,
    right: TypeDescriptor,
) -> Result<TypeDescriptor, ValidationErrorKind> {
    let nullable = left.nullable || right.nullable;
    let result = match operation {
        BinaryOperation::Add
        | BinaryOperation::Subtract
        | BinaryOperation::Multiply
        | BinaryOperation::Divide
        | BinaryOperation::Remainder
            if left.scalar == ScalarType::Int64 && right.scalar == ScalarType::Int64 =>
        {
            TypeDescriptor::new(ScalarType::Int64, nullable)
        }
        BinaryOperation::Concatenate
            if left.scalar == ScalarType::Text && right.scalar == ScalarType::Text =>
        {
            TypeDescriptor::new(ScalarType::Text, nullable)
        }
        BinaryOperation::And | BinaryOperation::Or
            if left.scalar == ScalarType::Boolean && right.scalar == ScalarType::Boolean =>
        {
            TypeDescriptor::new(ScalarType::Boolean, nullable)
        }
        BinaryOperation::Equal
        | BinaryOperation::NotEqual
        | BinaryOperation::Less
        | BinaryOperation::LessOrEqual
        | BinaryOperation::Greater
        | BinaryOperation::GreaterOrEqual
            if left.scalar == right.scalar =>
        {
            TypeDescriptor::new(ScalarType::Boolean, nullable)
        }
        _ => return Err(ValidationErrorKind::InvalidExpression),
    };
    Ok(result)
}

fn cast_permitted(source: ScalarType, target: ScalarType) -> bool {
    source == target
        || matches!(
            (source, target),
            (ScalarType::Int64, ScalarType::Text)
                | (ScalarType::Text, ScalarType::Int64)
                | (ScalarType::Boolean, ScalarType::Text)
                | (ScalarType::Text, ScalarType::Boolean)
        )
}

fn direct_expression_field(expression: &Expression) -> Option<FieldId> {
    match &expression.kind {
        ExpressionKind::Field(field) => Some(field.clone()),
        _ => None,
    }
}

fn referenced_fields(expression: &Expression) -> HashSet<FieldId> {
    let mut fields = HashSet::new();
    collect_referenced_fields(expression, &mut fields);
    fields
}

fn collect_referenced_fields(expression: &Expression, fields: &mut HashSet<FieldId>) {
    use ExpressionKind as K;
    match &expression.kind {
        K::Field(field) => {
            fields.insert(field.clone());
        }
        K::Unary { operand, .. } | K::IsNull { operand, .. } | K::Cast { operand, .. } => {
            collect_referenced_fields(operand, fields)
        }
        K::Binary { left, right, .. } => {
            collect_referenced_fields(left, fields);
            collect_referenced_fields(right, fields);
        }
        K::Case { arms, fallback } => {
            for arm in arms {
                collect_referenced_fields(&arm.when, fields);
                collect_referenced_fields(&arm.then, fields);
            }
            collect_referenced_fields(fallback, fields);
        }
        K::InList { value, candidates } => {
            collect_referenced_fields(value, fields);
            for candidate in candidates {
                collect_referenced_fields(candidate, fields);
            }
        }
        K::InQuery { value, .. } => collect_referenced_fields(value, fields),
        K::Literal(_) | K::Exists { .. } => {}
    }
}

fn node_dependencies(node: &Node) -> Vec<NodeId> {
    let mut dependencies = Vec::new();
    match &node.kind {
        NodeKind::Input { .. } | NodeKind::Empty => {}
        NodeKind::Project { input, entries } => {
            dependencies.push(input.clone());
            for entry in entries {
                if let ProjectEntry::Compute { expression, .. } = entry {
                    expression_dependencies(expression, &mut dependencies);
                }
            }
        }
        NodeKind::Filter { input, predicate } => {
            dependencies.push(input.clone());
            expression_dependencies(predicate, &mut dependencies);
        }
        NodeKind::Join {
            left,
            right,
            condition,
            ..
        } => {
            dependencies.push(left.clone());
            dependencies.push(right.clone());
            if let Some(condition) = condition {
                expression_dependencies(condition, &mut dependencies);
            }
        }
        NodeKind::Aggregate {
            input,
            grouping_keys,
            aggregates,
        } => {
            dependencies.push(input.clone());
            for key in grouping_keys {
                expression_dependencies(&key.expression, &mut dependencies);
            }
            for aggregate in aggregates {
                if let Some(argument) = &aggregate.argument {
                    expression_dependencies(argument, &mut dependencies);
                }
            }
        }
        NodeKind::Window { input, definitions } => {
            dependencies.push(input.clone());
            for definition in definitions {
                match definition {
                    WindowDefinition::PartitionedAggregate {
                        argument,
                        partition_by,
                        ..
                    } => {
                        if let Some(argument) = argument {
                            expression_dependencies(argument, &mut dependencies);
                        }
                        for expression in partition_by {
                            expression_dependencies(expression, &mut dependencies);
                        }
                    }
                    WindowDefinition::Ranking {
                        partition_by,
                        order_by,
                        ..
                    } => {
                        for expression in partition_by {
                            expression_dependencies(expression, &mut dependencies);
                        }
                        for item in order_by {
                            expression_dependencies(&item.expression, &mut dependencies);
                        }
                    }
                }
            }
        }
        NodeKind::Distinct { input }
        | NodeKind::Slice { input, .. }
        | NodeKind::ForgetOrder { input } => dependencies.push(input.clone()),
        NodeKind::Set { left, right, .. } => {
            dependencies.push(left.clone());
            dependencies.push(right.clone());
        }
        NodeKind::Order { input, items } => {
            dependencies.push(input.clone());
            for item in items {
                expression_dependencies(&item.expression, &mut dependencies);
            }
        }
    }
    dependencies
}

fn expression_dependencies(expression: &Expression, dependencies: &mut Vec<NodeId>) {
    use ExpressionKind as K;
    match &expression.kind {
        K::Unary { operand, .. } | K::IsNull { operand, .. } | K::Cast { operand, .. } => {
            expression_dependencies(operand, dependencies)
        }
        K::Binary { left, right, .. } => {
            expression_dependencies(left, dependencies);
            expression_dependencies(right, dependencies);
        }
        K::Case { arms, fallback } => {
            for arm in arms {
                expression_dependencies(&arm.when, dependencies);
                expression_dependencies(&arm.then, dependencies);
            }
            expression_dependencies(fallback, dependencies);
        }
        K::InList { value, candidates } => {
            expression_dependencies(value, dependencies);
            for candidate in candidates {
                expression_dependencies(candidate, dependencies);
            }
        }
        K::Exists { query } => dependencies.push(query.clone()),
        K::InQuery { value, query, .. } => {
            expression_dependencies(value, dependencies);
            dependencies.push(query.clone());
        }
        K::Literal(_) | K::Field(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int_field(id: &str) -> Field {
        Field::new(id, "value", TypeDescriptor::non_nullable(ScalarType::Int64))
    }

    #[test]
    fn validates_a_minimal_input_graph() {
        let graph = Graph::new(
            "n0",
            vec![Node {
                id: "n0".into(),
                kind: NodeKind::Input {
                    binding: "rows".into(),
                },
                output_schema: vec![int_field("f0")],
                collection: CollectionKind::Bag,
            }],
        );

        let summary = graph.validate().unwrap();
        assert_eq!(summary.root_schema, vec![int_field("f0")]);
        assert_eq!(summary.root_collection, CollectionKind::Bag);
    }

    #[test]
    fn rejects_unreachable_nodes() {
        let graph = Graph::new(
            "n0",
            vec![
                Node {
                    id: "n0".into(),
                    kind: NodeKind::Empty,
                    output_schema: vec![],
                    collection: CollectionKind::Bag,
                },
                Node {
                    id: "n1".into(),
                    kind: NodeKind::Empty,
                    output_schema: vec![],
                    collection: CollectionKind::Bag,
                },
            ],
        );

        assert!(matches!(
            graph.validate().unwrap_err().kind,
            ValidationErrorKind::UnreachableNode(_)
        ));
    }

    #[test]
    fn includes_demand_edges_in_reachability_and_cycle_checks() {
        let exists = Expression::new(
            ExpressionKind::Exists { query: "n1".into() },
            TypeDescriptor::non_nullable(ScalarType::Boolean),
        );
        let graph = Graph::new(
            "n0",
            vec![
                Node {
                    id: "n0".into(),
                    kind: NodeKind::Filter {
                        input: "n1".into(),
                        predicate: exists,
                    },
                    output_schema: vec![],
                    collection: CollectionKind::Bag,
                },
                Node {
                    id: "n1".into(),
                    kind: NodeKind::Filter {
                        input: "n0".into(),
                        predicate: Expression::new(
                            ExpressionKind::Literal(LiteralValue::Boolean(true)),
                            TypeDescriptor::non_nullable(ScalarType::Boolean),
                        ),
                    },
                    output_schema: vec![],
                    collection: CollectionKind::Bag,
                },
            ],
        );

        assert!(matches!(
            graph.validate().unwrap_err().kind,
            ValidationErrorKind::CyclicGraph(_)
        ));
    }

    #[test]
    fn slice_requires_ordering_fields_to_cover_a_value_key() {
        let input = Node {
            id: "n0".into(),
            kind: NodeKind::Input {
                binding: "rows".into(),
            },
            output_schema: vec![int_field("f0"), int_field("f1")],
            collection: CollectionKind::Bag,
        };
        let order = Node {
            id: "n1".into(),
            kind: NodeKind::Order {
                input: "n0".into(),
                items: vec![OrderingItem {
                    expression: Expression::new(
                        ExpressionKind::Field("f0".into()),
                        TypeDescriptor::non_nullable(ScalarType::Int64),
                    ),
                    direction: Direction::Ascending,
                    null_placement: NullPlacement::NotApplicable,
                }],
            },
            output_schema: input.output_schema.clone(),
            collection: CollectionKind::Ordered,
        };
        let slice = Node {
            id: "n2".into(),
            kind: NodeKind::Slice {
                input: "n1".into(),
                offset: 0,
                limit: Some(1),
            },
            output_schema: input.output_schema.clone(),
            collection: CollectionKind::Ordered,
        };
        let graph = Graph::new("n2", vec![input, order, slice]);

        assert!(matches!(
            graph.validate().unwrap_err().kind,
            ValidationErrorKind::IncompleteSliceKey
        ));
    }

    #[test]
    fn rejects_checked_annotations_that_disagree_with_semantics() {
        let graph = Graph::new(
            "n1",
            vec![
                Node {
                    id: "n0".into(),
                    kind: NodeKind::Input {
                        binding: "rows".into(),
                    },
                    output_schema: vec![int_field("f0")],
                    collection: CollectionKind::Bag,
                },
                Node {
                    id: "n1".into(),
                    kind: NodeKind::Filter {
                        input: "n0".into(),
                        predicate: Expression::new(
                            ExpressionKind::Literal(LiteralValue::Boolean(true)),
                            TypeDescriptor::non_nullable(ScalarType::Boolean),
                        ),
                    },
                    output_schema: vec![],
                    collection: CollectionKind::Bag,
                },
            ],
        );

        assert!(matches!(
            graph.validate().unwrap_err().kind,
            ValidationErrorKind::IncorrectOutputSchema
        ));
    }
}
