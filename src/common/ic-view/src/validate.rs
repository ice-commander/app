use crate::doc::{Cond, Document, Intent, Node, NodeKind, Pred, PredOp, SCHEMA};
use crate::expr::split_reference;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    pub at: String,
    pub message: String,
}

impl Issue {
    fn error(at: String, message: String) -> Issue {
        Issue {
            severity: Severity::Error,
            at,
            message,
        }
    }

    fn warning(at: String, message: String) -> Issue {
        Issue {
            severity: Severity::Warning,
            at,
            message,
        }
    }
}

fn collect_refs(operands: &[Value], namespace: &str, into: &mut BTreeSet<String>) {
    for operand in operands {
        if let Value::String(text) = operand {
            if let Some((found, key)) = split_reference(text) {
                if found == namespace {
                    into.insert(key.to_string());
                }
            }
        }
    }
}

fn walk_predicate(predicate: &Pred, into: &mut BTreeSet<String>) {
    predicate_refs(predicate, "state", into);
}

fn predicate_refs(predicate: &Pred, namespace: &str, into: &mut BTreeSet<String>) {
    let Pred::Op(op) = predicate else { return };
    match op {
        PredOp::Eq(operands) | PredOp::Ne(operands) => collect_refs(operands, namespace, into),
        PredOp::Truthy(target) | PredOp::Empty(target) => {
            collect_refs(std::slice::from_ref(target), namespace, into)
        }
        PredOp::OneOf { value, of } => {
            collect_refs(std::slice::from_ref(value), namespace, into);
            collect_refs(of, namespace, into);
        }
        PredOp::All(inner) | PredOp::Any(inner) => {
            for nested in inner {
                predicate_refs(nested, namespace, into);
            }
        }
        PredOp::Not(inner) => predicate_refs(inner, namespace, into),
        PredOp::HasStored(_) | PredOp::Touched(_) => {}
    }
}

/// Binds the user can type into, and the `data.` names the document's
/// predicates read. A name in both is almost always the mistake of writing
/// `data.host` where the typed value lands in `state.host`: the predicate then
/// reads what the plugin last put there and never sees the typing.
fn typed_binds_read_as_data(document: &Document) -> BTreeSet<String> {
    let mut typed = BTreeSet::new();
    let mut read = BTreeSet::new();
    document.form.walk(&mut |node| {
        if node.takes_value() {
            if let Some(bind) = &node.bind {
                typed.insert(bind.clone());
            }
        }
        for found in [node.visible.as_ref(), node.sensitive.as_ref()]
            .into_iter()
            .flatten()
        {
            predicate_refs(found, "data", &mut read);
        }
    });
    typed.intersection(&read).cloned().collect()
}

fn walk_conditional<T>(conditional: &Cond<T>, into: &mut BTreeSet<String>) {
    let Cond::Cases { cases, otherwise } = conditional else {
        return;
    };
    for case in cases {
        walk_predicate(&case.when, into);
    }
    if let Some(next) = otherwise {
        walk_conditional(next, into);
    }
}

fn node_refs(node: &Node, into: &mut BTreeSet<String>) {
    for found in [node.visible.as_ref(), node.sensitive.as_ref()]
        .into_iter()
        .flatten()
    {
        walk_predicate(found, into);
    }
    for found in [
        node.title.as_ref(),
        node.subtitle.as_ref(),
        node.text.as_ref(),
        node.placeholder.as_ref(),
        node.tooltip.as_ref(),
        node.caption.as_ref(),
        node.label.as_ref(),
        node.icon.as_ref(),
        node.role.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        walk_conditional(found, into);
    }
    if let Some(found) = node.intent.as_ref() {
        walk_conditional(found, into);
    }
}

fn intent_is_understood(intent: &Intent) -> bool {
    !matches!(intent, Intent::Unknown)
}

fn intent_understood(conditional: &Cond<Intent>) -> bool {
    match conditional {
        Cond::Fixed(intent) => intent_is_understood(intent),
        Cond::Cases { cases, otherwise } => {
            cases.iter().all(|case| intent_is_understood(&case.then))
                && otherwise
                    .as_ref()
                    .map(|next| intent_understood(next))
                    .unwrap_or(true)
        }
    }
}

fn intent_targets(conditional: &Cond<Intent>, into: &mut BTreeSet<String>) {
    match conditional {
        Cond::Fixed(intent) => {
            if let Intent::Pick { node } | Intent::Emit { node } = intent {
                into.insert(node.clone());
            }
        }
        Cond::Cases { cases, otherwise } => {
            for case in cases {
                if let Intent::Pick { node } | Intent::Emit { node } = &case.then {
                    into.insert(node.clone());
                }
            }
            if let Some(next) = otherwise {
                intent_targets(next, into);
            }
        }
    }
}

fn check_node(
    node: &Node,
    at: &str,
    document: &Document,
    ids: &mut BTreeSet<String>,
    refs: &mut BTreeSet<String>,
    intents: &mut BTreeSet<String>,
    issues: &mut Vec<Issue>,
) {
    if node.t.trim().is_empty() {
        issues.push(Issue::error(at.to_string(), "node has no type".to_string()));
    }
    if node.kind() == NodeKind::Unknown && !node.t.trim().is_empty() {
        issues.push(Issue::warning(
            at.to_string(),
            format!("unknown node type {}", node.t),
        ));
    }
    if let Some(id) = &node.id {
        if !ids.insert(id.clone()) {
            issues.push(Issue::error(
                at.to_string(),
                format!("duplicate node id {id}"),
            ));
        }
    }
    // A slider the plugin moves reads its place out of `data`, so it owes no field. One that
    // names neither falls through to the rule below and is an error.
    let reads_its_own = node.kind() == NodeKind::Slider && node.value_key.is_some();
    if node.takes_value() && !reads_its_own {
        match &node.bind {
            None => issues.push(Issue::error(
                at.to_string(),
                format!("{} node has no bind", node.t),
            )),
            Some(bind) => {
                if document.field(bind).is_none() {
                    issues.push(Issue::error(
                        at.to_string(),
                        format!("bind {bind} is not a declared field"),
                    ));
                }
            }
        }
    }
    if node.kind() == NodeKind::Choice && node.options.is_empty() {
        issues.push(Issue::error(
            at.to_string(),
            "choice node has no options".to_string(),
        ));
    }
    if node.kind() == NodeKind::Table && node.columns.is_empty() {
        issues.push(Issue::error(
            at.to_string(),
            "table node has no columns".to_string(),
        ));
    }
    // A picture is named rather than carried, so a node that names nothing is
    // an empty box where the user expects to see something.
    if matches!(node.kind(), NodeKind::Image | NodeKind::Media) && node.src.is_none() {
        issues.push(Issue::error(
            at.to_string(),
            format!("{} node has no src", node.t),
        ));
    }
    if node.kind() == NodeKind::Button && node.intent.is_none() {
        issues.push(Issue::warning(
            at.to_string(),
            "button node has no intent".to_string(),
        ));
    }
    node_refs(node, refs);
    if let Some(found) = node.intent.as_ref() {
        intent_targets(found, intents);
        if !intent_understood(found) {
            issues.push(Issue::error(
                at.to_string(),
                "intent is not understood".to_string(),
            ));
        }
    }
    for (index, child) in node.children.iter().enumerate() {
        let nested = format!("{at}.children[{index}]");
        check_node(child, &nested, document, ids, refs, intents, issues);
    }
}

pub fn validate(document: &Document) -> Vec<Issue> {
    let mut issues = Vec::new();
    if document.schema != SCHEMA {
        issues.push(Issue::warning(
            "schema".to_string(),
            format!(
                "document schema {} differs from {}",
                document.schema, SCHEMA
            ),
        ));
    }
    for bind in typed_binds_read_as_data(document) {
        issues.push(Issue::warning(
            "form".to_string(),
            format!(
                "a predicate reads data.{bind} while {bind} is typed into, and typing lands                  in state.{bind}"
            ),
        ));
    }
    let mut binds = BTreeSet::new();
    for (index, field) in document.fields.iter().enumerate() {
        if field.bind.trim().is_empty() {
            issues.push(Issue::error(
                format!("fields[{index}]"),
                "field has no bind".to_string(),
            ));
        }
        if !binds.insert(field.bind.clone()) {
            issues.push(Issue::error(
                format!("fields[{index}]"),
                format!("duplicate field {}", field.bind),
            ));
        }
        if let Some(predicate) = field.when.as_ref() {
            let mut used = BTreeSet::new();
            walk_predicate(predicate, &mut used);
            for key in used {
                if !document.fields.iter().any(|other| other.bind == key) {
                    issues.push(Issue::warning(
                        format!("fields[{index}].when"),
                        format!("references undeclared field {key}"),
                    ));
                }
            }
        }
    }

    let mut ids = BTreeSet::new();
    let mut refs = BTreeSet::new();
    let mut intents = BTreeSet::new();
    check_node(
        &document.form,
        "form",
        document,
        &mut ids,
        &mut refs,
        &mut intents,
        &mut issues,
    );

    let mut action_ids = BTreeSet::new();
    for (index, action) in document.actions.iter().enumerate() {
        if !action_ids.insert(action.id.clone()) {
            issues.push(Issue::error(
                format!("actions[{index}]"),
                format!("duplicate action id {}", action.id),
            ));
        }
        if let Some(predicate) = action.visible.as_ref() {
            walk_predicate(predicate, &mut refs);
        }
        if let Some(predicate) = action.sensitive.as_ref() {
            walk_predicate(predicate, &mut refs);
        }
        for found in [action.label.as_ref(), action.role.as_ref()]
            .into_iter()
            .flatten()
        {
            walk_conditional(found, &mut refs);
        }
        if let Some(found) = action.intent.as_ref() {
            walk_conditional(found, &mut refs);
            intent_targets(found, &mut intents);
            if !intent_understood(found) {
                issues.push(Issue::error(
                    format!("actions[{index}]"),
                    "intent is not understood".to_string(),
                ));
            }
        }
    }

    for key in refs {
        if !binds.contains(&key) {
            issues.push(Issue::warning(
                "predicates".to_string(),
                format!("state.{key} is not a declared field"),
            ));
        }
    }
    for target in intents {
        if !ids.contains(&target) {
            issues.push(Issue::error(
                "intents".to_string(),
                format!("intent targets unknown node {target}"),
            ));
        }
    }
    if let Some(identity) = &document.identity {
        if !binds.contains(identity) {
            issues.push(Issue::error(
                "identity".to_string(),
                format!("identity {identity} is not a declared field"),
            ));
        }
    }
    if let Some(opens_at) = &document.opens_at {
        if !binds.contains(opens_at) {
            issues.push(Issue::error(
                "opens_at".to_string(),
                format!("opens_at {opens_at} is not a declared field"),
            ));
        }
    }
    for locked in &document.immutable_after_create {
        if !binds.contains(locked) {
            issues.push(Issue::warning(
                "immutable_after_create".to_string(),
                format!("{locked} is not a declared field"),
            ));
        }
    }
    issues
}

pub fn errors(document: &Document) -> Vec<Issue> {
    validate(document)
        .into_iter()
        .filter(|issue| issue.severity == Severity::Error)
        .collect()
}

pub fn accept(document: &Document) -> Result<(), Vec<Issue>> {
    let blocking = errors(document);
    if blocking.is_empty() {
        Ok(())
    } else {
        Err(blocking)
    }
}

fn conditional_keys(conditional: &Cond<crate::doc::Text>, into: &mut BTreeSet<String>) {
    match conditional {
        Cond::Fixed(text) => {
            if let Some(key) = text.key() {
                into.insert(key.to_string());
            }
        }
        Cond::Cases { cases, otherwise } => {
            for case in cases {
                if let Some(key) = case.then.key() {
                    into.insert(key.to_string());
                }
            }
            if let Some(next) = otherwise {
                conditional_keys(next, into);
            }
        }
    }
}

fn node_translation_keys(node: &Node, into: &mut BTreeSet<String>) {
    for conditional in [
        node.title.as_ref(),
        node.subtitle.as_ref(),
        node.text.as_ref(),
        node.placeholder.as_ref(),
        node.tooltip.as_ref(),
        node.label.as_ref(),
        node.role.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        conditional_keys(conditional, into);
    }
    for option in &node.options {
        if let Some(key) = option.label.as_ref().and_then(|label| label.key()) {
            into.insert(key.to_string());
        }
    }
    for column in &node.columns {
        if let Some(key) = column.title.as_ref().and_then(|title| title.key()) {
            into.insert(key.to_string());
        }
    }
    for child in &node.children {
        node_translation_keys(child, into);
    }
}

pub fn translation_keys(document: &Document) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    if let Some(key) = document.label.as_ref().and_then(|label| label.key()) {
        found.insert(key.to_string());
    }
    node_translation_keys(&document.form, &mut found);
    for action in &document.actions {
        for conditional in [action.label.as_ref(), action.role.as_ref()]
            .into_iter()
            .flatten()
        {
            conditional_keys(conditional, &mut found);
        }
    }
    found
}

fn enum_mismatch(raw: &serde_json::Map<String, Value>, key: &str, parsed: Value) -> bool {
    match raw.get(key) {
        Some(found) => found != &parsed,
        None => false,
    }
}

fn dropped_node_properties(node: &Node, raw: &Value, at: &str, issues: &mut Vec<Issue>) {
    let Some(object) = raw.as_object() else {
        return;
    };
    let kept: &[(&str, bool)] = &[
        ("title", node.title.is_some()),
        ("subtitle", node.subtitle.is_some()),
        ("text", node.text.is_some()),
        ("placeholder", node.placeholder.is_some()),
        ("tooltip", node.tooltip.is_some()),
        ("label", node.label.is_some()),
        ("icon", node.icon.is_some()),
        ("role", node.role.is_some()),
        ("visible", node.visible.is_some()),
        ("sensitive", node.sensitive.is_some()),
        ("intent", node.intent.is_some()),
        ("decode", node.decode.is_some()),
        ("picker", node.picker.is_some()),
    ];
    for (key, understood) in kept {
        let present = object.get(*key).map(|v| !v.is_null()).unwrap_or(false);
        if !present || *understood {
            continue;
        }
        // A property that could not be read goes missing, and a missing label
        // or tooltip is visibly missing. A missing predicate is not: `visible`
        // and `sensitive` mean "always" when they are absent, so a misread one
        // puts a control in front of the user that should not be there, and
        // nothing later in the drawing can tell. That one is an error.
        if matches!(*key, "visible" | "sensitive") {
            issues.push(Issue::error(
                at.to_string(),
                format!("{key} is not a predicate this application understands"),
            ));
        } else {
            issues.push(Issue::warning(
                at.to_string(),
                format!("property {key} could not be understood"),
            ));
        }
    }
    let enums: &[(&str, Value)] = &[
        (
            "chrome",
            serde_json::to_value(node.chrome).unwrap_or(Value::Null),
        ),
        (
            "variant",
            serde_json::to_value(node.variant).unwrap_or(Value::Null),
        ),
        (
            "emit",
            serde_json::to_value(node.emit).unwrap_or(Value::Null),
        ),
        (
            "scroll",
            serde_json::to_value(node.scroll).unwrap_or(Value::Null),
        ),
        (
            "surface",
            serde_json::to_value(node.surface).unwrap_or(Value::Null),
        ),
    ];
    for (key, parsed) in enums {
        if enum_mismatch(object, key, parsed.clone()) {
            issues.push(Issue::warning(
                at.to_string(),
                format!("property {key} could not be understood"),
            ));
        }
    }
    if let Some(Value::Array(items)) = object.get("children") {
        for (index, (child, raw_child)) in node.children.iter().zip(items).enumerate() {
            let nested = format!("{at}.children[{index}]");
            dropped_node_properties(child, raw_child, &nested, issues);
        }
    }
}

pub fn validate_source(source: &str) -> Result<(Document, Vec<Issue>), String> {
    let document = Document::parse(source)?;
    let raw: Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
    let mut issues = validate(&document);
    if let Some(form) = raw.get("form") {
        dropped_node_properties(&document.form, form, "form", &mut issues);
    }
    if let Some(Value::Array(items)) = raw.get("fields") {
        for (index, (field, raw_field)) in document.fields.iter().zip(items).enumerate() {
            let Some(object) = raw_field.as_object() else {
                continue;
            };
            let kept: &[(&str, bool)] = &[
                ("when", field.when.is_some()),
                ("default", field.default.is_some()),
            ];
            for (key, understood) in kept {
                let present = object.get(*key).map(|v| !v.is_null()).unwrap_or(false);
                if present && !understood {
                    issues.push(Issue::warning(
                        format!("fields[{index}]"),
                        format!("property {key} could not be understood"),
                    ));
                }
            }
            let enums: &[(&str, Value)] = &[
                (
                    "type",
                    serde_json::to_value(field.field_type).unwrap_or(Value::Null),
                ),
                (
                    "scope",
                    serde_json::to_value(field.scope).unwrap_or(Value::Null),
                ),
                (
                    "derive",
                    serde_json::to_value(field.derive).unwrap_or(Value::Null),
                ),
                (
                    "on_parse_error",
                    serde_json::to_value(field.on_parse_error).unwrap_or(Value::Null),
                ),
            ];
            for (key, parsed) in enums {
                if enum_mismatch(object, key, parsed.clone()) {
                    issues.push(Issue::warning(
                        format!("fields[{index}]"),
                        format!("property {key} could not be understood"),
                    ));
                }
            }
        }
    }
    if let Some(Value::Array(items)) = raw.get("actions") {
        for (index, (action, raw_action)) in document.actions.iter().zip(items).enumerate() {
            let Some(object) = raw_action.as_object() else {
                continue;
            };
            let kept: &[(&str, bool)] = &[
                ("label", action.label.is_some()),
                ("role", action.role.is_some()),
                ("visible", action.visible.is_some()),
                ("sensitive", action.sensitive.is_some()),
                ("intent", action.intent.is_some()),
            ];
            for (key, understood) in kept {
                let present = object.get(*key).map(|v| !v.is_null()).unwrap_or(false);
                if present && !understood {
                    issues.push(Issue::warning(
                        format!("actions[{index}]"),
                        format!("property {key} could not be understood"),
                    ));
                }
            }
        }
    }
    Ok((document, issues))
}
