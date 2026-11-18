//! 게임 상태 직렬화. 마스터가 매 틱 상태를 바이트로 보내고 슬레이브가 되살린다(`docs/game-api.md` 3.2).

use crate::{Error, Result};

/// 게임 상태 인코딩의 최대 크기. ESP-NOW 한 패킷(250바이트)에 헤더와 여유를 뺀 값이다.
pub const MAX_STATE_BYTES: usize = 200;

/// 모듈 사이로 보내는 게임 상태.
pub trait GameState: Sized {
    /// 인코딩한 최대 크기. [`MAX_STATE_BYTES`] 이하여야 한다.
    const MAX_BYTES: usize;

    fn encode(&self, out: &mut StateWriter<'_>) -> Result<()>;
    fn decode(input: &mut StateReader<'_>) -> Result<Self>;
}

/// 고정 버퍼에 쓰는 커서. 정수는 리틀 엔디언이다.
#[derive(Debug)]
pub struct StateWriter<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> StateWriter<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    /// 지금까지 쓴 바이트 수.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 쓴 부분.
    pub fn written(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    pub fn bytes(&mut self, data: &[u8]) -> Result<()> {
        let end = self
            .len
            .checked_add(data.len())
            .filter(|&end| end <= self.buf.len())
            .ok_or(Error::InvalidArgument)?;
        self.buf[self.len..end].copy_from_slice(data);
        self.len = end;
        Ok(())
    }

    pub fn u8(&mut self, value: u8) -> Result<()> {
        self.bytes(&[value])
    }

    pub fn i8(&mut self, value: i8) -> Result<()> {
        self.bytes(&value.to_le_bytes())
    }

    pub fn bool(&mut self, value: bool) -> Result<()> {
        self.u8(u8::from(value))
    }

    pub fn u16(&mut self, value: u16) -> Result<()> {
        self.bytes(&value.to_le_bytes())
    }

    pub fn i16(&mut self, value: i16) -> Result<()> {
        self.bytes(&value.to_le_bytes())
    }

    pub fn u32(&mut self, value: u32) -> Result<()> {
        self.bytes(&value.to_le_bytes())
    }
}

/// 바이트를 읽는 커서. 남은 바이트가 모자라면 [`Error::InvalidArgument`].
#[derive(Debug, Clone)]
pub struct StateReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> StateReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// 아직 읽지 않은 바이트 수.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|&end| end <= self.data.len())
            .ok_or(Error::InvalidArgument)?;
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut out = [0; N];
        out.copy_from_slice(self.bytes(N)?);
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    pub fn i8(&mut self) -> Result<i8> {
        Ok(i8::from_le_bytes(self.array()?))
    }

    /// 0과 1만 받는다.
    pub fn bool(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::InvalidArgument),
        }
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_all_types() {
        let mut buf = [0; 32];
        let mut w = StateWriter::new(&mut buf);
        w.u8(200).unwrap();
        w.i8(-5).unwrap();
        w.bool(true).unwrap();
        w.u16(0xBEEF).unwrap();
        w.i16(-1234).unwrap();
        w.u32(0xDEAD_BEEF).unwrap();
        w.bytes(&[1, 2, 3]).unwrap();
        assert_eq!(w.len(), 1 + 1 + 1 + 2 + 2 + 4 + 3);

        let len = w.len();
        let mut r = StateReader::new(&buf[..len]);
        assert_eq!(r.u8().unwrap(), 200);
        assert_eq!(r.i8().unwrap(), -5);
        assert!(r.bool().unwrap());
        assert_eq!(r.u16().unwrap(), 0xBEEF);
        assert_eq!(r.i16().unwrap(), -1234);
        assert_eq!(r.u32().unwrap(), 0xDEAD_BEEF);
        assert_eq!(r.bytes(3).unwrap(), &[1, 2, 3]);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn integers_are_little_endian() {
        let mut buf = [0; 4];
        let mut w = StateWriter::new(&mut buf);
        w.u32(0x0403_0201).unwrap();
        assert_eq!(buf, [1, 2, 3, 4]);
    }

    #[test]
    fn writer_rejects_overflow_without_partial_write() {
        let mut buf = [0; 3];
        let mut w = StateWriter::new(&mut buf);
        w.u16(1).unwrap();
        assert_eq!(w.u16(2), Err(Error::InvalidArgument));
        assert_eq!(w.len(), 2);
        w.u8(9).unwrap();
        assert_eq!(w.written(), &[1, 0, 9]);
    }

    #[test]
    fn reader_rejects_short_input() {
        let mut r = StateReader::new(&[1]);
        assert_eq!(r.u16(), Err(Error::InvalidArgument));
        assert_eq!(r.u8(), Ok(1));
        assert_eq!(r.u8(), Err(Error::InvalidArgument));
    }

    #[test]
    fn reader_rejects_invalid_bool() {
        assert_eq!(StateReader::new(&[2]).bool(), Err(Error::InvalidArgument));
    }
}
