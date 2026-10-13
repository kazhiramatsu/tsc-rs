//! tsgo `lsproto/jsonrpc.go` and `jsonrpc/jsonrpc.go`: the base protocol's
//! messages. A message without a method but with an id is a response; one
//! without an id is a notification; any other a request. Params and results
//! stay raw JSON text: an inbound one until a handler decodes it, an
//! outbound one written from its typed value in the type's field order.

use std::collections::HashMap;

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::value::RawValue;
use serde_json::Value;

use super::codec::{Decode, DecodeError};
use super::{ErrorCode, IntegerOrString, NoParams};

/// tsgo `jsonrpc.ID`: a string or an integer. Go keeps both in one value, so
/// the empty string is the integer 0.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Id {
    Int(i32),
    String(String),
}

impl Id {
    /// tsgo `NewID`.
    pub fn from_integer_or_string(value: &IntegerOrString) -> Self {
        match value {
            IntegerOrString::String(text) if !text.is_empty() => Self::String(text.clone()),
            IntegerOrString::String(_) => Self::Int(0),
            IntegerOrString::Integer(number) => Self::Int(*number),
        }
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(number) => write!(formatter, "{number}"),
            Self::String(text) => formatter.write_str(text),
        }
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Int(number) => serializer.serialize_i32(*number),
            Self::String(text) => serializer.serialize_str(text),
        }
    }
}

impl Decode for Id {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        match value {
            Value::String(text) if text.is_empty() => Ok(Self::Int(0)),
            Value::String(text) => Ok(Self::String(text.clone())),
            other => i32::decode(other).map(Self::Int),
        }
    }
}

/// tsgo `jsonrpc.ResponseError`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl std::fmt::Display for ResponseError {
    /// tsgo `ResponseError.String`.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "[{}]: {}", self.code, self.message)
    }
}

impl Decode for ResponseError {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        let object = super::codec::expect_object(value)?;
        Ok(Self {
            code: object
                .get("code")
                .map(i32::decode)
                .transpose()?
                .unwrap_or_default(),
            message: object
                .get("message")
                .map(String::decode)
                .transpose()?
                .unwrap_or_default(),
            data: object.get("data").filter(|data| !data.is_null()).cloned(),
        })
    }
}

/// Raw JSON text, as received or as written from a typed value.
pub type RawJson = Box<RawValue>;

/// A typed value as raw JSON text, in its type's field order.
pub fn to_raw<T: Serialize + ?Sized>(value: &T) -> Result<RawJson, serde_json::Error> {
    serde_json::value::to_raw_value(value)
}

/// A request or a notification (tsgo `RequestMessage`).
#[derive(Clone, Debug)]
pub struct RequestMessage {
    pub id: Option<Id>,
    pub method: String,
    pub params: Option<RawJson>,
}

impl RequestMessage {
    /// tsgo `NewRequestMessage` and `NewNotificationMessage`: a method's
    /// params, omitted for a method that takes none.
    pub fn new<T: Serialize + 'static>(
        id: Option<Id>,
        method: &str,
        params: &T,
    ) -> Result<Self, serde_json::Error> {
        let params = if std::any::TypeId::of::<T>() == std::any::TypeId::of::<NoParams>() {
            None
        } else {
            Some(to_raw(params)?)
        };
        Ok(Self {
            id,
            method: method.to_owned(),
            params,
        })
    }
}

impl Serialize for RequestMessage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("RequestMessage", 4)?;
        state.serialize_field("jsonrpc", "2.0")?;
        if let Some(id) = &self.id {
            state.serialize_field("id", id)?;
        }
        state.serialize_field("method", &self.method)?;
        if let Some(params) = &self.params {
            state.serialize_field("params", params)?;
        }
        state.end()
    }
}

/// tsgo `ResponseMessage`: the id is `null` when the request's was not known.
#[derive(Clone, Debug)]
pub struct ResponseMessage {
    pub id: Option<Id>,
    pub result: Option<RawJson>,
    pub error: Option<ResponseError>,
}

impl Serialize for ResponseMessage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("ResponseMessage", 4)?;
        state.serialize_field("jsonrpc", "2.0")?;
        state.serialize_field("id", &self.id)?;
        if let Some(result) = &self.result {
            state.serialize_field("result", result)?;
        }
        if let Some(error) = &self.error {
            state.serialize_field("error", error)?;
        }
        state.end()
    }
}

/// tsgo `lsproto.Message`.
#[derive(Clone, Debug)]
pub enum Message {
    Request(RequestMessage),
    Notification(RequestMessage),
    Response(ResponseMessage),
}

impl Serialize for Message {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Request(request) | Self::Notification(request) => request.serialize(serializer),
            Self::Response(response) => response.serialize(serializer),
        }
    }
}

/// An error with the protocol's code (tsgo returns an `ErrorCode`, or wraps
/// one with `%w`). Its message is the response error's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspError {
    pub code: ErrorCode,
    pub message: String,
}

impl LspError {
    /// tsgo `fmt.Errorf("%w: %s", code, detail)`.
    pub fn wrap(code: ErrorCode, detail: impl std::fmt::Display) -> Self {
        Self {
            code,
            message: format!("{code}: {detail}"),
        }
    }

    /// An error without a protocol code (tsgo's plain errors), answered as
    /// `InternalError`.
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::INTERNAL_ERROR,
            message: message.into(),
        }
    }
}

impl From<ErrorCode> for LspError {
    /// tsgo `ErrorCode.Error`: the code's name.
    fn from(code: ErrorCode) -> Self {
        Self {
            code,
            message: code.to_string(),
        }
    }
}

impl std::fmt::Display for LspError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LspError {}

/// Go's name of a JSON value's kind in a decoding error.
fn json_kind(raw: &str) -> &'static str {
    match raw.trim_start().as_bytes().first() {
        Some(b'"') => "string",
        Some(b'{') => "object",
        Some(b'[') => "array",
        Some(b't' | b'f') => "boolean",
        Some(b'n') => "null",
        _ => "number",
    }
}

/// tsgo `Message.UnmarshalJSON` under `lspReader.Read`: a message from its
/// JSON text.
pub fn parse_message(data: &[u8]) -> Result<Message, LspError> {
    let invalid = |error: &dyn std::fmt::Display| LspError::wrap(ErrorCode::INVALID_REQUEST, error);
    let fields: HashMap<String, RawJson> =
        serde_json::from_slice(data).map_err(|error| invalid(&error))?;
    if let Some(version) = fields.get("jsonrpc") {
        if version.get() != "\"2.0\"" {
            return Err(invalid(&format!(
                "json: cannot unmarshal JSON object into Go lsproto.Message: InvalidRequest: json: cannot unmarshal JSON {} into Go jsonrpc.JSONRPCVersion within \"/jsonrpc\": invalid JSON-RPC version",
                json_kind(version.get())
            )));
        }
    }
    let value = |raw: &RawJson| -> Result<Value, LspError> {
        serde_json::from_str(raw.get()).map_err(|error| invalid(&error))
    };
    let method = match fields.get("method") {
        Some(raw) => String::decode(&value(raw)?).map_err(|error| invalid(&error))?,
        None => String::new(),
    };
    let id = match fields.get("id") {
        Some(raw) if raw.get() != "null" => {
            Some(Id::decode(&value(raw)?).map_err(|error| invalid(&error))?)
        }
        _ => None,
    };
    let mut fields = fields;
    if id.is_some() && method.is_empty() {
        let error = match fields.get("error") {
            Some(raw) if raw.get() != "null" => {
                Some(ResponseError::decode(&value(raw)?).map_err(|error| invalid(&error))?)
            }
            _ => None,
        };
        return Ok(Message::Response(ResponseMessage {
            id,
            result: fields.remove("result"),
            error,
        }));
    }
    let request = RequestMessage {
        id,
        method,
        params: fields.remove("params"),
    };
    Ok(if request.id.is_none() {
        Message::Notification(request)
    } else {
        Message::Request(request)
    })
}

/// A method's params (tsgo `UnmarshalParams`): a method without params must
/// be given none; any other must be given an object or an array.
pub fn decode_params<T: Decode + 'static>(request: &RequestMessage) -> Result<T, LspError> {
    let invalid = |error: &dyn std::fmt::Display| LspError::wrap(ErrorCode::INVALID_PARAMS, error);
    if std::any::TypeId::of::<T>() == std::any::TypeId::of::<NoParams>() {
        if let Some(params) = &request.params {
            return Err(invalid(&format!(
                "expected no params, got {}",
                params.get()
            )));
        }
        return T::decode(&Value::Null).map_err(|error| invalid(&error));
    }
    let Some(params) = request
        .params
        .as_ref()
        .filter(|params| matches!(params.get().as_bytes().first(), Some(b'{' | b'[')))
    else {
        return Err(invalid(&"params must be an object or array"));
    };
    let value: Value = serde_json::from_str(params.get()).map_err(|error| invalid(&error))?;
    T::decode(&value).map_err(|error| {
        // Go's `json.Unmarshal` names the type its decoder failed for.
        let name = std::any::type_name::<T>();
        let name = name.rsplit("::").next().unwrap_or(name);
        invalid(&format!(
            "json: cannot unmarshal into Go lsproto.{name}: {error}"
        ))
    })
}

/// tsgo `RequestInfo.UnmarshalResult`: a response's result as the request's
/// response type.
pub fn decode_result<T: Decode>(result: Option<&RawJson>) -> Result<T, String> {
    let Some(result) = result else {
        return Err("jsontext: unexpected EOF".to_owned());
    };
    let value: Value = serde_json::from_str(result.get()).map_err(|error| error.to_string())?;
    T::decode(&value).map_err(|error| error.0)
}
