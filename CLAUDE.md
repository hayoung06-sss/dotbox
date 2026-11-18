# DotBox Engine

ESP32 기반 도트매트릭스 휴대용 게임기 **DotBox**용 통합 게임 엔진. 소스 코드로만 배포한다.
2026-2 컴퓨터구조론 5조 프로젝트.

## 기준 문서

- 기획/스펙: https://docs.google.com/document/d/1Eamj175jZgOSw444PLO44mb_cv7SEvi0G-LNUIOSUYM/edit
  (`curl -sL .../export?format=txt`로 받으면 모든 탭이 합쳐져 나온다)
  - 1번째 탭(초기 기획, AI 계획서): 무시.
  - 3번째 탭 "DotBox 게임기 스펙": **하드웨어의 기준**.
  - "DotBox Engine 명세 v0.1": MAX7219, 16×8 단색, DS1302, HC-06 같은 낡은 하드웨어 전제가 남아 있다.
    엔진 구조와 동작 규칙(계층 구조, 고정 틱, 씬 수명, 입력 이벤트, 메시지 형식)만 참고한다.
    **C++ 전용 내용과 네이밍 컨벤션은 따르지 않는다.**

## 결정 사항

- 언어: **Rust** (edition 2024). 문서의 C++/ESP-IDF 지정은 폐기했다.
- 펌웨어: **std** 환경. `esp-idf-svc`, `esp-idf-hal`, `esp-idf-sys`를 쓴다.
- 네이밍: **Rust 표준 컨벤션**을 따른다(RFC 430, Rust API Guidelines).
  - 타입·트레잇·enum 변형: `UpperCamelCase`. 약어도 한 단어로 취급한다(`Rgb`, `EspNow`, `Wifi`).
  - 함수·메서드·변수·모듈·파일: `snake_case`. 상수와 static: `SCREEN_WIDTH`.
  - getter에는 `get_`를 붙이지 않는다(`width()`). 변환 함수는 `as_`/`to_`/`into_`, 생성자는 `new`/`with_*`를 쓴다.
  - private 필드에 `_` 접두사를 붙이지 않는다(`_`는 미사용 표시 전용이다).
  - 단위가 있는 값은 이름에 단위를 넣거나(`tick_ms`, `freq_hz`, `battery_mv`) `Duration` 같은 타입을 쓴다.
- 오류: 실패할 수 있는 작업은 `Result<T, dotbox_core::Error>`를 돌려준다. 문서의 `Status` 개념을 대체한다.

## 하드웨어 (3번째 탭 기준)

같은 구성의 모듈 2개(왼쪽/오른쪽). 아래는 한 모듈 기준이다.

| 부품 | 핀 | 비고 |
|---|---|---|
| ESP32 (Xtensa, 원본 ESP32) | — | 게임 연산, ESP-NOW |
| WS2812B 16×16 RGB | DIN GPIO13 | 5V. 데이터선에 330~470Ω. 지그재그 배선 여부는 실물로 확인 |
| 조이스틱 X/Y | GPIO32 / GPIO33 (ADC1) | VCC는 반드시 3.3V |
| 조이스틱 버튼 | GPIO25 | 내부 풀업, active-low |
| 수동 부저 | GPIO14 | LEDC PWM. 왼쪽 모듈에는 필수 |
| 결합 감지 홀센서 | GPIO26 | 선택 사항 |
| 전원 | 5V 2A 이상 (18650 두 개) | 매트릭스 밝기는 15~25%로 제한 |

- 논리 좌표계는 합체·분리와 관계없이 고정이다. 게임은 분리 상태를 신경 쓰지 않는다(`dotbox_core::display`).
  - `ScreenLayout::Wide`(기본): 32×16 화면. x 0~15는 왼쪽 패널, 16~31은 오른쪽 패널.
  - `ScreenLayout::Mirrored`: 16×16 화면 하나를 두 패널에 똑같이 보여 주는 편의 기능.
- 왼쪽은 마스터로 게임 로직, 왼쪽 화면, 사운드를 맡는다. 오른쪽은 슬레이브로 입력 전송과 오른쪽 화면 렌더를 맡는다.
- 모듈 간 통신은 ESP-NOW로 한다. 픽셀이 아닌 게임 상태(수십 바이트)만 보내고, 각 보드가 각자 렌더한다.
- ADC2는 Wi-Fi(ESP-NOW 포함)와 동시에 쓸 수 없다. 아날로그 입력은 ADC1 핀(GPIO32~39)만 쓴다.

## 구조

이 저장소는 **엔진 라이브러리만** 담는다. 게임은 외부 crate/저장소에서 엔진을 의존성으로 가져다 쓰고,
게임 쪽이 `main`을 가진 채 플랫폼 실행기(`dotbox_sim::run`, `dotbox_esp::run`)를 호출한다.

```
Cargo.toml              # 호스트용 가상 workspace (members = crates/*, examples/*; dotbox-esp 제외)
crates/
  dotbox-core/          # 하드웨어 독립 엔진. std만 쓰고 ESP 의존성은 없다. PC에서 테스트한다
  dotbox-sim/           # 라이브러리. PC 실행기: 창에 32×16 화면과 키보드 입력
examples/               # 엔진 검증용 예제. 예제마다 별도 crate(workspace 멤버)
  hello/
platform/               # (예정)
  dotbox-esp/           # 라이브러리. ESP32 실행기. 별도 툴체인(esp)과 target이라 workspace에서 뺀다
```

- 외부 게임의 펌웨어 바이너리는 게임 쪽에서 `rust-toolchain.toml`(channel = "esp"),
  `.cargo/config.toml`(target = xtensa-esp32-espidf, runner = espflash), `sdkconfig.defaults`를 갖춰 빌드한다.
- 핵심 원칙: 하드웨어 접근은 `dotbox-core`의 트레잇(`Display`, `InputSource`, `Buzzer`, `Link`, `Storage`, `Clock`)으로 추상화한다.
  `dotbox-esp`는 ESP 구현을, `dotbox-sim`과 테스트는 가짜 구현을 꽂는다.
- 매 틱 경로와 렌더 경로에서는 힙 할당을 하지 않는다. 버퍼는 미리 잡아 재사용한다.
- ISR과 무선 콜백에서는 데이터를 복사해 채널/큐로 넘기기만 한다. 상태 변경은 엔진 루프 스레드에서만 한다.

## 변경 로그

- 엔진 사용자에게 보이는 변경을 하면 `CHANGELOG.md`의 `[Unreleased]` 아래 해당 분류(Added/Changed/Removed/Fixed 등)에 한 줄을 추가한다.
- 형식은 Keep a Changelog, 버전은 SemVer를 따른다. 아직 버전을 낸 적이 없다. 버전 절은 릴리스할 때만 만든다.

## 명령

```sh
cargo test                     # 호스트 crates 테스트
cargo run -p hello             # 예제를 PC 시뮬레이터로 실행
```

## 주의

- **ESP-IDF 빌드는 경로에 한글, 공백, OneDrive가 있으면 실패하고, 경로가 길면 Windows 경로 길이 제한에 걸린다.**
  저장소는 `D:\RustProject\DotBoxEngine`(ASCII, OneDrive 아님)이지만 경로 길이 문제는 남는다.
  펌웨어를 빌드할 때는 `.cargo/config.toml`에 `CARGO_TARGET_DIR`/`ESP_IDF_TOOLS_INSTALL_DIR`를 짧은 경로(예: `C:\dbx`)로 지정한다.
- ESP 툴체인 설치: `cargo install espup ldproxy espflash` 후 `espup install`.
