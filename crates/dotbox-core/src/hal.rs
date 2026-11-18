//! 하드웨어 추상화 트레잇.
//!
//! 플랫폼 crate(`dotbox-sim`, `dotbox-esp`)와 테스트가 이 트레잇을 구현해 엔진에 꽂는다.

use crate::Result;

/// RGB 색. 감마 보정 전 값이다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Self = Self::new(0, 0, 0);
    pub const WHITE: Self = Self::new(255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// 단조 증가 시계. 값은 `u32` 밀리초이고 약 49.7일마다 0으로 돌아간다.
/// 경과 시간은 항상 `wrapping_sub`로 계산한다.
pub trait Clock {
    fn now_ms(&self) -> u32;
}

/// LED 매트릭스 출력 장치.
pub trait Display {
    fn width(&self) -> u16;
    fn height(&self) -> u16;

    /// 프레임 하나를 출력한다. `pixels`는 왼쪽 위부터 행 우선 순서이고 길이는 `width * height`다.
    fn write_frame(&mut self, pixels: &[Rgb]) -> Result<()>;

    /// 밝기를 0~100%로 설정한다. 0은 화면을 끈다.
    fn set_brightness_percent(&mut self, percent: u8) -> Result<()>;
}

/// 조이스틱 원시 값. 보정과 데드존은 엔진이 처리한다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RawInput {
    /// ADC 원시 값(ESP32 12비트 기준 0~4095).
    pub x: u16,
    pub y: u16,
    /// 디바운스 전 버튼 상태.
    pub button_down: bool,
}

/// 입력 장치.
pub trait InputSource {
    fn read(&mut self) -> Result<RawInput>;
}

/// 단일 음을 내는 부저.
pub trait Buzzer {
    fn play_tone(&mut self, freq_hz: u32) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
}

/// 모듈 간 통신(ESP-NOW 등). 메시지는 호출 시점에 복사한다.
pub trait Link {
    /// 송신 큐에 복사한다. `Ok`는 큐 수락을 뜻하고 전달 완료가 아니다.
    fn send(&mut self, data: &[u8]) -> Result<()>;

    /// 받은 메시지 하나를 `buf`에 복사하고 길이를 돌려준다. 없으면 `None`.
    fn try_recv(&mut self, buf: &mut [u8]) -> Result<Option<usize>>;
}

/// 영구 저장소(키-값).
pub trait Storage {
    /// 값을 `buf`에 복사하고 길이를 돌려준다. 키가 없으면 `None`.
    fn read(&mut self, key: &str, buf: &mut [u8]) -> Result<Option<usize>>;
    fn write(&mut self, key: &str, data: &[u8]) -> Result<()>;
}
