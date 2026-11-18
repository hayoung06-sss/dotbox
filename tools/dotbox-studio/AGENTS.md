# DotBox Studio

DotBox 게임 개발자를 위한 **데스크톱 개발 도구(GUI)**. 스프라이트·글꼴·효과음 같은 에셋을 만들고 미리 보며,
나중에는 기기 빌드·플래시와 시리얼 로그까지 다룬다. 이 디렉토리는 엔진 담당과 다른 팀원이 맡는다.

저장소 루트의 `CLAUDE.md`(엔진 전체 규칙)도 함께 따른다. 이 파일은 Studio 작업에 필요한 내용을 추린 것이며,
루트와 충돌하면 루트가 우선한다. 루트 CLAUDE.md를 자동으로 읽지 않는 AI 도구라면 먼저 그 파일을 읽는다.

## DotBox가 뭔가

- ESP32 기반 휴대용 도트매트릭스 게임기. 같은 모듈 2개(왼쪽/오른쪽)가 붙어 있다.
- 모듈마다 WS2812B **16×16 RGB** 매트릭스가 하나씩 있다. 두 개를 이어 **32×16** 화면이 된다.
- 입력은 모듈마다 조이스틱(X/Y 아날로그 + 버튼) 하나. 소리는 왼쪽 모듈의 **수동 부저**(PWM 단음, 화음 불가).
- 전원 때문에 매트릭스 밝기는 **15~25%로 제한**한다. PC 화면에서 보는 색과 실제 LED 색은 꽤 다르다.
- 엔진은 Rust로 작성했다(`crates/dotbox-core`). 게임은 외부 저장소에서 엔진을 의존성으로 가져다 쓴다.

## Studio의 위치

```
tools/
  dotbox-tools/    # (예정) 라이브러리: 에셋 변환, 프로젝트 생성, doctor, 빌드·플래시
  dotbox-cli/      # (예정) 바이너리 `dotbox`: dotbox-tools를 감싸는 CLI
  dotbox-studio/   # ← 여기. egui 앱. 최종적으로 `dotbox studio`로 실행한다
```

### 반드시 지킬 설계 원칙

1. **CLI를 먼저, GUI는 그 위에.** Studio에서 하는 일은 모두 같은 일을 하는 `dotbox` CLI 명령이 있어야 한다.
   변환·검증·생성 로직은 `dotbox-tools`에 두고, Studio는 그 함수를 **직접 호출**한다. CLI 출력을 파싱하지 않는다.
   Studio 안에는 UI 상태와 화면 코드만 둔다.
2. **미리보기는 엔진 코드로 그린다.** `dotbox-core`를 의존성으로 링크하고 `FrameBuffer`, `Rgb`, `ScreenLayout`,
   (생기면) 글꼴 합성 코드를 그대로 써서 그린다. 미리보기용 렌더러를 따로 만들지 않는다.
   그래야 명세의 "변환 도구 미리보기와 기기 출력 비트맵이 일치"라는 기준을 구조적으로 지킬 수 있다.
3. **GUI는 원본 에셋만 편집한다.** 원본(예: PNG, 글꼴 설정, 멜로디 파일)을 게임 저장소의 `assets/`에 저장하고,
   Rust `const` 데이터는 게임의 `build.rs`가 `dotbox-tools`로 생성한다. Studio가 생성 코드를 직접 쓰지 않는다.
   엔진은 매 틱 경로에서 힙 할당을 하지 않으므로 런타임 에셋은 정적 데이터여야 한다.
4. **결정적 출력.** 같은 입력과 같은 도구 버전이면 같은 결과가 나와야 한다(명세 요구). 해시맵 순회 순서,
   시각, 부동소수 반올림처럼 결과를 흔드는 요소를 변환 결과에 넣지 않는다.
5. **CLI 명세를 따른다.** 명령 목록, 게임 프로젝트 구조(`dotbox.toml`, `assets/`), `doctor` 검사 항목은
   저장소 루트의 `docs/cli.md`에 있다. Studio 기능은 이 명령들과 1:1로 대응해야 하고, Studio에서 새 기능이
   필요하면 먼저 이 문서에 CLI 명령으로 제안한다.
6. **Studio는 게임을 실행하지 않는다.** 게임 실행은 각 게임이 `dotbox_sim::run`으로 띄우는 시뮬레이터 창이 맡는다.

## 기술 선택

- GUI: **egui / eframe**. 순수 Rust라 `dotbox-core` 타입을 그대로 쓸 수 있다.
  버전은 작업 시작 시점의 최신 안정판을 골라 `Cargo.toml`에 명시한다.
- Rust edition 2024. 플랫폼은 우선 Windows. macOS/Linux에서 깨지는 코드는 피한다.
- 무거운 의존성(egui 등)은 이 crate에만 둔다. `dotbox-tools`는 게임 `build.rs`도 쓰므로 GUI 의존성을 넣지 않는다.

## 기능과 우선순위

| 순위 | 기능 | 메모 |
|---|---|---|
| 1 | **스프라이트·화면 편집기** | 16×16과 32×16 캔버스, 팔레트, 패널 경계(x=16) 표시, 밝기 15~25%를 흉내 낸 실기기 미리보기 |
| 2 | **글꼴 변환과 미리보기** | 엔진 명세 11장. TTF/OTF를 픽셀 크기·문자 집합·이진화 임계값으로 1비트 비트맵으로 만든다. 한글은 필요한 완성형 음절만 넣는다 |
| 3 | **효과음·멜로디 편집기** | 단음 부저 기준(주파수 Hz + 길이 ms 시퀀스). PC 스피커로 재생한다 |
| 4 | **기기 패널** | `dotbox doctor` 결과, 왼쪽/오른쪽 모듈 빌드·플래시, 모듈 2개의 시리얼 로그 동시 보기 |

처음에는 1번부터 시작한다. 1번은 `dotbox-core`의 `FrameBuffer`만 있으면 되고, `dotbox-tools`가 없어도 시작할 수 있다.

## 지금 쓸 수 있는 엔진 API (`dotbox-core`)

엔진은 골격 단계라 **API가 바뀔 수 있다.** 쓰기 전에 `crates/dotbox-core/src/`의 실제 코드를 확인한다.

- `dotbox_core::hal::Rgb { r, g, b }`: 감마 보정 전 색. `Rgb::BLACK`, `Rgb::WHITE`, `Rgb::new(r, g, b)`.
- `dotbox_core::display`
  - 상수 `PANEL_WIDTH`/`PANEL_HEIGHT`(16), `PANEL_PIXELS`(256), `SCREEN_WIDTH`(32), `SCREEN_HEIGHT`(16).
  - `ScreenLayout::Wide`(32×16, 기본)와 `ScreenLayout::Mirrored`(16×16 화면을 두 패널에 똑같이 표시).
  - `FrameBuffer`: `new(layout)`, `set_pixel(x, y, color)`(화면 밖 무시), `pixel(x, y)`, `fill`, `clear`,
    `copy_panel(Side::Left | Side::Right, &mut [Rgb; 256])`.
- 아직 없는 것: 감마·밝기 처리, 글꼴, 스프라이트 타입, 사운드 데이터 형식, 에셋 변환.
- `dotbox-tools`와 `dotbox-cli`는 1단계(`new`, `doctor`, `run`, `build`, `flash`, `monitor`, `studio`)가 구현되어 있다.
  Studio의 기기 패널은 `dotbox_tools::doctor::run`, `device::flash`, `device::detect_ports` 등을 직접 부른다.
  `dotbox studio`는 PATH에서 `dotbox-studio`를 찾아 프로젝트 디렉토리를 첫 인자로 넘겨 실행한다.

## 엔진 담당과 맞출 것

- **에셋 원본 형식과 생성 데이터 형식**(스프라이트, 글꼴, 멜로디)은 엔진 런타임과 맞물리므로 혼자 확정하지 않는다.
  제안서(간단한 Markdown이나 PR 설명)로 합의한 뒤 구현한다.
- `crates/`(엔진) 코드는 엔진 담당 영역이다. Studio에 필요한 엔진 변경은 직접 크게 고치지 말고 요청하거나 작은 PR로 낸다.
- 변환 로직이 필요한데 `dotbox-tools`가 아직 없으면 엔진 담당과 상의해 `tools/dotbox-tools`를 만든다.
  임시로 Studio 안에 둘 때는 GUI 의존성 없는 별도 모듈로 분리해 나중에 그대로 옮길 수 있게 한다.

## 코드 규칙 (루트 CLAUDE.md 요약)

- 네이밍은 Rust 표준 컨벤션: 타입 `UpperCamelCase`(약어도 한 단어: `Rgb`, `Png`), 함수·변수·모듈 `snake_case`,
  상수 `SCREEN_WIDTH`. getter에 `get_` 금지. 단위가 있는 값은 이름에 단위(`freq_hz`, `duration_ms`).
- 실패할 수 있는 도구 로직은 `Result`를 돌려준다. GUI에서는 오류를 사용자에게 보여 주고 패닉하지 않는다.
- 문서 주석과 커밋 메시지는 한국어. 커밋 제목은 `dotbox-studio: 무엇을 했는지` 형식(기존 로그 참고).
- UI와 로직을 나눠서 로직(편집 명령, 실행 취소, 변환)은 GUI 없이 단위 테스트한다.
- 엔진·도구 사용자에게 보이는 변경은 루트 `CHANGELOG.md`의 `[Unreleased]`에 한 줄을 추가한다.

## 시작하기

1. 이 디렉토리에 `Cargo.toml`(패키지 이름 `dotbox-studio`, `version.workspace = true`, `edition.workspace = true`,
   `publish = false`)과 `src/main.rs`를 만든다. 의존성: `dotbox-core.workspace = true`, `eframe`.
2. 루트 `Cargo.toml`의 workspace `members`에 `"tools/dotbox-studio"`를 추가한다.
   `tools/*` glob은 쓰지 않는다. Cargo.toml이 없는 디렉토리가 섞여 있으면 빌드가 실패한다.
3. 명령:

```sh
cargo run -p dotbox-studio     # Studio 실행
cargo test -p dotbox-studio    # Studio 테스트
cargo test                     # 저장소 전체 테스트 (엔진을 깨지 않았는지 확인)
```

## 참고

- 엔진 기획/스펙: https://docs.google.com/document/d/1Eamj175jZgOSw444PLO44mb_cv7SEvi0G-LNUIOSUYM/edit
  (`curl -sL .../export?format=txt`로 받으면 모든 탭이 합쳐져 나온다). 1번째 탭은 무시한다.
  "DotBox Engine 명세 v0.1"의 11장(폰트 래스터화)이 글꼴 도구의 요구 사항이다. 이 명세에 있는 C++ 관련 내용과
  MAX7219·16×8 단색 같은 낡은 하드웨어 전제는 따르지 않는다. 하드웨어는 3번째 탭 "DotBox 게임기 스펙"이 기준이다.
