//! Independently implemented stable multiplayer payloads. Slot/user identity is
//! validated here and must still be bound to server-owned membership by callers.
use crate::{
    ProtocolError, Result,
    stable::{Reader, Writer, is_md5},
};
use serde::{Deserialize, Serialize};

pub const SLOTS: usize = 16;
pub const OPEN: u8 = 1;
pub const LOCKED: u8 = 2;
pub const NOT_READY: u8 = 4;
pub const READY: u8 = 8;
pub const NO_MAP: u8 = 16;
pub const PLAYING: u8 = 32;
pub const COMPLETE: u8 = 64;
pub fn occupied(status: u8) -> bool {
    status & 124 != 0
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Match {
    pub id: u16,
    pub in_progress: bool,
    pub mods: u32,
    pub name: String,
    // Gateway replaces this with a nonsecret lock marker before persisting state.
    pub password: String,
    pub map_name: String,
    pub map_id: i32,
    pub map_md5: String,
    pub statuses: [u8; SLOTS],
    pub teams: [u8; SLOTS],
    pub users: [Option<i32>; SLOTS],
    pub host_id: i32,
    pub mode: u8,
    pub win_condition: u8,
    pub team_type: u8,
    pub freemods: bool,
    pub slot_mods: [u32; SLOTS],
    pub seed: i32,
}
impl Match {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let id = r.u16()?;
        let in_progress = r.boolean()?;
        let _powerplay = r.u8()?;
        let mods = r.u32()?;
        let name = r.string()?.to_owned();
        let password = r.string()?.to_owned();
        let map_name = r.string()?.to_owned();
        let map_id = r.i32()?;
        let map_md5 = r.string()?.to_owned();
        let mut statuses = [OPEN; SLOTS];
        let mut teams = [0; SLOTS];
        for value in &mut statuses {
            *value = r.u8()?;
        }
        for value in &mut teams {
            *value = r.u8()?;
        }
        let mut users = [None; SLOTS];
        for (i, value) in users.iter_mut().enumerate() {
            if occupied(statuses[i]) {
                *value = Some(r.i32()?);
            }
        }
        let host_id = r.i32()?;
        let mode = r.u8()?;
        let win_condition = r.u8()?;
        let team_type = r.u8()?;
        let freemods = r.boolean()?;
        let mut slot_mods = [0; SLOTS];
        if freemods {
            for value in &mut slot_mods {
                *value = r.u32()?;
            }
        }
        let seed = r.i32()?;
        r.finish()?;
        let value = Self {
            id,
            in_progress,
            mods,
            name,
            password,
            map_name,
            map_id,
            map_md5,
            statuses,
            teams,
            users,
            host_id,
            mode,
            win_condition,
            team_type,
            freemods,
            slot_mods,
            seed,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty()
            || self.name.len() > 256
            || self.name.chars().any(char::is_control)
            || self.password.len() > 128
            || self.password.chars().any(char::is_control)
            || self.map_name.len() > 512
            || self.map_name.chars().any(char::is_control)
        {
            return Err(ProtocolError::Invalid("match text"));
        }
        if self.mode > 3
            || self.win_condition > 3
            || self.team_type > 3
            || self.mods > i32::MAX as u32
            || !self.map_md5.is_empty() && !is_md5(&self.map_md5)
        {
            return Err(ProtocolError::Invalid("match settings"));
        }
        let mut seen = std::collections::HashSet::new();
        for i in 0..SLOTS {
            if !matches!(
                self.statuses[i],
                0 | OPEN | LOCKED | NOT_READY | READY | NO_MAP | PLAYING | COMPLETE | 128
            ) || self.teams[i] > 2
                || self.slot_mods[i] > i32::MAX as u32
            {
                return Err(ProtocolError::Invalid("match slot"));
            }
            if occupied(self.statuses[i]) != self.users[i].is_some() {
                return Err(ProtocolError::Invalid("match slot identity"));
            }
            match self.users[i] {
                Some(id) if id <= 0 || !seen.insert(id) => {
                    return Err(ProtocolError::Invalid("duplicate match user"));
                }
                _ => {}
            }
        }
        Ok(())
    }
    /// Locked matches use marker 0x0b,0x00 without exposing the room password.
    pub fn encode(&self, hide_password: bool) -> Result<Vec<u8>> {
        self.validate()?;
        let mut w = Writer::new();
        w.u16(self.id)
            .boolean(self.in_progress)
            .u8(0)
            .u32(self.mods)
            .string(&self.name)?;
        if hide_password && !self.password.is_empty() {
            w.u8(11).u8(0);
        } else {
            w.string(&self.password)?;
        }
        w.string(&self.map_name)?
            .i32(self.map_id)
            .string(&self.map_md5)?;
        for value in self.statuses {
            w.u8(value);
        }
        for value in self.teams {
            w.u8(value);
        }
        for value in self.users.into_iter().flatten() {
            w.i32(value);
        }
        w.i32(self.host_id)
            .u8(self.mode)
            .u8(self.win_condition)
            .u8(self.team_type)
            .boolean(self.freemods);
        if self.freemods {
            for value in self.slot_mods {
                w.u32(value);
            }
        }
        w.i32(self.seed);
        Ok(w.into_bytes())
    }
}

pub fn validate_score_frame(bytes: &[u8]) -> Result<()> {
    let mut r = Reader::new(bytes);
    r.i32()?;
    r.u8()?;
    for _ in 0..6 {
        r.u16()?;
    }
    if r.i32()? < 0 {
        return Err(ProtocolError::Invalid("negative score frame"));
    }
    r.u16()?;
    r.u16()?;
    r.boolean()?;
    r.u8()?;
    r.u8()?;
    if r.boolean()? && (!r.f64()?.is_finite() || !r.f64()?.is_finite()) {
        return Err(ProtocolError::Invalid("nonfinite score frame"));
    }
    r.finish()
}
pub fn validate_spectator_bundle(bytes: &[u8]) -> Result<()> {
    let mut r = Reader::new(bytes);
    r.i32()?;
    let count = r.u16()?;
    if count > 4096 {
        return Err(ProtocolError::Limit("spectator frames"));
    }
    for _ in 0..count {
        r.u8()?;
        r.u8()?;
        if !r.f32()?.is_finite() || !r.f32()?.is_finite() {
            return Err(ProtocolError::Invalid("spectator coordinates"));
        }
        r.i32()?;
    }
    if r.u8()? > 8 {
        return Err(ProtocolError::Invalid("spectator action"));
    }
    let frame = r.take(29)?;
    if frame[28] == 1 {
        let mut full = frame.to_vec();
        full.extend_from_slice(r.take(16)?);
        validate_score_frame(&full)?;
    } else {
        validate_score_frame(frame)?;
    }
    r.u16()?;
    r.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Match {
        let mut m = Match {
            id: 7,
            in_progress: false,
            mods: 0,
            name: "Fixture room".into(),
            password: "secret".into(),
            map_name: "Original fixture".into(),
            map_id: 1,
            map_md5: "a".repeat(32),
            statuses: [OPEN; 16],
            teams: [0; 16],
            users: [None; 16],
            host_id: 1,
            mode: 0,
            win_condition: 0,
            team_type: 0,
            freemods: true,
            slot_mods: [0; 16],
            seed: 1,
        };
        m.statuses[0] = READY;
        m.users[0] = Some(1);
        m
    }
    #[test]
    fn match_roundtrip_and_password_hidden() {
        let m = fixture();
        let encoded = m.encode(false).unwrap();
        assert_eq!(Match::decode(&encoded).unwrap().password, "secret");
        let hidden = m.encode(true).unwrap();
        assert!(!hidden.windows(6).any(|w| w == b"secret"));
        assert_eq!(Match::decode(&hidden).unwrap().users, m.users);
        for n in 0..encoded.len() {
            assert!(Match::decode(&encoded[..n]).is_err());
        }
    }
    #[test]
    fn forged_slot_identity_rejected() {
        let mut m = fixture();
        m.users[1] = Some(1);
        m.statuses[1] = READY;
        assert!(m.encode(false).is_err());
        m.users[1] = None;
        assert!(m.encode(false).is_err());
    }
    #[test]
    fn bounded_score_and_spectator_frames() {
        let score = [0u8; 29];
        validate_score_frame(&score).unwrap();
        let mut bundle = Vec::new();
        bundle.extend_from_slice(&0i32.to_le_bytes());
        bundle.extend_from_slice(&0u16.to_le_bytes());
        bundle.push(0);
        bundle.extend_from_slice(&score);
        bundle.extend_from_slice(&1u16.to_le_bytes());
        validate_spectator_bundle(&bundle).unwrap();
        bundle[4..6].copy_from_slice(&4097u16.to_le_bytes());
        assert!(validate_spectator_bundle(&bundle).is_err());
    }
}
