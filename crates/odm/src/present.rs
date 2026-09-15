//! Presentation spine: success values serialize for `--json`, render human text, optional exit.

use odm_core::OdmError;
use serde::Serialize;

/// CLI output mode (JSON vs human).
#[derive(Debug, Clone, Copy)]
pub struct GlobalOut {
    pub json: bool,
}

/// A success value the binary can finish in one place.
pub trait Present {
    fn to_json(&self) -> Result<serde_json::Value, OdmError>;
    fn to_human(&self) -> String;
    fn exit_code(&self) -> i32 {
        0
    }
}

/// Print success (JSON or human) and return the value's exit code.
pub fn finish(out: &GlobalOut, value: &impl Present) -> Result<i32, OdmError> {
    if out.json {
        print_json(&value.to_json()?)?;
    } else {
        let h = value.to_human();
        print!("{h}");
        if !h.is_empty() && !h.ends_with('\n') {
            println!();
        }
    }
    Ok(value.exit_code())
}

pub fn print_json<T: Serialize>(value: &T) -> Result<(), OdmError> {
    let s = serde_json::to_string_pretty(value)
        .map_err(|e| OdmError::operation(format!("json encode failed: {e}")))?;
    println!("{s}");
    Ok(())
}

/// Print error (human stderr or JSON stdout) and return exit code.
pub fn print_error(out: &GlobalOut, err: &OdmError) -> i32 {
    let code = odm_core::exit_code(err);
    if out.json {
        let detail = err.detail();
        let body = serde_json::json!({
            "ok": false,
            "error": {
                "code": err.code(),
                "message": err.message(),
                "detail": detail,
            }
        });
        match serde_json::to_string_pretty(&body) {
            Ok(s) => println!("{s}"),
            Err(_) => eprintln!("error: {}", err.message()),
        }
    } else {
        let msg = err.message();
        if let Some((first, rest)) = msg.split_once('\n') {
            eprintln!("error: {first}");
            if !rest.is_empty() {
                eprintln!("{rest}");
            }
        } else {
            eprintln!("error: {msg}");
        }
    }
    code
}

/// Helper: Serialize → JSON Value.
pub fn json_value<T: Serialize>(value: &T) -> Result<serde_json::Value, OdmError> {
    serde_json::to_value(value)
        .map_err(|e| OdmError::operation(format!("json encode failed: {e}")))
}

/// Shared `{ ok: true, name }` envelope (project/progen rm).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NamedOk {
    pub ok: bool,
    pub name: String,
}

impl NamedOk {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            ok: true,
            name: name.into(),
        }
    }
}

impl Present for NamedOk {
    fn to_json(&self) -> Result<serde_json::Value, OdmError> {
        json_value(self)
    }
    fn to_human(&self) -> String {
        // Caller should prefer entity-specific human; default fallback.
        format!("ok {}\n", self.name)
    }
}

/// Shared `{ ok: true, name, materialized }` envelope (project/progen add).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NamedMaterialize {
    pub ok: bool,
    pub name: String,
    pub materialized: Option<&'static str>,
}

impl NamedMaterialize {
    pub fn new(name: impl Into<String>, materialized: Option<&'static str>) -> Self {
        Self {
            ok: true,
            name: name.into(),
            materialized,
        }
    }
}

impl Present for NamedMaterialize {
    fn to_json(&self) -> Result<serde_json::Value, OdmError> {
        json_value(self)
    }
    fn to_human(&self) -> String {
        format!("ok {}\n", self.name)
    }
}

/// Wrap any Serialize value with a prebuilt human string and optional exit.
#[derive(Debug, Clone)]
pub struct Ready<T> {
    pub data: T,
    pub human: String,
    pub exit: i32,
}

impl<T> Ready<T> {
    pub fn ok(data: T, human: impl Into<String>) -> Self {
        Self {
            data,
            human: human.into(),
            exit: 0,
        }
    }

    pub fn with_exit(data: T, human: impl Into<String>, exit: i32) -> Self {
        Self {
            data,
            human: human.into(),
            exit,
        }
    }
}

impl<T: Serialize> Present for Ready<T> {
    fn to_json(&self) -> Result<serde_json::Value, OdmError> {
        json_value(&self.data)
    }
    fn to_human(&self) -> String {
        self.human.clone()
    }
    fn exit_code(&self) -> i32 {
        self.exit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_ok_json_shape() {
        let v = serde_json::to_value(NamedOk::new("alpha")).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["name"], "alpha");
    }

    #[test]
    fn named_materialize_json_shape() {
        let v = serde_json::to_value(NamedMaterialize::new("alpha", Some("cloned"))).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["name"], "alpha");
        assert_eq!(v["materialized"], "cloned");
        let n = serde_json::to_value(NamedMaterialize::new("b", None)).unwrap();
        assert!(n["materialized"].is_null());
    }

    #[test]
    fn ready_exit_code() {
        let r = Ready::with_exit(NamedOk::new("x"), "hi\n", 3);
        assert_eq!(r.exit_code(), 3);
        assert_eq!(r.to_human(), "hi\n");
        assert_eq!(r.to_json().unwrap()["name"], "x");
    }
}
