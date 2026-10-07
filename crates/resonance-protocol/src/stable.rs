//! osu! stable's HTTP-polled Bancho binary format.
//! Packet IDs and field ordering are interoperability facts; the implementation
//! is written independently and does not incorporate reference-server code.

use crate::{ProtocolError, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const HEADER_LEN: usize = 7;
pub const MAX_PACKET_BYTES: usize = 1_048_576;
pub const MAX_REQUEST_BYTES: usize = 4_194_304;
pub const MAX_STRING_BYTES: usize = 16_384;
pub const MAX_PACKETS: usize = 4096;

pub mod client {
    pub const CHANGE_ACTION: u16 = 0;
    pub const SEND_PUBLIC_MESSAGE: u16 = 1;
    pub const LOGOUT: u16 = 2;
    pub const REQUEST_STATUS_UPDATE: u16 = 3;
    pub const PING: u16 = 4;
    pub const START_SPECTATING: u16 = 16;
    pub const STOP_SPECTATING: u16 = 17;
    pub const SPECTATE_FRAMES: u16 = 18;
    pub const SEND_PRIVATE_MESSAGE: u16 = 25;
    pub const JOIN_LOBBY: u16 = 30;
    pub const CREATE_MATCH: u16 = 31;
    pub const JOIN_MATCH: u16 = 32;
    pub const PART_MATCH: u16 = 33;
    pub const CHANNEL_JOIN: u16 = 63;
    pub const CHANNEL_PART: u16 = 78;
    pub const FRIEND_ADD: u16 = 73;
    pub const FRIEND_REMOVE: u16 = 74;
    pub const TOGGLE_BLOCK_NON_FRIEND_DMS: u16 = 99;
    pub const USER_STATS_REQUEST: u16 = 85;
    pub const USER_PRESENCE_REQUEST: u16 = 97;
}

pub mod server {
    pub const USER_ID: u16 = 5;
    pub const SEND_MESSAGE: u16 = 7;
    pub const PONG: u16 = 8;
    pub const USER_STATS: u16 = 11;
    pub const USER_LOGOUT: u16 = 12;
    pub const NOTIFICATION: u16 = 24;
    pub const CHANNEL_JOIN_SUCCESS: u16 = 64;
    pub const CHANNEL_INFO: u16 = 65;
    pub const CHANNEL_KICK: u16 = 66;
    pub const CHANNEL_AUTO_JOIN: u16 = 67;
    pub const PRIVILEGES: u16 = 71;
    pub const FRIENDS_LIST: u16 = 72;
    pub const PROTOCOL_VERSION: u16 = 75;
    pub const USER_PRESENCE: u16 = 83;
    pub const RESTART: u16 = 86;
    pub const CHANNEL_INFO_END: u16 = 89;
    pub const SILENCE_END: u16 = 92;
    pub const USER_DM_BLOCKED: u16 = 100;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet<'a> {
    pub id: u16,
    pub payload: &'a [u8],
}

/// Validate the complete request before executing any contained operation.
/// Unknown IDs are preserved so the gateway can ignore them deliberately.
pub fn decode_packets(body: &[u8]) -> Result<Vec<Packet<'_>>> {
    if body.len() > MAX_REQUEST_BYTES {
        return Err(ProtocolError::Limit("request size"));
    }
    let mut packets = Vec::new();
    let mut offset = 0;
    while offset < body.len() {
        if packets.len() == MAX_PACKETS {
            return Err(ProtocolError::Limit("packet count"));
        }
        let header = body
            .get(offset..offset + HEADER_LEN)
            .ok_or(ProtocolError::Truncated)?;
        let id = u16::from_le_bytes([header[0], header[1]]);
        if header[2] != 0 {
            return Err(ProtocolError::Invalid("packet reserved byte"));
        }
        let length = u32::from_le_bytes(header[3..7].try_into().unwrap()) as usize;
        if length > MAX_PACKET_BYTES {
            return Err(ProtocolError::Limit("packet size"));
        }
        offset += HEADER_LEN;
        let end = offset
            .checked_add(length)
            .ok_or(ProtocolError::Limit("packet size"))?;
        let payload = body.get(offset..end).ok_or(ProtocolError::Truncated)?;
        packets.push(Packet { id, payload });
        offset = end;
    }
    Ok(packets)
}

pub fn encode_packet(id: u16, payload: &[u8]) -> Result<Vec<u8>> {
    if payload.len() > MAX_PACKET_BYTES {
        return Err(ProtocolError::Limit("packet size"));
    }
    let mut output = Vec::with_capacity(HEADER_LEN + payload.len());
    output.extend(id.to_le_bytes());
    output.push(0);
    output.extend((payload.len() as u32).to_le_bytes());
    output.extend(payload);
    Ok(output)
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
    pub fn finish(self) -> Result<()> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(ProtocolError::TrailingData)
        }
    }
    pub fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(ProtocolError::Limit("field size"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ProtocolError::Truncated)?;
        self.offset = end;
        Ok(value)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn boolean(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ProtocolError::Invalid("boolean")),
        }
    }
    pub fn string(&mut self) -> Result<&'a str> {
        match self.u8()? {
            0 => return Ok(""),
            11 => {}
            _ => return Err(ProtocolError::Invalid("string marker")),
        }
        let mut length = 0u32;
        for byte_index in 0..5 {
            let byte = self.u8()?;
            if byte_index == 4 && byte & 0xf0 != 0 {
                return Err(ProtocolError::Invalid("ULEB128 length"));
            }
            length |= u32::from(byte & 0x7f) << (7 * byte_index);
            if byte & 0x80 == 0 {
                if length as usize > MAX_STRING_BYTES {
                    return Err(ProtocolError::Limit("string size"));
                }
                return std::str::from_utf8(self.take(length as usize)?)
                    .map_err(|_| ProtocolError::Utf8);
            }
        }
        Err(ProtocolError::Invalid("ULEB128 length"))
    }
    pub fn i32_list(&mut self) -> Result<Vec<i32>> {
        let length = self.u16()? as usize;
        if length > MAX_PACKETS {
            return Err(ProtocolError::Limit("integer list"));
        }
        if self.remaining() < length * 4 {
            return Err(ProtocolError::Truncated);
        }
        (0..length).map(|_| self.i32()).collect()
    }
}

#[derive(Default)]
pub struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
    pub fn u8(&mut self, value: u8) -> &mut Self {
        self.bytes.push(value);
        self
    }
    pub fn u16(&mut self, value: u16) -> &mut Self {
        self.bytes.extend(value.to_le_bytes());
        self
    }
    pub fn i32(&mut self, value: i32) -> &mut Self {
        self.bytes.extend(value.to_le_bytes());
        self
    }
    pub fn u32(&mut self, value: u32) -> &mut Self {
        self.bytes.extend(value.to_le_bytes());
        self
    }
    pub fn i64(&mut self, value: i64) -> &mut Self {
        self.bytes.extend(value.to_le_bytes());
        self
    }
    pub fn f32(&mut self, value: f32) -> &mut Self {
        self.bytes.extend(value.to_le_bytes());
        self
    }
    pub fn f64(&mut self, value: f64) -> &mut Self {
        self.bytes.extend(value.to_le_bytes());
        self
    }
    pub fn boolean(&mut self, value: bool) -> &mut Self {
        self.u8(u8::from(value))
    }
    pub fn string(&mut self, value: &str) -> Result<&mut Self> {
        if value.len() > MAX_STRING_BYTES {
            return Err(ProtocolError::Limit("string size"));
        }
        if value.is_empty() {
            self.bytes.push(0);
            return Ok(self);
        }
        self.bytes.push(11);
        let mut length = value.len();
        loop {
            let mut byte = (length & 0x7f) as u8;
            length >>= 7;
            if length != 0 {
                byte |= 0x80;
            }
            self.bytes.push(byte);
            if length == 0 {
                break;
            }
        }
        self.bytes.extend(value.as_bytes());
        Ok(self)
    }
    pub fn i32_list(&mut self, values: &[i32]) -> Result<&mut Self> {
        if values.len() > MAX_PACKETS {
            return Err(ProtocolError::Limit("integer list"));
        }
        self.u16(values.len() as u16);
        for value in values {
            self.i32(*value);
        }
        Ok(self)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct LoginRequest {
    pub username: String,
    pub password_md5: String,
    pub client_version: String,
    pub utc_offset: i8,
    pub display_city: bool,
    /// Hardware identifiers are sensitive. Do not log or expose these.
    pub client_hashes: String,
    pub block_non_friend_dms: bool,
}

impl fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginRequest")
            .field("username", &self.username)
            .field("client_version", &self.client_version)
            .field("credentials", &"[redacted]")
            .finish()
    }
}

impl LoginRequest {
    pub fn parse(body: &[u8]) -> Result<Self> {
        if body.len() > 8192 {
            return Err(ProtocolError::Limit("login size"));
        }
        let body = std::str::from_utf8(body).map_err(|_| ProtocolError::Utf8)?;
        let lines: Vec<_> = body.trim_end_matches('\n').split('\n').collect();
        if lines.len() != 3 {
            return Err(ProtocolError::Invalid("login line count"));
        }
        let username = lines[0];
        if username.is_empty() || username.len() > 64 || username.chars().any(char::is_control) {
            return Err(ProtocolError::Invalid("username"));
        }
        if !is_md5(lines[1]) {
            return Err(ProtocolError::Invalid("password digest"));
        }
        let fields: Vec<_> = lines[2].split('|').collect();
        if fields.len() != 5 {
            return Err(ProtocolError::Invalid("client metadata"));
        }
        let client_version = fields[0];
        if client_version.is_empty()
            || client_version.len() > 64
            || !client_version
                .bytes()
                .all(|v| v.is_ascii_alphanumeric() || matches!(v, b'.' | b'-'))
        {
            return Err(ProtocolError::Invalid("client version"));
        }
        let utc_offset: i8 = fields[1]
            .parse()
            .map_err(|_| ProtocolError::Invalid("UTC offset"))?;
        if !(-24..=24).contains(&utc_offset) {
            return Err(ProtocolError::Invalid("UTC offset"));
        }
        let boolean = |field| match field {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(ProtocolError::Invalid("login boolean")),
        };
        if fields[3].len() > 4096 || fields[3].chars().any(char::is_control) {
            return Err(ProtocolError::Invalid("client hashes"));
        }
        Ok(Self {
            username: username.to_owned(),
            password_md5: lines[1].to_ascii_lowercase(),
            client_version: client_version.to_owned(),
            utc_offset,
            display_city: boolean(fields[2])?,
            client_hashes: fields[3].to_owned(),
            block_non_friend_dms: boolean(fields[4])?,
        })
    }
}

pub fn is_md5(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub sender: String,
    pub text: String,
    pub recipient: String,
    pub sender_id: i32,
}

impl ChatMessage {
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(payload);
        let message = Self {
            sender: reader.string()?.to_owned(),
            text: reader.string()?.to_owned(),
            recipient: reader.string()?.to_owned(),
            sender_id: reader.i32()?,
        };
        reader.finish()?;
        if message.text.len() > 4096
            || message.recipient.is_empty()
            || message.recipient.len() > 128
            || message.text.contains('\0')
        {
            return Err(ProtocolError::Invalid("chat message"));
        }
        Ok(message)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut writer = Writer::new();
        writer
            .string(&self.sender)?
            .string(&self.text)?
            .string(&self.recipient)?
            .i32(self.sender_id);
        let bytes = writer.into_bytes();
        Self::decode(&bytes)?;
        Ok(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UserAction {
    pub action: u8,
    pub info_text: String,
    pub beatmap_md5: String,
    pub mods: u32,
    pub mode: u8,
    pub beatmap_id: i32,
}

impl UserAction {
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(payload);
        let value = Self {
            action: reader.u8()?,
            info_text: reader.string()?.to_owned(),
            beatmap_md5: reader.string()?.to_owned(),
            mods: reader.u32()?,
            mode: reader.u8()?,
            beatmap_id: reader.i32()?,
        };
        reader.finish()?;
        if value.action > 13
            || value.mode > 3
            || value.info_text.len() > 1024
            || (!value.beatmap_md5.is_empty() && !is_md5(&value.beatmap_md5))
        {
            return Err(ProtocolError::Invalid("user action"));
        }
        Ok(value)
    }
    pub fn write_to(&self, writer: &mut Writer) -> Result<()> {
        writer
            .u8(self.action)
            .string(&self.info_text)?
            .string(&self.beatmap_md5)?
            .u32(self.mods)
            .u8(self.mode)
            .i32(self.beatmap_id);
        Ok(())
    }
}

/// Client privilege flags are a presentation contract. Never use these flags as
/// Illyaverse's server-side authorization decisions.
pub const CLIENT_PRIVILEGE_NORMAL: u32 = 1;

pub fn int_packet(id: u16, value: i32) -> Result<Vec<u8>> {
    encode_packet(id, &value.to_le_bytes())
}
pub fn string_packet(id: u16, value: &str) -> Result<Vec<u8>> {
    let mut writer = Writer::new();
    writer.string(value)?;
    encode_packet(id, &writer.into_bytes())
}
pub fn channel_info(name: &str, topic: &str, members: u16) -> Result<Vec<u8>> {
    channel_packet(server::CHANNEL_INFO, name, topic, members)
}
pub fn channel_packet(id: u16, name: &str, topic: &str, members: u16) -> Result<Vec<u8>> {
    let mut writer = Writer::new();
    writer.string(name)?.string(topic)?.u16(members);
    encode_packet(id, &writer.into_bytes())
}
pub fn friends_list(ids: &[i32]) -> Result<Vec<u8>> {
    let mut writer = Writer::new();
    writer.i32_list(ids)?;
    encode_packet(server::FRIENDS_LIST, &writer.into_bytes())
}

#[derive(Debug, Clone)]
pub struct Presence {
    pub user_id: i32,
    pub username: String,
    pub utc_offset: i8,
    pub country_code: u8,
    pub client_privileges: u8,
    pub mode: u8,
    pub longitude: f32,
    pub latitude: f32,
    pub rank: i32,
}

impl Presence {
    pub fn packet(&self) -> Result<Vec<u8>> {
        if !(-24..=24).contains(&self.utc_offset)
            || self.mode > 3
            || !self.longitude.is_finite()
            || !self.latitude.is_finite()
            || self.longitude.abs() > 180.0
            || self.latitude.abs() > 90.0
        {
            return Err(ProtocolError::Invalid("presence"));
        }
        let mut writer = Writer::new();
        writer
            .i32(self.user_id)
            .string(&self.username)?
            .u8((i16::from(self.utc_offset) + 24) as u8)
            .u8(self.country_code)
            .u8((self.client_privileges & 31) | self.mode << 5)
            .f32(self.longitude)
            .f32(self.latitude)
            .i32(self.rank);
        encode_packet(server::USER_PRESENCE, &writer.into_bytes())
    }
}

#[derive(Debug, Clone, Default)]
pub struct UserStats {
    pub user_id: i32,
    pub action: UserAction,
    pub ranked_score: i64,
    /// Accuracy is represented as a fraction from 0.0 to 1.0 on the wire.
    pub accuracy: f32,
    pub play_count: i32,
    pub total_score: i64,
    pub rank: i32,
    pub pp: u16,
}

impl UserStats {
    pub fn packet(&self) -> Result<Vec<u8>> {
        if !self.accuracy.is_finite()
            || !(0.0..=1.0).contains(&self.accuracy)
            || self.ranked_score < 0
            || self.total_score < 0
            || self.play_count < 0
            || self.rank < 0
        {
            return Err(ProtocolError::Invalid("user stats"));
        }
        let mut writer = Writer::new();
        writer.i32(self.user_id);
        self.action.write_to(&mut writer)?;
        writer
            .i64(self.ranked_score)
            .f32(self.accuracy)
            .i32(self.play_count)
            .i64(self.total_score)
            .i32(self.rank)
            .u16(self.pp);
        encode_packet(server::USER_STATS, &writer.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_fixture_header_and_unknown_ids_are_preserved() {
        let fixture = [5, 0, 0, 4, 0, 0, 0, 42, 0, 0, 0];
        assert_eq!(int_packet(server::USER_ID, 42).unwrap(), fixture);
        let mut batch = fixture.to_vec();
        batch.extend(encode_packet(65000, &[1, 2]).unwrap());
        let decoded = decode_packets(&batch).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[1].id, 65000);
        assert_eq!(decoded[1].payload, &[1, 2]);
    }
    #[test]
    fn every_truncated_frame_is_rejected() {
        let fixture = int_packet(server::USER_ID, 42).unwrap();
        for length in 1..fixture.len() {
            assert!(decode_packets(&fixture[..length]).is_err());
        }
        let mut oversized = [0; 7];
        oversized[3..].copy_from_slice(&((MAX_PACKET_BYTES + 1) as u32).to_le_bytes());
        assert_eq!(
            decode_packets(&oversized),
            Err(ProtocolError::Limit("packet size"))
        );
    }
    #[test]
    fn strings_use_utf8_byte_length_and_reject_malformed_lengths() {
        let value = "伊莉雅".repeat(70);
        let mut writer = Writer::new();
        writer.string(&value).unwrap().string("").unwrap();
        let bytes = writer.into_bytes();
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.string().unwrap(), value);
        assert_eq!(reader.string().unwrap(), "");
        reader.finish().unwrap();
        for bad in [
            &[12][..],
            &[11, 128][..],
            &[11, 255, 255, 255, 255, 16][..],
            &[11, 1, 255][..],
        ] {
            assert!(Reader::new(bad).string().is_err());
        }
    }
    #[test]
    fn invalid_second_packet_does_not_yield_first_packet() {
        let mut batch = encode_packet(client::PING, &[]).unwrap();
        batch.push(0);
        assert_eq!(decode_packets(&batch), Err(ProtocolError::Truncated));
    }
    #[test]
    fn login_validates_shape_and_redacts_secrets() {
        let login = LoginRequest::parse(
            b"Illya\n0123456789abcdef0123456789abcdef\nb20261007|8|0|hashes:private:|1\n",
        )
        .unwrap();
        assert_eq!(login.utc_offset, 8);
        assert!(login.block_non_friend_dms);
        assert!(!format!("{login:?}").contains("01234567"));
        assert!(!format!("{login:?}").contains("private"));
        assert!(LoginRequest::parse(b"Illya\npassword\nb20261007|8|0|hash|1").is_err());
        assert!(
            LoginRequest::parse(b"Illya\n0123456789abcdef0123456789abcdef\nb20261007|127|0|hash|1")
                .is_err()
        );
    }
    #[test]
    fn chat_fixture_has_strict_boundaries() {
        let message = ChatMessage {
            sender: "Illya".into(),
            text: "你好！".into(),
            recipient: "#lobby".into(),
            sender_id: 7,
        };
        let payload = message.encode().unwrap();
        assert_eq!(ChatMessage::decode(&payload).unwrap(), message);
        let mut extra = payload;
        extra.push(0);
        assert_eq!(
            ChatMessage::decode(&extra),
            Err(ProtocolError::TrailingData)
        );
    }
    #[test]
    fn presence_and_stats_reject_non_finite_numbers() {
        let stats = UserStats {
            accuracy: f32::NAN,
            ..UserStats::default()
        };
        assert!(stats.packet().is_err());
        let presence = Presence {
            user_id: 1,
            username: "Illya".into(),
            utc_offset: 8,
            country_code: 0,
            client_privileges: 1,
            mode: 0,
            longitude: f32::INFINITY,
            latitude: 0.0,
            rank: 0,
        };
        assert!(presence.packet().is_err());
    }
}
