//! tsgo `api/protocol_msgpack.go`: the synchronous API's protocol, each
//! message a MessagePack array `[type, method, payload]` whose method and
//! payload are binary strings. The payload is JSON, or the raw bytes of a
//! binary response; a request's method is its ID.

use std::io::{BufReader, BufWriter, ErrorKind, Read, Write};

use crate::ipc::{Error, Id, Message, Payload, Protocol, ResponseError};

/// tsgo `MessageType`.
const MESSAGE_TYPE_REQUEST: u8 = 1;
const MESSAGE_TYPE_CALL_RESPONSE: u8 = 2;
const MESSAGE_TYPE_CALL_ERROR: u8 = 3;
const MESSAGE_TYPE_RESPONSE: u8 = 4;
const MESSAGE_TYPE_ERROR: u8 = 5;
const MESSAGE_TYPE_CALL: u8 = 6;

const FIXED_ARRAY_3: u8 = 0x93;
const BIN_8: u8 = 0xC4;
const BIN_16: u8 = 0xC5;
const BIN_32: u8 = 0xC6;
const U8: u8 = 0xCC;

/// tsgo `ErrInvalidRequest`'s text, which a malformed message's error
/// starts with.
const INVALID_REQUEST: &str = "api: invalid request";

/// tsgo `MessagePackProtocol`.
pub struct MessagePackProtocol<R, W: Write> {
    reader: BufReader<R>,
    writer: BufWriter<W>,
}

/// A read that produced a value, failed, or met the end of the input
/// before its first byte (`Ok(None)`, which ends the connection as Go's
/// `io.EOF` does).
type ReadResult<T> = Result<Option<T>, Error>;

impl<R: Read, W: Write> MessagePackProtocol<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer: BufWriter::new(writer),
        }
    }

    fn read_byte(&mut self) -> ReadResult<u8> {
        Ok(self.read_full(1)?.map(|byte| byte[0]))
    }

    /// Go's `io.ReadFull`: the end of the input before the first byte ends
    /// the connection, after it the message is cut short.
    fn read_full(&mut self, length: usize) -> ReadResult<Vec<u8>> {
        let mut data = vec![0; length];
        let mut filled = 0;
        while filled < length {
            match self.reader.read(&mut data[filled..]) {
                Ok(0) if filled == 0 => return Ok(None),
                Ok(0) => return Err(Error::new("unexpected EOF")),
                Ok(read) => filled += read,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(Some(data))
    }

    fn read_tuple(&mut self) -> ReadResult<(u8, String, Vec<u8>)> {
        let Some(marker) = self.read_byte()? else {
            return Ok(None);
        };
        if marker != FIXED_ARRAY_3 {
            return Err(Error::new(format!(
                "{INVALID_REQUEST}: expected fixed 3-element array (0x93), received: 0x{marker:02x}"
            )));
        }
        let Some(marker) = self.read_byte()? else {
            return Ok(None);
        };
        let message_type = if marker <= 0x7F {
            marker
        } else if marker == U8 {
            let Some(value) = self.read_byte()? else {
                return Ok(None);
            };
            value
        } else {
            return Err(Error::new(format!(
                "{INVALID_REQUEST}: expected positive fixint or uint8 marker, received: 0x{marker:02x}"
            )));
        };
        if !(MESSAGE_TYPE_REQUEST..=MESSAGE_TYPE_CALL).contains(&message_type) {
            return Err(Error::new(format!(
                "{INVALID_REQUEST}: unknown message type: {message_type}"
            )));
        }
        let Some(method) = self.read_bin()? else {
            return Ok(None);
        };
        let Some(payload) = self.read_bin()? else {
            return Ok(None);
        };
        Ok(Some((
            message_type,
            String::from_utf8_lossy(&method).into_owned(),
            payload,
        )))
    }

    fn read_bin(&mut self) -> ReadResult<Vec<u8>> {
        let Some(marker) = self.read_byte()? else {
            return Ok(None);
        };
        let width = match marker {
            BIN_8 => 1,
            BIN_16 => 2,
            BIN_32 => 4,
            _ => {
                return Err(Error::new(format!(
                    "{INVALID_REQUEST}: expected binary data (0xc4-0xc6), received: 0x{marker:02x}"
                )))
            }
        };
        let Some(size) = self.read_full(width)? else {
            return Ok(None);
        };
        let size = size
            .iter()
            .fold(0_usize, |size, &byte| (size << 8) | usize::from(byte));
        self.read_full(size)
    }

    fn write_tuple(&mut self, message_type: u8, method: &str, payload: &[u8]) -> Result<(), Error> {
        self.writer.write_all(&[FIXED_ARRAY_3, message_type])?;
        self.write_bin(method.as_bytes())?;
        self.write_bin(payload)?;
        self.writer.flush()?;
        Ok(())
    }

    fn write_bin(&mut self, data: &[u8]) -> Result<(), Error> {
        let length = data.len();
        if let Ok(length) = u8::try_from(length) {
            self.writer.write_all(&[BIN_8, length])?;
        } else if let Ok(length) = u16::try_from(length) {
            self.writer.write_all(&[BIN_16])?;
            self.writer.write_all(&length.to_be_bytes())?;
        } else {
            let length = u32::try_from(length)
                .map_err(|_| Error::new("a MessagePack binary holds at most 4 GiB"))?;
            self.writer.write_all(&[BIN_32])?;
            self.writer.write_all(&length.to_be_bytes())?;
        }
        self.writer.write_all(data)?;
        Ok(())
    }
}

impl<R: Read + Send, W: Write + Send> Protocol for MessagePackProtocol<R, W> {
    fn read_message(&mut self) -> Result<Option<Message>, Error> {
        let Some((message_type, method, payload)) = self.read_tuple()? else {
            return Ok(None);
        };
        let id = Some(Id::String(method.clone()));
        Ok(Some(match message_type {
            MESSAGE_TYPE_REQUEST => Message {
                id,
                method,
                params: payload,
                ..Message::default()
            },
            // A response's method is empty, its ID the call's method.
            MESSAGE_TYPE_CALL_RESPONSE => Message {
                id,
                result: payload,
                ..Message::default()
            },
            MESSAGE_TYPE_CALL_ERROR => Message {
                id,
                error: Some(ResponseError::internal(
                    String::from_utf8_lossy(&payload).into_owned(),
                )),
                ..Message::default()
            },
            _ => {
                return Err(Error::new(format!(
                    "unexpected message type: {message_type}"
                )))
            }
        }))
    }

    fn write_request(&mut self, _id: &Id, method: &str, params: &str) -> Result<(), Error> {
        // tsgo marshals the parameters: none are `null`.
        let params = if params.is_empty() { "null" } else { params };
        self.write_tuple(MESSAGE_TYPE_CALL, method, params.as_bytes())
    }

    fn write_notification(&mut self, method: &str, params: &str) -> Result<(), Error> {
        // The protocol does not tell a notification from a call.
        let params = if params.is_empty() { "null" } else { params };
        self.write_tuple(MESSAGE_TYPE_CALL, method, params.as_bytes())
    }

    fn write_response(&mut self, id: &Id, result: &Payload) -> Result<(), Error> {
        let payload = match result {
            Payload::Json(text) => text.as_bytes(),
            Payload::Binary(bytes) => bytes,
        };
        let method = id.to_string();
        self.write_tuple(MESSAGE_TYPE_RESPONSE, &method, payload)
    }

    fn write_error(&mut self, id: &Id, error: &ResponseError) -> Result<(), Error> {
        let method = id.to_string();
        self.write_tuple(MESSAGE_TYPE_ERROR, &method, error.message.as_bytes())
    }
}
