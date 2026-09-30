use crate::doc::{Derive, Document, Field, FieldType, OnParseError, Scope, Summary};
use crate::expr::{as_text, evaluate, fill_template, is_truthy, resolve, State};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub enum CommitProblem {
    NotAnInteger { bind: String, got: String },
    OutOfRange { bind: String, got: i64 },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Commit {
    pub settings: BTreeMap<String, String>,
    pub record: BTreeMap<String, String>,
    pub keep_stored: BTreeSet<String>,
    pub missing: Vec<String>,
    pub problems: Vec<CommitProblem>,
}

impl Commit {
    pub fn is_valid(&self) -> bool {
        self.missing.is_empty() && self.problems.is_empty()
    }
}

pub fn applicable(field: &Field, state: &State) -> bool {
    field
        .when
        .as_ref()
        .map(|predicate| evaluate(predicate, state))
        .unwrap_or(true)
}

fn wants_default(field: &Field, present: bool, state: &State) -> bool {
    match field.derive {
        Derive::Never => false,
        Derive::Always => true,
        Derive::CommitOnly => !present,
        Derive::WhileUntouched => !present || !state.touched.contains(&field.bind),
    }
}

fn default_for(field: &Field, state: &State) -> Option<Value> {
    resolve(field.default.as_ref()?, state)
}

fn coerce(field: &Field, raw: &Value, state: &State) -> Result<Option<String>, CommitProblem> {
    match field.field_type {
        FieldType::Bool => Ok(Some(is_truthy(raw).to_string())),
        FieldType::Integer => {
            let text = as_text(raw);
            let parsed = text.trim().parse::<i64>();
            let number = match parsed {
                Ok(value) => value,
                Err(_) => {
                    if field.on_parse_error == OnParseError::Default {
                        match default_for(field, state) {
                            Some(fallback) => match as_text(&fallback).trim().parse::<i64>() {
                                Ok(value) => value,
                                Err(_) => {
                                    return Err(CommitProblem::NotAnInteger {
                                        bind: field.bind.clone(),
                                        got: text,
                                    })
                                }
                            },
                            None => return Ok(None),
                        }
                    } else {
                        return Err(CommitProblem::NotAnInteger {
                            bind: field.bind.clone(),
                            got: text,
                        });
                    }
                }
            };
            if field.min.map(|low| number < low).unwrap_or(false)
                || field.max.map(|high| number > high).unwrap_or(false)
            {
                return Err(CommitProblem::OutOfRange {
                    bind: field.bind.clone(),
                    got: number,
                });
            }
            Ok(Some(number.to_string()))
        }
        FieldType::Text | FieldType::Path => Ok(Some(as_text(raw))),
    }
}

pub fn commit(document: &Document, state: &State) -> Commit {
    let mut outcome = Commit::default();
    for field in &document.fields {
        let usable = applicable(field, state);
        if !usable && !field.keep_when_inapplicable {
            continue;
        }
        let raw = state.state.get(&field.bind).cloned();
        let present = raw
            .as_ref()
            .map(|value| !as_text(value).is_empty())
            .unwrap_or(false);

        if field.secret
            && !present
            && field.empty_keeps_stored
            && state.stored_secrets.contains(&field.bind)
        {
            outcome.keep_stored.insert(field.bind.clone());
            continue;
        }

        let mut chosen = raw.unwrap_or(Value::Null);
        if wants_default(field, present, state) {
            if let Some(fallback) = default_for(field, state) {
                if !present || field.derive == Derive::Always {
                    chosen = fallback;
                }
            }
        }

        let rendered = match coerce(field, &chosen, state) {
            Ok(Some(text)) => text,
            Ok(None) => String::new(),
            Err(problem) => {
                outcome.problems.push(problem);
                continue;
            }
        };

        if rendered.is_empty() && field.empty_as_absent {
            if field.required && usable {
                outcome.missing.push(field.bind.clone());
            }
            continue;
        }
        if rendered.is_empty() && field.required && usable {
            outcome.missing.push(field.bind.clone());
        }

        match field.scope {
            Scope::Record => outcome.record.insert(field.bind.clone(), rendered),
            Scope::Settings => outcome.settings.insert(field.bind.clone(), rendered),
        };
    }
    outcome
}

pub fn render_summary(summary: Option<&Summary>, state: &State) -> Option<String> {
    let mut current = summary?;
    loop {
        if let Some(text) = fill_template(&current.fmt, state) {
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
        {
            let next = current.fallback.as_deref()?;
            current = next
        }
    }
}

pub fn immutable_now(document: &Document, state: &State) -> BTreeSet<String> {
    let creating = state
        .view
        .get("mode")
        .map(|mode| as_text(mode) == "new")
        .unwrap_or(false);
    if creating {
        BTreeSet::new()
    } else {
        document.immutable_after_create.iter().cloned().collect()
    }
}
