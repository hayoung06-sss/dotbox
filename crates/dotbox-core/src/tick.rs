//! 고정 틱 스케줄러.

use crate::{Error, Result};

/// 기본 틱 간격.
pub const TICK_MS: u32 = 16;
/// 한 루프에서 따라잡을 수 있는 최대 틱 수.
pub const MAX_TICKS_PER_LOOP: u32 = 3;

/// 경과 시간을 고정 틱 수로 바꾼다.
///
/// 따라잡기 한도를 넘은 밀린 시간은 버리고 [`dropped_ms`](Self::dropped_ms)에 누적한다.
/// 이렇게 해서 루프가 오래 멈춘 뒤 게임이 수십 틱을 한꺼번에 진행하지 않게 한다.
#[derive(Debug, Clone)]
pub struct FixedTicker {
    tick_ms: u32,
    max_ticks_per_loop: u32,
    last_ms: u32,
    pending_ms: u32,
    tick_count: u64,
    dropped_ms: u64,
}

impl FixedTicker {
    /// 기본값(`TICK_MS`, `MAX_TICKS_PER_LOOP`)으로 만든다.
    pub fn new(now_ms: u32) -> Self {
        Self::with_config(TICK_MS, MAX_TICKS_PER_LOOP, now_ms)
            .expect("default tick config is valid")
    }

    /// `tick_ms`와 `max_ticks_per_loop`는 0보다 커야 한다.
    pub fn with_config(tick_ms: u32, max_ticks_per_loop: u32, now_ms: u32) -> Result<Self> {
        if tick_ms == 0 || max_ticks_per_loop == 0 {
            return Err(Error::InvalidArgument);
        }
        Ok(Self {
            tick_ms,
            max_ticks_per_loop,
            last_ms: now_ms,
            pending_ms: 0,
            tick_count: 0,
            dropped_ms: 0,
        })
    }

    /// 현재 시각까지 실행할 틱 수를 돌려준다(0..=`max_ticks_per_loop`).
    pub fn advance(&mut self, now_ms: u32) -> u32 {
        let elapsed_ms = now_ms.wrapping_sub(self.last_ms);
        self.last_ms = now_ms;
        self.pending_ms = self.pending_ms.saturating_add(elapsed_ms);

        let due = self.pending_ms / self.tick_ms;
        let ticks = due.min(self.max_ticks_per_loop);
        self.pending_ms -= ticks * self.tick_ms;

        if due > ticks {
            // 한도를 넘은 틱은 버리고 틱 미만 나머지만 남긴다.
            let remainder_ms = self.pending_ms % self.tick_ms;
            self.dropped_ms += u64::from(self.pending_ms - remainder_ms);
            self.pending_ms = remainder_ms;
        }

        self.tick_count += u64::from(ticks);
        ticks
    }

    /// 씬 전환 등에서 기준 시각을 다시 잡는다. 밀린 시간은 버린다(통계에 넣지 않는다).
    pub fn reset(&mut self, now_ms: u32) {
        self.last_ms = now_ms;
        self.pending_ms = 0;
    }

    pub fn tick_ms(&self) -> u32 {
        self.tick_ms
    }

    /// 지금까지 실행한 틱 수.
    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    /// 따라잡기 한도를 넘어 버린 시간의 합.
    pub fn dropped_ms(&self) -> u64 {
        self.dropped_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticker(now_ms: u32) -> FixedTicker {
        FixedTicker::with_config(20, 3, now_ms).unwrap()
    }

    #[test]
    fn rejects_zero_config() {
        assert_eq!(
            FixedTicker::with_config(0, 3, 0).unwrap_err(),
            Error::InvalidArgument
        );
        assert_eq!(
            FixedTicker::with_config(20, 0, 0).unwrap_err(),
            Error::InvalidArgument
        );
    }

    #[test]
    fn accumulates_partial_ticks() {
        let mut t = ticker(0);
        assert_eq!(t.advance(10), 0);
        assert_eq!(t.advance(25), 1);
        assert_eq!(t.advance(39), 0);
        assert_eq!(t.advance(40), 1);
        assert_eq!(t.tick_count(), 2);
    }

    #[test]
    fn caps_catch_up_and_drops_backlog() {
        let mut t = ticker(0);
        // 1005ms 지연: 50틱이 밀렸지만 3틱만 실행한다.
        assert_eq!(t.advance(1005), 3);
        assert_eq!(t.dropped_ms(), 1000 - 60);
        // 남은 5ms는 유지된다.
        assert_eq!(t.advance(1020), 1);
        assert_eq!(t.advance(1024), 0);
    }

    #[test]
    fn handles_clock_wraparound() {
        let start = u32::MAX - 15;
        let mut t = ticker(start);
        // 시계가 0을 지나 4가 되면 20ms가 흐른 것이다.
        assert_eq!(t.advance(4), 1);
    }

    #[test]
    fn reset_discards_pending_time() {
        let mut t = ticker(0);
        assert_eq!(t.advance(15), 0);
        t.reset(100);
        assert_eq!(t.advance(110), 0);
        assert_eq!(t.advance(120), 1);
        assert_eq!(t.dropped_ms(), 0);
    }
}
