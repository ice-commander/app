use crate::doc::{Cond, Pred, PredOp, Text};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const NAMESPACES: &[&str] = &["state", "view", "host", "data", "arg"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub state: BTreeMap<String, Value>,
    #[serde(default)]
    pub view: BTreeMap<String, Value>,
    #[serde(default)]
    pub host: BTreeMap<String, Value>,
    #[serde(default)]
    pub data: BTreeMap<String, Value>,
    #[serde(default)]
    pub arg: Value,
    #[serde(default)]
    pub stored_secrets: BTreeSet<String>,
    #[serde(default)]
    pub touched: BTreeSet<String>,
}

/// Unlike the flat namespaces, an argument's key walks in: keys and indices, dot by dot.
fn dig<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(value, |held, step| match held {
        Value::Array(items) => items.get(step.parse::<usize>().ok()?),
        other => other.get(step),
    })
}

pub fn split_reference(raw: &str) -> Option<(&str, &str)> {
    let (namespace, key) = raw.split_once('.')?;
    if NAMESPACES.contains(&namespace) && !key.is_empty() {
        Some((namespace, key))
    } else {
        None
    }
}

impl State {
    pub fn for_document(document: &crate::doc::Document) -> State {
        State {
            data: document.data.clone(),
            ..State::default()
        }
    }

    pub fn with_values(values: &BTreeMap<String, String>) -> State {
        let mut built = State::default();
        for (key, value) in values {
            built
                .state
                .insert(key.clone(), Value::String(value.clone()));
        }
        built
    }

    pub fn set_state(&mut self, key: &str, value: Value) -> &mut Self {
        self.state.insert(key.to_string(), value);
        self
    }

    pub fn set_view(&mut self, key: &str, value: Value) -> &mut Self {
        self.view.insert(key.to_string(), value);
        self
    }

    pub fn set_host(&mut self, key: &str, value: Value) -> &mut Self {
        self.host.insert(key.to_string(), value);
        self
    }

    pub fn lookup(&self, reference: &str) -> Option<&Value> {
        let (namespace, key) = split_reference(reference)?;
        if namespace == "arg" {
            return dig(&self.arg, key);
        }
        let table = match namespace {
            "state" => &self.state,
            "view" => &self.view,
            "host" => &self.host,
            "data" => &self.data,
            _ => return None,
        };
        table.get(key)
    }

    pub fn text_of(&self, key: &str) -> String {
        self.state.get(key).map(as_text).unwrap_or_default()
    }
}

pub fn as_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|v| v != 0.0).unwrap_or(false),
        Value::String(s) => !s.is_empty() && s != "false" && s != "0",
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
    }
}

pub fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        },
        (Value::Null, other) | (other, Value::Null) => as_text(other).is_empty(),
        (Value::Array(a), Value::Array(b)) => a == b,
        (Value::Object(a), Value::Object(b)) => a == b,
        _ => as_text(left) == as_text(right),
    }
}

pub fn operand(raw: &Value, state: &State) -> Value {
    if let Value::String(text) = raw {
        if split_reference(text).is_some() {
            return state.lookup(text).cloned().unwrap_or(Value::Null);
        }
    }
    raw.clone()
}

pub fn evaluate(predicate: &Pred, state: &State) -> bool {
    match predicate {
        Pred::Always(value) => *value,
        Pred::Op(op) => evaluate_op(op, state),
    }
}

fn evaluate_op(op: &PredOp, state: &State) -> bool {
    match op {
        PredOp::Eq(operands) => {
            let resolved: Vec<Value> = operands.iter().map(|o| operand(o, state)).collect();
            resolved.len() >= 2
                && resolved
                    .windows(2)
                    .all(|pair| values_equal(&pair[0], &pair[1]))
        }
        PredOp::Ne(operands) => {
            let resolved: Vec<Value> = operands.iter().map(|o| operand(o, state)).collect();
            resolved.len() >= 2
                && !resolved
                    .windows(2)
                    .all(|pair| values_equal(&pair[0], &pair[1]))
        }
        PredOp::All(inner) => inner.iter().all(|p| evaluate(p, state)),
        PredOp::Any(inner) => inner.iter().any(|p| evaluate(p, state)),
        PredOp::Not(inner) => !evaluate(inner, state),
        PredOp::Truthy(target) => is_truthy(&operand(target, state)),
        PredOp::Empty(target) => as_text(&operand(target, state)).is_empty(),
        PredOp::HasStored(key) => state.stored_secrets.contains(key),
        PredOp::Touched(key) => state.touched.contains(key),
        PredOp::OneOf { value, of } => {
            let resolved = operand(value, state);
            of.iter()
                .any(|candidate| values_equal(&resolved, &operand(candidate, state)))
        }
    }
}

pub fn resolve<T: Clone>(conditional: &Cond<T>, state: &State) -> Option<T> {
    match conditional {
        Cond::Fixed(value) => Some(value.clone()),
        Cond::Cases { cases, otherwise } => {
            for case in cases {
                if evaluate(&case.when, state) {
                    return Some(case.then.clone());
                }
            }
            otherwise.as_ref().and_then(|next| resolve(next, state))
        }
    }
}

pub fn resolve_text(
    conditional: Option<&Cond<Text>>,
    state: &State,
    translate: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let picked = resolve(conditional?, state)?;
    let text = picked.resolve(translate);
    if text.contains('{') {
        return Some(substitute_with(&text, state, translate));
    }
    Some(text)
}

pub fn substitute(template: &str, state: &State) -> String {
    substitute_with(template, state, &|_| None)
}

/// A stored value may itself be a `Text`, so a placeholder can carry a translation.
pub fn display(value: &Value, translate: &dyn Fn(&str) -> Option<String>) -> String {
    if value.is_object() {
        if let Ok(text) = serde_json::from_value::<Text>(value.clone()) {
            return text.resolve(translate);
        }
    }
    as_text(value)
}

pub fn substitute_with(
    template: &str,
    state: &State,
    translate: &dyn Fn(&str) -> Option<String>,
) -> String {
    let mut rendered = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        rendered.push_str(&rest[..open]);
        let tail = &rest[open + 1..];
        let Some(close) = tail.find('}') else {
            rendered.push('{');
            rest = tail;
            continue;
        };
        let key = &tail[..close];
        match state.lookup(key) {
            Some(found) => rendered.push_str(&display(found, translate)),
            None if split_reference(key).is_some() => {}
            None => match state.state.get(key) {
                Some(found) => rendered.push_str(&display(found, translate)),
                None => {
                    rendered.push('{');
                    rendered.push_str(key);
                    rendered.push('}');
                }
            },
        }
        rest = &tail[close + 1..];
    }
    rendered.push_str(rest);
    rendered
}

pub fn visible(predicate: Option<&Pred>, state: &State) -> bool {
    predicate.map(|p| evaluate(p, state)).unwrap_or(true)
}

pub fn series_of(state: &State, key: &str) -> Vec<crate::doc::Series> {
    let Some(raw) = state.data.get(key) else {
        return Vec::new();
    };
    serde_json::from_value(raw.clone()).unwrap_or_default()
}

pub fn series_bounds(series: &[crate::doc::Series], node: &crate::doc::Node) -> (f64, f64) {
    let mut low = node.min.unwrap_or(f64::MAX);
    let mut high = node.max.unwrap_or(f64::MIN);
    if node.min.is_none() || node.max.is_none() {
        for line in series {
            for value in &line.values {
                if node.min.is_none() && *value < low {
                    low = *value;
                }
                if node.max.is_none() && *value > high {
                    high = *value;
                }
            }
        }
    }
    if !low.is_finite() || low == f64::MAX {
        low = 0.0;
    }
    if !high.is_finite() || high == f64::MIN {
        high = low + 1.0;
    }
    if (high - low).abs() < f64::EPSILON {
        high = low + 1.0;
    }
    (low, high)
}

pub fn choice_index(node: &crate::doc::Node, current: &Value) -> Option<usize> {
    let insensitive = node
        .decode
        .as_ref()
        .map(|decode| decode.case_insensitive)
        .unwrap_or(false);
    let matches = |candidate: &Value| {
        if values_equal(candidate, current) {
            return true;
        }
        insensitive && as_text(candidate).to_lowercase() == as_text(current).to_lowercase()
    };
    if let Some(found) = node.options.iter().position(|o| matches(&o.value)) {
        return Some(found);
    }
    let fallback = node
        .decode
        .as_ref()
        .and_then(|decode| decode.unknown.clone())?;
    node.options
        .iter()
        .position(|o| values_equal(&o.value, &fallback))
}

pub fn fill_template(template: &str, state: &State) -> Option<String> {
    let mut rendered = String::with_capacity(template.len());
    let mut rest = template;
    let mut substituted = false;
    while let Some(open) = rest.find('{') {
        rendered.push_str(&rest[..open]);
        let tail = &rest[open + 1..];
        let close = match tail.find('}') {
            Some(position) => position,
            None => {
                rendered.push('{');
                rest = tail;
                continue;
            }
        };
        let key = &tail[..close];
        let value = match state.lookup(key) {
            Some(found) => as_text(found),
            None => as_text(state.state.get(key).unwrap_or(&Value::Null)),
        };
        if value.is_empty() {
            return None;
        }
        substituted = true;
        rendered.push_str(&value);
        rest = &tail[close + 1..];
    }
    rendered.push_str(rest);
    if !substituted && template.contains('{') {
        return None;
    }
    Some(rendered)
}
