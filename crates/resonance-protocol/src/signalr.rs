//! Original, bounded implementation of the public SignalR hub wire format.
//! Models belonging to osu! are adapted by the gateway, not by this framing layer.
use serde_json::{Value, json};
use std::io::{Cursor, Read};

pub const RECORD_SEPARATOR: u8 = 0x1e;
pub const MAX_RECORD: usize = 131_072;
const MAX_BUFFER: usize = MAX_RECORD * 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Json,
    MessagePack,
}

#[derive(Debug, PartialEq)]
pub enum Incoming {
    Invocation {
        id: Option<String>,
        target: String,
        arguments: Vec<Value>,
    },
    Ping,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError(pub &'static str);
impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for WireError {}

pub fn handshake(record: &[u8]) -> Result<Protocol, WireError> {
    if record.len() > 1024 {
        return Err(WireError("handshake_too_large"));
    }
    let value: Value =
        serde_json::from_slice(record).map_err(|_| WireError("invalid_handshake"))?;
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(WireError("unsupported_protocol_version"));
    }
    match value.get("protocol").and_then(Value::as_str) {
        Some("json") => Ok(Protocol::Json),
        Some("messagepack") => Ok(Protocol::MessagePack),
        _ => Err(WireError("unsupported_protocol")),
    }
}

pub struct Codec {
    protocol: Protocol,
    buffer: Vec<u8>,
}
impl Codec {
    pub fn new(protocol: Protocol) -> Self {
        Self {
            protocol,
            buffer: Vec::new(),
        }
    }
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Incoming>, WireError> {
        if self.buffer.len() + bytes.len() > MAX_BUFFER {
            return Err(WireError("buffer_too_large"));
        }
        self.buffer.extend_from_slice(bytes);
        let mut records = Vec::new();
        loop {
            let (prefix, length) = match self.protocol {
                Protocol::Json => match self.buffer.iter().position(|b| *b == RECORD_SEPARATOR) {
                    Some(n) => (0, n),
                    None => {
                        if self.buffer.len() > MAX_RECORD {
                            return Err(WireError("record_too_large"));
                        }
                        break;
                    }
                },
                Protocol::MessagePack => match length_prefix(&self.buffer)? {
                    Some((prefix, length)) => (prefix, length),
                    None => break,
                },
            };
            if length > MAX_RECORD {
                return Err(WireError("record_too_large"));
            }
            if self.buffer.len() < prefix + length {
                break;
            }
            let value = match self.protocol {
                Protocol::Json => serde_json::from_slice::<Value>(&self.buffer[..length])
                    .map_err(|_| WireError("invalid_json"))?,
                Protocol::MessagePack => {
                    let mut cursor = Cursor::new(&self.buffer[prefix..prefix + length]);
                    let value = read_native_value(&mut cursor, 48)?;
                    if cursor.position() != length as u64 {
                        return Err(WireError("trailing_messagepack"));
                    }
                    from_messagepack(value)?
                }
            };
            let consumed = prefix + length + usize::from(self.protocol == Protocol::Json);
            self.buffer.drain(..consumed);
            records.push(parse(value, self.protocol)?);
            if records.len() > 64 {
                return Err(WireError("too_many_records"));
            }
            if self.buffer.is_empty() {
                break;
            }
        }
        Ok(records)
    }
    pub fn invocation(&self, target: &str, arguments: Vec<Value>) -> Result<Vec<u8>, WireError> {
        self.invocation_with_id(None, target, arguments)
    }
    pub fn invocation_with_id(
        &self,
        id: Option<&str>,
        target: &str,
        arguments: Vec<Value>,
    ) -> Result<Vec<u8>, WireError> {
        self.encode(match self.protocol {
            Protocol::Json => {
                let mut value = json!({"type":1,"target":target,"arguments":arguments});
                if let Some(id) = id {
                    value["invocationId"] = id.into();
                }
                value
            }
            Protocol::MessagePack => json!([1, {}, id, target, arguments, []]),
        })
    }
    pub fn completion(
        &self,
        id: &str,
        result: Result<Option<Value>, &str>,
    ) -> Result<Vec<u8>, WireError> {
        let value = match (self.protocol, result) {
            (Protocol::Json, Ok(Some(result))) => {
                json!({"type":3,"invocationId":id,"result":result})
            }
            (Protocol::Json, Ok(None)) => json!({"type":3,"invocationId":id}),
            (Protocol::Json, Err(error)) => json!({"type":3,"invocationId":id,"error":error}),
            (Protocol::MessagePack, Ok(Some(result))) => json!([3, {}, id, 3, result]),
            (Protocol::MessagePack, Ok(None)) => json!([3, {}, id, 2]),
            (Protocol::MessagePack, Err(error)) => json!([3, {}, id, 1, error]),
        };
        self.encode(value)
    }
    pub fn ping(&self) -> Vec<u8> {
        self.encode(match self.protocol {
            Protocol::Json => json!({"type":6}),
            Protocol::MessagePack => json!([6]),
        })
        .unwrap_or_default()
    }
    pub fn close(&self, reason: &str) -> Vec<u8> {
        self.encode(match self.protocol {
            Protocol::Json => json!({"type":7,"error":reason,"allowReconnect":false}),
            Protocol::MessagePack => json!([7, reason, false]),
        })
        .unwrap_or_default()
    }
    fn encode(&self, value: Value) -> Result<Vec<u8>, WireError> {
        match self.protocol {
            Protocol::Json => {
                let mut bytes =
                    serde_json::to_vec(&value).map_err(|_| WireError("encoding_failed"))?;
                if bytes.len() > MAX_RECORD {
                    return Err(WireError("record_too_large"));
                }
                bytes.push(RECORD_SEPARATOR);
                Ok(bytes)
            }
            Protocol::MessagePack => {
                let value = to_messagepack(value)?;
                let mut bytes = Vec::new();
                write_native_value(&mut bytes, &value, 48)?;
                if bytes.len() > MAX_RECORD {
                    return Err(WireError("record_too_large"));
                }
                let mut result = Vec::new();
                let mut length = bytes.len();
                loop {
                    let next = (length & 0x7f) as u8;
                    length >>= 7;
                    result.push(next | if length > 0 { 0x80 } else { 0 });
                    if length == 0 {
                        break;
                    }
                }
                result.extend(bytes);
                Ok(result)
            }
        }
    }
}

/// Decode one complete record in either direction (invocation or completion).
pub fn decode_messagepack_record(bytes: &[u8]) -> Result<Value, WireError> {
    let (prefix, length) = length_prefix(bytes)?.ok_or(WireError("truncated_record"))?;
    if length > MAX_RECORD {
        return Err(WireError("record_too_large"));
    }
    if bytes.len() != prefix + length {
        return Err(WireError("invalid_record_length"));
    }
    let mut cursor = Cursor::new(&bytes[prefix..]);
    let value = read_native_value(&mut cursor, 48)?;
    if cursor.position() != length as u64 {
        return Err(WireError("trailing_messagepack"));
    }
    from_messagepack(value)
}
// osu!'s APIMod settings formatter encodes a pair count in an array header,
// followed by key/value tokens. That is deliberately not a normal MessagePack
// dictionary. Recognize the public APIMod [acronym, settings] shape so a bounded
// decoder can consume the correct number of tokens before returning to the hub.
fn acronym(value: &rmpv::Value) -> bool {
    value.as_str().is_some_and(|v| {
        !v.is_empty()
            && v.len() <= 8
            && v.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    })
}
fn container_length(cursor: &mut Cursor<&[u8]>, array: bool) -> Result<Option<usize>, WireError> {
    let Some(marker) = cursor.get_ref().get(cursor.position() as usize).copied() else {
        return Err(WireError("truncated_messagepack"));
    };
    let (small, wide16, wide32) = if array {
        (0x90, 0xdc, 0xdd)
    } else {
        (0x80, 0xde, 0xdf)
    };
    if marker & 0xf0 == small {
        cursor.set_position(cursor.position() + 1);
        return Ok(Some(usize::from(marker & 15)));
    }
    let length = if marker == wide16 {
        let mut bytes = [0u8; 2];
        cursor.set_position(cursor.position() + 1);
        cursor
            .read_exact(&mut bytes)
            .map_err(|_| WireError("truncated_messagepack"))?;
        usize::from(u16::from_be_bytes(bytes))
    } else if marker == wide32 {
        let mut bytes = [0u8; 4];
        cursor.set_position(cursor.position() + 1);
        cursor
            .read_exact(&mut bytes)
            .map_err(|_| WireError("truncated_messagepack"))?;
        u32::from_be_bytes(bytes) as usize
    } else {
        return Ok(None);
    };
    if length > MAX_RECORD {
        return Err(WireError("container_too_large"));
    }
    Ok(Some(length))
}
fn read_native_value(cursor: &mut Cursor<&[u8]>, depth: u16) -> Result<rmpv::Value, WireError> {
    use rmpv::Value as M;
    if depth == 0 {
        return Err(WireError("messagepack_depth_exceeded"));
    }
    if let Some(length) = container_length(cursor, true)? {
        let mut values = Vec::new();
        if length == 2 {
            let first = read_native_value(cursor, depth - 1)?;
            if acronym(&first)
                && let Some(count) = container_length(cursor, true)?
            {
                if count > 16 {
                    return Err(WireError("too_many_mod_settings"));
                }
                let mut settings = Vec::new();
                for _ in 0..count {
                    let key = read_native_value(cursor, depth - 1)?;
                    if key.as_str().is_none() {
                        return Err(WireError("invalid_mod_setting_key"));
                    }
                    let value = read_native_value(cursor, depth - 1)?;
                    settings.push((key, value));
                }
                return Ok(M::Array(vec![first, M::Map(settings)]));
            }
            values.push(first);
        }
        while values.len() < length {
            values.push(read_native_value(cursor, depth - 1)?);
        }
        return Ok(M::Array(values));
    }
    if let Some(length) = container_length(cursor, false)? {
        let mut values = Vec::new();
        for _ in 0..length {
            values.push((
                read_native_value(cursor, depth - 1)?,
                read_native_value(cursor, depth - 1)?,
            ));
        }
        return Ok(M::Map(values));
    }
    rmpv::decode::read_value_with_max_depth(cursor, usize::from(depth))
        .map_err(|_| WireError("invalid_messagepack"))
}
fn write_container(bytes: &mut Vec<u8>, length: usize, array: bool) {
    let (small, wide16, wide32) = if array {
        (0x90, 0xdc, 0xdd)
    } else {
        (0x80, 0xde, 0xdf)
    };
    if length < 16 {
        bytes.push(small | length as u8);
    } else if length <= usize::from(u16::MAX) {
        bytes.push(wide16);
        bytes.extend((length as u16).to_be_bytes());
    } else {
        bytes.push(wide32);
        bytes.extend((length as u32).to_be_bytes());
    }
}
fn write_native_value(
    bytes: &mut Vec<u8>,
    value: &rmpv::Value,
    depth: u16,
) -> Result<(), WireError> {
    use rmpv::Value as M;
    if depth == 0 {
        return Err(WireError("messagepack_depth_exceeded"));
    }
    match value {
        M::Array(values) => {
            write_container(bytes, values.len(), true);
            if values.len() == 2
                && acronym(&values[0])
                && let M::Map(settings) = &values[1]
            {
                if settings.len() > 16 {
                    return Err(WireError("too_many_mod_settings"));
                }
                write_native_value(bytes, &values[0], depth - 1)?;
                write_container(bytes, settings.len(), true);
                for (key, value) in settings {
                    write_native_value(bytes, key, depth - 1)?;
                    write_native_value(bytes, value, depth - 1)?;
                }
                return Ok(());
            }
            for value in values {
                write_native_value(bytes, value, depth - 1)?;
            }
        }
        M::Map(values) => {
            write_container(bytes, values.len(), false);
            for (key, value) in values {
                write_native_value(bytes, key, depth - 1)?;
                write_native_value(bytes, value, depth - 1)?;
            }
        }
        _ => rmpv::encode::write_value(bytes, value).map_err(|_| WireError("encoding_failed"))?,
    }
    Ok(())
}
fn length_prefix(bytes: &[u8]) -> Result<Option<(usize, usize)>, WireError> {
    let mut length = 0usize;
    for (i, byte) in bytes.iter().take(5).enumerate() {
        if i == 4 && *byte > 7 {
            return Err(WireError("invalid_length_prefix"));
        }
        length |= usize::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(Some((i + 1, length)));
        }
    }
    if bytes.len() >= 5 {
        return Err(WireError("invalid_length_prefix"));
    }
    Ok(None)
}
fn parse(value: Value, protocol: Protocol) -> Result<Incoming, WireError> {
    let kind = if protocol == Protocol::Json {
        value.get("type")
    } else {
        value.get(0)
    }
    .and_then(Value::as_i64)
    .ok_or(WireError("invalid_message_type"))?;
    match kind {
        1 => {
            let (id, target, arguments) = match protocol {
                Protocol::Json => (
                    value.get("invocationId"),
                    value.get("target"),
                    value.get("arguments"),
                ),
                Protocol::MessagePack => (value.get(2), value.get(3), value.get(4)),
            };
            let id = match id {
                None | Some(Value::Null) => None,
                Some(Value::String(id)) if !id.is_empty() && id.len() <= 64 => Some(id.clone()),
                _ => return Err(WireError("invalid_invocation_id")),
            };
            let target = target
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty() && v.len() <= 64)
                .ok_or(WireError("invalid_target"))?
                .to_owned();
            let arguments = arguments
                .and_then(Value::as_array)
                .filter(|v| v.len() <= 16)
                .ok_or(WireError("invalid_arguments"))?
                .clone();
            Ok(Incoming::Invocation {
                id,
                target,
                arguments,
            })
        }
        6 => Ok(Incoming::Ping),
        7 => Ok(Incoming::Close),
        _ => Err(WireError("unsupported_message_type")),
    }
}
fn from_messagepack(value: rmpv::Value) -> Result<Value, WireError> {
    use rmpv::Value as M;
    Ok(match value {
        M::Nil => Value::Null,
        M::Boolean(v) => v.into(),
        M::Integer(v) => {
            if let Some(v) = v.as_i64() {
                v.into()
            } else if let Some(v) = v.as_u64() {
                v.into()
            } else {
                return Err(WireError("invalid_integer"));
            }
        }
        M::F32(v) => json!(v),
        M::F64(v) => json!(v),
        M::String(v) => v.as_str().ok_or(WireError("invalid_utf8"))?.into(),
        M::Array(v) => Value::Array(
            v.into_iter()
                .map(from_messagepack)
                .collect::<Result<_, _>>()?,
        ),
        M::Map(v) => {
            let mut map = serde_json::Map::new();
            for (key, value) in v {
                let key = match key {
                    M::String(v) => v.as_str().ok_or(WireError("invalid_utf8"))?.to_owned(),
                    M::Integer(v) => v.to_string(),
                    _ => return Err(WireError("invalid_map_key")),
                };
                if map.insert(key, from_messagepack(value)?).is_some() {
                    return Err(WireError("duplicate_map_key"));
                }
            }
            Value::Object(map)
        }
        // Keep framework timestamps and replay extensions opaque while relaying.
        M::Ext(kind, bytes) => json!({"$msgpack_ext":[kind,bytes]}),
        M::Binary(bytes) => json!({"$msgpack_binary":bytes}),
    })
}
fn to_messagepack(value: Value) -> Result<rmpv::Value, WireError> {
    use rmpv::Value as M;
    Ok(match value {
        Value::Null => M::Nil,
        Value::Bool(v) => M::Boolean(v),
        Value::String(v) => M::from(v),
        Value::Number(v) => {
            if let Some(v) = v.as_i64() {
                M::from(v)
            } else if let Some(v) = v.as_u64() {
                M::from(v)
            } else {
                M::F64(v.as_f64().ok_or(WireError("invalid_number"))?)
            }
        }
        Value::Array(v) => M::Array(
            v.into_iter()
                .map(to_messagepack)
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(mut v) => {
            if v.len() == 1 && v.contains_key("$msgpack_ext") {
                let ext = v.remove("$msgpack_ext").unwrap();
                let ext = ext.as_array().ok_or(WireError("invalid_extension"))?;
                let kind = ext
                    .first()
                    .and_then(Value::as_i64)
                    .and_then(|n| i8::try_from(n).ok())
                    .ok_or(WireError("invalid_extension"))?;
                let bytes = byte_array(ext.get(1).ok_or(WireError("invalid_extension"))?)?;
                M::Ext(kind, bytes)
            } else if v.len() == 1 && v.contains_key("$msgpack_binary") {
                M::Binary(byte_array(&v.remove("$msgpack_binary").unwrap())?)
            } else {
                M::Map(
                    v.into_iter()
                        .map(|(key, value)| {
                            Ok((
                                key.parse::<i64>()
                                    .map(M::from)
                                    .unwrap_or_else(|_| M::from(key)),
                                to_messagepack(value)?,
                            ))
                        })
                        .collect::<Result<_, WireError>>()?,
                )
            }
        }
    })
}
fn byte_array(value: &Value) -> Result<Vec<u8>, WireError> {
    value
        .as_array()
        .ok_or(WireError("invalid_binary"))?
        .iter()
        .map(|v| {
            v.as_u64()
                .and_then(|v| u8::try_from(v).ok())
                .ok_or(WireError("invalid_binary"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_json_and_multiple_records() {
        let mut codec = Codec::new(Protocol::Json);
        assert!(
            codec
                .feed(b"{\"type\":1,\"invocationId\":\"1\",\"target\":\"Join")
                .unwrap()
                .is_empty()
        );
        let records = codec
            .feed(b"Room\",\"arguments\":[12]}\x1e{\"type\":6}\x1e")
            .unwrap();
        assert_eq!(
            records,
            vec![
                Incoming::Invocation {
                    id: Some("1".into()),
                    target: "JoinRoom".into(),
                    arguments: vec![json!(12)]
                },
                Incoming::Ping
            ]
        );
    }
    #[test]
    fn native_mod_setting_formatter_pair_count() {
        let codec = Codec::new(Protocol::MessagePack);
        let args = vec![json!([["DT",{"speed_change":1.5}],["HD",{}]])];
        let bytes = codec
            .invocation_with_id(Some("7"), "ChangeUserMods", args.clone())
            .unwrap();
        let decoded = decode_messagepack_record(&bytes).unwrap();
        assert_eq!(decoded[4], json!(args));
        assert!(bytes.windows(3).any(|w| w == [0x91, 0xac, b's'])); // settings pair count followed by a string key
        assert_eq!(
            Codec::new(Protocol::MessagePack).feed(&bytes).unwrap(),
            vec![Incoming::Invocation {
                id: Some("7".into()),
                target: "ChangeUserMods".into(),
                arguments: args
            }]
        );
    }
    #[test]
    fn messagepack_roundtrip_preserves_integer_dictionary_and_extensions() {
        let codec = Codec::new(Protocol::MessagePack);
        let arguments = vec![json!({"300":12}), json!({"$msgpack_ext":[-1,[0,1,2,3]]})];
        let bytes = codec.invocation("Frames", arguments.clone()).unwrap();
        let mut decoder = Codec::new(Protocol::MessagePack);
        assert!(decoder.feed(&bytes[..1]).unwrap().is_empty());
        assert_eq!(
            decoder.feed(&bytes[1..]).unwrap(),
            vec![Incoming::Invocation {
                id: None,
                target: "Frames".into(),
                arguments
            }]
        );
    }
    #[test]
    fn rejects_length_depth_and_wrong_handshake() {
        assert!(
            Codec::new(Protocol::MessagePack)
                .feed(&[255, 255, 255, 255, 255])
                .is_err()
        );
        assert!(
            Codec::new(Protocol::MessagePack)
                .feed(&[255, 255, 127])
                .is_err()
        );
        assert!(handshake(br#"{"protocol":"json","version":2}"#).is_err());
        let mut payload = vec![0x91; 80];
        payload.push(0xc0);
        let mut bytes = vec![payload.len() as u8];
        bytes.extend(payload);
        assert!(Codec::new(Protocol::MessagePack).feed(&bytes).is_err());
    }
}
