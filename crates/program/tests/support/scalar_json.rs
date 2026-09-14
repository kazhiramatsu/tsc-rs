// Explicit observation boundary for existing serde JSON fixture schemas.
// Non-scalar strings fail rather than becoming replacement or escaped values.
// Object fields, array order, null presence, and Number values are preserved.
pub trait ScalarJson {
    fn scalar_json(&self) -> serde_json::Value;
}
pub fn observe<T: ScalarJson + ?Sized>(value: &T) -> serde_json::Value {
    value.scalar_json()
}
impl<T: ScalarJson + ?Sized> ScalarJson for &T {
    fn scalar_json(&self) -> serde_json::Value {
        (*self).scalar_json()
    }
}
impl ScalarJson for tsc_diagnostics::JsString {
    fn scalar_json(&self) -> serde_json::Value {
        self.as_js().scalar_json()
    }
}
impl ScalarJson for tsc_diagnostics::JsStr<'_> {
    fn scalar_json(&self) -> serde_json::Value {
        serde_json::Value::String(
            self.as_str()
                .expect("scalar legacy JSON observation")
                .to_owned(),
        )
    }
}
impl ScalarJson for tsc_program::JsonValue {
    fn scalar_json(&self) -> serde_json::Value {
        use serde_json::Value as V;
        use tsc_program::JsonValue as J;
        match self {
            J::Null => V::Null,
            J::Bool(value) => V::Bool(*value),
            J::Number(value) => V::Number(value.clone()),
            J::String(value) => value.scalar_json(),
            J::Array(values) => values.scalar_json(),
            J::Object(values) => V::Object(
                values
                    .iter()
                    .map(|(key, value)| {
                        (
                            key.as_str().expect("scalar legacy JSON key").to_owned(),
                            value.scalar_json(),
                        )
                    })
                    .collect(),
            ),
        }
    }
}
impl<T: ScalarJson> ScalarJson for [T] {
    fn scalar_json(&self) -> serde_json::Value {
        serde_json::Value::Array(self.iter().map(ScalarJson::scalar_json).collect())
    }
}
impl<T: ScalarJson> ScalarJson for Vec<T> {
    fn scalar_json(&self) -> serde_json::Value {
        self.as_slice().scalar_json()
    }
}
impl<T: ScalarJson> ScalarJson for Option<T> {
    fn scalar_json(&self) -> serde_json::Value {
        self.as_ref()
            .map_or(serde_json::Value::Null, ScalarJson::scalar_json)
    }
}
impl ScalarJson for str {
    fn scalar_json(&self) -> serde_json::Value {
        serde_json::Value::String(self.to_owned())
    }
}
impl ScalarJson for String {
    fn scalar_json(&self) -> serde_json::Value {
        self.as_str().scalar_json()
    }
}
impl ScalarJson for serde_json::Value {
    fn scalar_json(&self) -> serde_json::Value {
        self.clone()
    }
}
