//! `json.*`: JSON text to Lua values and back (serde_json underneath).
//!
//! ```lua
//! local t = json.decode('{"a":[1,2,null],"b":{}}')  -- nil, message on bad input
//! t.a[3] == json.null                               -- null keeps array positions
//! json.encode({ id = 1, tags = json.array() })      -- '{"id":1,"tags":[]}'
//! json.encode(t, { pretty = true })
//! ```
//!
//! Encoding: a table whose keys are exactly 1..n is an array, any other
//! table an object (integer keys become strings, keys sorted); an empty
//! table is `{}` unless marked with `json.array`. Decoded arrays carry that
//! mark, so `[]` survives a round trip. Integral numbers are written
//! without a fraction; NaN, infinities, functions and other userdata raise.

use mlua::{Lua, LightUserData, Table, Value};
use serde_json::{Map, Number, Value as J};

/// Deep enough for any real document, shallow enough to stop a cycle.
const MAX_DEPTH: usize = 128;

fn null() -> Value {
    Value::LightUserData(LightUserData(std::ptr::null_mut()))
}

/// The metatable `json.array` sets, from the registry.
fn array_mt(lua: &Lua) -> mlua::Result<Table> {
    const KEY: &str = "tempest.json.array";
    if let Ok(t) = lua.named_registry_value::<Table>(KEY) {
        return Ok(t);
    }
    let t = lua.create_table()?;
    t.set("__name", "json.array")?;
    lua.set_named_registry_value(KEY, t.clone())?;
    Ok(t)
}

pub fn to_json(lua: &Lua, v: &Value) -> mlua::Result<J> {
    to_json_at(lua, v, 0)
}

fn to_json_at(lua: &Lua, v: &Value, depth: usize) -> mlua::Result<J> {
    if depth > MAX_DEPTH {
        return Err(mlua::Error::external("json.encode: nested too deep (a table that contains itself?)"));
    }
    Ok(match v {
        Value::Nil => J::Null,
        Value::LightUserData(p) if p.0.is_null() => J::Null,
        Value::Boolean(b) => J::Bool(*b),
        Value::Integer(i) => J::from(crate::lua_api::to_i64(*i)),
        Value::Number(n) => number(*n)?,
        Value::String(s) => J::String(s.to_str().map_err(|_| mlua::Error::external("json.encode: a string is not valid UTF-8"))?.to_string()),
        Value::Table(t) => {
            let is_array = t.metatable().is_some_and(|mt| array_mt(lua).is_ok_and(|a| a == mt));
            let len = t.raw_len();
            if is_array || (len > 0 && t.pairs::<Value, Value>().count() == len) {
                let mut out = Vec::with_capacity(len);
                for i in 1..=len {
                    out.push(to_json_at(lua, &t.raw_get::<Value>(i)?, depth + 1)?);
                }
                J::Array(out)
            } else {
                let mut out = Map::new();
                for pair in t.pairs::<Value, Value>() {
                    let (k, v) = pair?;
                    let key = match &k {
                        Value::String(s) => s.to_str()?.to_string(),
                        Value::Integer(i) => i.to_string(),
                        Value::Number(n) if n.fract() == 0.0 && n.is_finite() => format!("{n:.0}"),
                        other => return Err(mlua::Error::external(format!("json.encode: a {} cannot be an object key", other.type_name()))),
                    };
                    out.insert(key, to_json_at(lua, &v, depth + 1)?);
                }
                J::Object(out)
            }
        }
        other => return Err(mlua::Error::external(format!("json.encode: cannot encode a {}", other.type_name()))),
    })
}

fn number(n: f64) -> mlua::Result<J> {
    if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
        return Ok(J::from(n as i64));
    }
    Number::from_f64(n).map(J::Number).ok_or_else(|| mlua::Error::external(format!("json.encode: {n} is not a JSON number")))
}

pub fn to_lua(lua: &Lua, v: &J) -> mlua::Result<Value> {
    Ok(match v {
        J::Null => null(),
        J::Bool(b) => Value::Boolean(*b),
        J::Number(n) => match n.as_i64() {
            Some(i) => crate::lua_api::int(i),
            None => Value::Number(n.as_f64().unwrap_or(f64::NAN)),
        },
        J::String(s) => Value::String(lua.create_string(s)?),
        J::Array(items) => {
            let t = lua.create_table_with_capacity(items.len(), 0)?;
            for (i, item) in items.iter().enumerate() {
                t.raw_set(i + 1, to_lua(lua, item)?)?;
            }
            t.set_metatable(Some(array_mt(lua)?));
            Value::Table(t)
        }
        J::Object(fields) => {
            let t = lua.create_table_with_capacity(0, fields.len())?;
            for (k, v) in fields {
                t.raw_set(k.as_str(), to_lua(lua, v)?)?;
            }
            Value::Table(t)
        }
    })
}

/// `json.decode` for other modules: the value, or `nil, message`.
pub fn decode(lua: &Lua, text: &[u8]) -> mlua::Result<mlua::MultiValue> {
    use mlua::IntoLuaMulti;
    match serde_json::from_slice::<J>(text) {
        Ok(v) => to_lua(lua, &v)?.into_lua_multi(lua),
        Err(e) => (Value::Nil, format!("json.decode: {e}")).into_lua_multi(lua),
    }
}

pub fn register(lua: &Lua) -> mlua::Result<()> {
    let t = lua.create_table()?;
    t.set("null", null())?;
    t.set(
        "encode",
        lua.create_function(|lua, (v, opts): (Value, Option<Table>)| {
            let j = to_json(lua, &v)?;
            let pretty = opts.map(|o| o.get::<bool>("pretty")).transpose()?.unwrap_or(false);
            let text = if pretty { serde_json::to_string_pretty(&j) } else { serde_json::to_string(&j) };
            text.map_err(mlua::Error::external)
        })?,
    )?;
    t.set("decode", lua.create_function(|lua, text: mlua::String| decode(lua, &text.as_bytes()))?)?;
    // json.array(t?) -> t: encode `t` (default a new table) as an array even when empty.
    t.set(
        "array",
        lua.create_function(|lua, t: Option<Table>| {
            let t = match t {
                Some(t) => t,
                None => lua.create_table()?,
            };
            t.set_metatable(Some(array_mt(lua)?));
            Ok(t)
        })?,
    )?;
    lua.globals().set("json", t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lua() -> Lua {
        let lua = Lua::new();
        register(&lua).unwrap();
        lua
    }

    #[test]
    fn round_trips() {
        let lua = lua();
        let out: String = lua
            .load(r#"return json.encode(json.decode('{"a":[1,2.5,null,"x"],"b":{},"c":[],"d":true,"big":12345678901}'))"#)
            .eval()
            .unwrap();
        assert_eq!(out, r#"{"a":[1,2.5,null,"x"],"b":{},"big":12345678901,"c":[],"d":true}"#);
    }

    #[test]
    fn tables() {
        let lua = lua();
        let out: String = lua.load(r#"return json.encode({ [2420] = "Zhin", list = { 3, 2, 1 }, f = 2.0, e = {} })"#).eval().unwrap();
        assert_eq!(out, r#"{"2420":"Zhin","e":{},"f":2,"list":[3,2,1]}"#);
        let out: String = lua.load(r#"return json.encode({ 1, 2, x = 3 })"#).eval().unwrap();
        assert_eq!(out, r#"{"1":1,"2":2,"x":3}"#);
        let out: String = lua.load(r#"return json.encode(json.array())"#).eval().unwrap();
        assert_eq!(out, "[]");
    }

    #[test]
    fn errors() {
        let lua = lua();
        let (v, err): (Value, String) = lua.load(r#"return json.decode('{"a":')"#).eval().unwrap();
        assert!(v.is_nil() && err.starts_with("json.decode:"), "{err}");
        assert!(lua.load("local t = {} t.t = t return json.encode(t)").exec().is_err());
        assert!(lua.load("return json.encode(0/0)").exec().is_err());
        assert!(lua.load("return json.encode(print)").exec().is_err());
    }
}
