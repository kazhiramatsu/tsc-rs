//! tsgo `jsonrpc` (`baseproto.go`, `jsonrpc.go`) and `ipc.JSONRPCProtocol`:
//! JSON-RPC 2.0 messages framed by the LSP base protocol's
//! `Content-Length` header.

use std::io::{BufRead, BufReader, BufWriter, ErrorKind, Read, Write};

use base64::Engine;
use serde::de::{self, Deserializer, Visitor};
use serde::Deserialize;
use serde_json::value::RawValue;

use super::{Error, Id, Message, Payload, Protocol, ResponseError};

/// tsgo `jsonrpc.Reader`: the content of one message after its headers.
pub struct Reader<R> {
    reader: BufReader<R>,
}

impl<R: Read> Reader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
        }
    }

    /// The next message's content; nothing at the end of the input.
    pub fn read(&mut self) -> Result<Option<Vec<u8>>, Error> {
        let mut content_length = 0_i64;
        loop {
            let mut line = Vec::new();
            self.reader
                .read_until(b'\n', &mut line)
                .map_err(|error| Error::new(format!("jsonrpc: read header: {error}")))?;
            // Go's `ReadBytes` reports the end of the input for a line
            // without its newline.
            if !line.ends_with(b"\n") {
                return Ok(None);
            }
            if line == b"\r\n" {
                break;
            }
            let Some(colon) = line.iter().position(|&byte| byte == b':') else {
                return Err(Error::new(format!(
                    "jsonrpc: invalid header: {:?}",
                    String::from_utf8_lossy(&line)
                )));
            };
            if &line[..colon] == b"Content-Length" {
                let value = String::from_utf8_lossy(line[colon + 1..].trim_ascii());
                content_length = value.parse::<i64>().map_err(|error| {
                    let reason = match error.kind() {
                        std::num::IntErrorKind::PosOverflow
                        | std::num::IntErrorKind::NegOverflow => "value out of range",
                        _ => "invalid syntax",
                    };
                    Error::new(format!(
                        "jsonrpc: invalid content length: parse error: strconv.ParseInt: parsing {value:?}: {reason}"
                    ))
                })?;
                if content_length < 0 {
                    return Err(Error::new(format!(
                        "jsonrpc: invalid content length: negative value {content_length}"
                    )));
                }
            }
        }
        if content_length <= 0 {
            return Err(Error::new("jsonrpc: no content length"));
        }
        let length = usize::try_from(content_length)
            .map_err(|_| Error::new("jsonrpc: invalid content length: too large"))?;
        let mut data = vec![0; length];
        self.reader.read_exact(&mut data).map_err(|error| {
            let error = if error.kind() == ErrorKind::UnexpectedEof {
                "unexpected EOF".to_owned()
            } else {
                error.to_string()
            };
            Error::new(format!("jsonrpc: read content: {error}"))
        })?;
        Ok(Some(data))
    }
}

/// tsgo `jsonrpc.Writer`: one message with its `Content-Length` header.
pub struct Writer<W: Write> {
    writer: BufWriter<W>,
}

impl<W: Write> Writer<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer: BufWriter::new(writer),
        }
    }

    pub fn write(&mut self, data: &[u8]) -> Result<(), Error> {
        write!(self.writer, "Content-Length: {}\r\n\r\n", data.len())?;
        self.writer.write_all(data)?;
        self.writer.flush()?;
        Ok(())
    }
}

/// tsgo `ipc.JSONRPCProtocol`.
pub struct JsonRpcProtocol<R, W: Write> {
    reader: Reader<R>,
    writer: Writer<W>,
}

impl<R: Read, W: Write> JsonRpcProtocol<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: Reader::new(reader),
            writer: Writer::new(writer),
        }
    }
}

impl<R: Read + Send, W: Write + Send> Protocol for JsonRpcProtocol<R, W> {
    fn read_message(&mut self) -> Result<Option<Message>, Error> {
        match self.reader.read()? {
            Some(data) => parse_message(&data).map(Some),
            None => Ok(None),
        }
    }

    fn write_request(&mut self, id: &Id, method: &str, params: &str) -> Result<(), Error> {
        self.writer
            .write(request_text(Some(id), method, params).as_bytes())
    }

    fn write_notification(&mut self, method: &str, params: &str) -> Result<(), Error> {
        self.writer
            .write(request_text(None, method, params).as_bytes())
    }

    fn write_response(&mut self, id: &Id, result: &Payload) -> Result<(), Error> {
        let result = match result {
            Payload::Json(text) => text.clone(),
            // Go writes a byte slice as base64 text.
            Payload::Binary(bytes) => {
                quote(&base64::engine::general_purpose::STANDARD.encode(bytes))
            }
        };
        let text = format!(
            r#"{{"jsonrpc":"2.0","id":{},"result":{result}}}"#,
            id_text(id)
        );
        self.writer.write(text.as_bytes())
    }

    fn write_error(&mut self, id: &Id, error: &ResponseError) -> Result<(), Error> {
        let text = format!(
            r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":{},"message":{}}}}}"#,
            id_text(id),
            error.code,
            quote(&error.message)
        );
        self.writer.write(text.as_bytes())
    }
}

/// tsgo `jsonrpc.RequestMessage`: without an ID for a notification, and
/// without parameters when there are none.
fn request_text(id: Option<&Id>, method: &str, params: &str) -> String {
    let mut text = r#"{"jsonrpc":"2.0""#.to_owned();
    if let Some(id) = id {
        text.push_str(r#","id":"#);
        text.push_str(&id_text(id));
    }
    text.push_str(r#","method":"#);
    text.push_str(&quote(method));
    if !params.is_empty() {
        text.push_str(r#","params":"#);
        text.push_str(params);
    }
    text.push('}');
    text
}

fn id_text(id: &Id) -> String {
    match id {
        Id::String(text) => quote(text),
        Id::Int(number) => number.to_string(),
    }
}

fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("a string serializes")
}

/// tsgo's `json.Unmarshal` of a `jsonrpc.Message`: `jsonrpc` must be `"2.0"`
/// when present, and `params` and `result` keep their JSON text (`null`
/// included).
fn parse_message(data: &[u8]) -> Result<Message, Error> {
    #[derive(Deserialize)]
    struct WireMessage<'a> {
        #[serde(default, borrow, deserialize_with = "raw")]
        jsonrpc: Option<&'a RawValue>,
        #[serde(default)]
        id: Option<WireId>,
        #[serde(default)]
        method: Option<String>,
        #[serde(default, borrow, deserialize_with = "raw")]
        params: Option<&'a RawValue>,
        #[serde(default, borrow, deserialize_with = "raw")]
        result: Option<&'a RawValue>,
        #[serde(default)]
        error: Option<WireError>,
    }

    #[derive(Deserialize)]
    struct WireError {
        #[serde(default)]
        code: i32,
        #[serde(default)]
        message: String,
    }

    let message: WireMessage<'_> =
        serde_json::from_slice(data).map_err(|error| Error::new(error.to_string()))?;
    if message
        .jsonrpc
        .is_some_and(|version| version.get() != r#""2.0""#)
    {
        return Err(Error::new("invalid JSON-RPC version"));
    }
    let bytes = |value: Option<&RawValue>| {
        value
            .map(|value| value.get().as_bytes().to_vec())
            .unwrap_or_default()
    };
    Ok(Message {
        id: message.id.map(|id| id.0),
        method: message.method.unwrap_or_default(),
        params: bytes(message.params),
        result: bytes(message.result),
        error: message.error.map(|error| ResponseError {
            code: error.code,
            message: error.message,
        }),
    })
}

/// A present value's JSON text, `null` included.
fn raw<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<&'de RawValue>, D::Error> {
    <&RawValue>::deserialize(deserializer).map(Some)
}

/// tsgo `jsonrpc.ID`'s JSON: a string, or an integer that fits 32 bits.
struct WireId(Id);

impl<'de> Deserialize<'de> for WireId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct IdVisitor;

        impl Visitor<'_> for IdVisitor {
            type Value = WireId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a string or a 32-bit integer")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<WireId, E> {
                Ok(WireId(Id::String(value.to_owned())))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<WireId, E> {
                i32::try_from(value)
                    .map(|value| WireId(Id::Int(value)))
                    .map_err(|_| E::custom(format!("ID {value} does not fit 32 bits")))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<WireId, E> {
                i32::try_from(value)
                    .map(|value| WireId(Id::Int(value)))
                    .map_err(|_| E::custom(format!("ID {value} does not fit 32 bits")))
            }
        }

        deserializer.deserialize_any(IdVisitor)
    }
}
