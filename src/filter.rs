//! Field filters such as `mos<3.5`, `equipment=NOE-IP` or `local_ip~10.1.`.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::fields::{self, Field, Kind};
use crate::ticket::{Ticket, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    NotContains,
}

// Longest operators first so `<=` is not read as `<`.
const OPS: &[(&str, Op)] = &[
    ("!=", Op::Ne),
    ("<>", Op::Ne),
    ("<=", Op::Le),
    (">=", Op::Ge),
    ("!~", Op::NotContains),
    ("=", Op::Eq),
    ("<", Op::Lt),
    (">", Op::Gt),
    ("~", Op::Contains),
];

#[derive(Debug, Clone)]
pub struct Condition {
    pub field: &'static Field,
    pub op: Op,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

impl FromStr for Condition {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (pos, sym, op) = OPS
            .iter()
            .filter_map(|&(sym, op)| s.find(sym).map(|p| (p, sym, op)))
            .min_by_key(|&(p, sym, _)| (p, std::cmp::Reverse(sym.len())))
            .ok_or_else(|| ParseError(format!("no operator in filter '{s}' (use = != < <= > >= ~ !~)")))?;
        let name = s[..pos].trim();
        let field =
            fields::lookup(name).ok_or_else(|| ParseError(format!("unknown field '{name}' (see `ipview fields`)")))?;
        Ok(Condition { field, op, value: s[pos + sym.len()..].trim().to_string() })
    }
}

impl Condition {
    pub fn matches(&self, t: &Ticket) -> bool {
        let Some(actual) = t.value(self.field.id) else {
            return false;
        };
        match self.op {
            Op::Contains => contains(&actual, &self.value),
            Op::NotContains => !contains(&actual, &self.value),
            op => {
                // Enumerations can also be matched by their raw code, e.g. `codec=8`.
                let raw = matches!(self.field.kind, Kind::Codec | Kind::VoiceMode)
                    .then(|| t.u8(self.field.id).map(|c| Value::Int(c as u64)))
                    .flatten()
                    .filter(|_| self.value.parse::<f64>().is_ok());
                let Some(ord) = compare(self.field, raw.as_ref().unwrap_or(&actual), &self.value) else {
                    return false;
                };
                match op {
                    Op::Eq => ord == Ordering::Equal,
                    Op::Ne => ord != Ordering::Equal,
                    Op::Lt => ord == Ordering::Less,
                    Op::Le => ord != Ordering::Greater,
                    Op::Gt => ord == Ordering::Greater,
                    Op::Ge => ord != Ordering::Less,
                    Op::Contains | Op::NotContains => unreachable!(),
                }
            }
        }
    }
}

fn contains(actual: &Value, needle: &str) -> bool {
    actual.to_string().to_lowercase().contains(&needle.to_lowercase())
}

fn compare(field: &Field, actual: &Value, wanted: &str) -> Option<Ordering> {
    // Timestamps render as "YYYY-MM-DD HH:MM:SS", so prefixes like "2026-08-26"
    // compare correctly as strings.
    if field.kind == Kind::Timestamp {
        let s = actual.to_string();
        return Some(s[..wanted.len().min(s.len())].cmp(wanted));
    }
    if let (Some(a), Ok(w)) = (actual.as_f64(), wanted.parse::<f64>()) {
        return a.partial_cmp(&w);
    }
    let a = actual.to_string().to_lowercase();
    Some(a.as_str().cmp(wanted.to_lowercase().as_str()))
}

/// A set of conditions combined with AND (default) or OR.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub conditions: Vec<Condition>,
    pub any: bool,
}

impl Filter {
    pub fn matches(&self, t: &Ticket) -> bool {
        if self.conditions.is_empty() {
            return true;
        }
        if self.any {
            self.conditions.iter().any(|c| c.matches(t))
        } else {
            self.conditions.iter().all(|c| c.matches(t))
        }
    }
}

/// Matches tickets between two endpoints, in either direction.
pub fn between(t: &Ticket, a: &str, b: &str) -> bool {
    let local = t.value(8).map(|v| v.to_string()).unwrap_or_default();
    let distant = t.value(9).map(|v| v.to_string()).unwrap_or_default();
    (local == a && distant == b) || (local == b && distant == a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticket::tests::tlv;

    fn ticket() -> Ticket {
        Ticket::from_raw(&tlv(&[
            (1, &0x6a8e_b23bu32.to_be_bytes()),
            (6, &[1, 2]),
            (8, &[10, 0, 0, 1]),
            (9, &[10, 0, 0, 2]),
            (12, &[0, 30]),
        ]))
    }

    fn check(expr: &str) -> bool {
        expr.parse::<Condition>().unwrap().matches(&ticket())
    }

    #[test]
    fn numeric_and_text_comparisons() {
        assert!(check("duration=30"));
        assert!(check("12>=30"));
        assert!(!check("duration<30"));
        assert!(check("equipment=noe-ip"));
        assert!(check("local_ip~10.0."));
        assert!(check("distant_ip!~192.168"));
        assert!(check("end_time>=2026-08-26"));
        assert!(check("end_time<2026-08-27"));
        assert!(!check("local_id=123"), "absent fields never match");
    }

    #[test]
    fn enums_match_by_name_or_code() {
        let t = Ticket::from_raw(&tlv(&[(15, &[8])]));
        let m = |s: &str| s.parse::<Condition>().unwrap().matches(&t);
        assert!(m("codec=8"));
        assert!(m("codec=codec 8"));
        assert!(!m("codec=0"));
    }

    #[test]
    fn parse_errors() {
        assert!("duration".parse::<Condition>().is_err());
        assert!("bogus=1".parse::<Condition>().is_err());
        assert_eq!("mos <= 3.5".parse::<Condition>().unwrap().op, Op::Le);
    }

    #[test]
    fn and_or_and_between() {
        let c = |s: &str| s.parse::<Condition>().unwrap();
        let t = ticket();
        let and = Filter { conditions: vec![c("duration=30"), c("duration=31")], any: false };
        let or = Filter { any: true, ..and.clone() };
        assert!(!and.matches(&t));
        assert!(or.matches(&t));
        assert!(between(&t, "10.0.0.2", "10.0.0.1"));
        assert!(!between(&t, "10.0.0.2", "10.0.0.3"));
    }
}
