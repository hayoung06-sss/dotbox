# 변경 로그

이 프로젝트의 주요 변경 사항을 기록합니다.

형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르고,
버전은 [유의적 버전(SemVer)](https://semver.org/lang/ko/)을 따릅니다.

## 작성 규칙

- 새 변경은 항상 `[Unreleased]` 아래에 적습니다.
- 변경은 다음 분류 중 하나에 넣습니다. 내용이 없는 분류는 적지 않습니다.
  - `Added`: 새 기능
  - `Changed`: 기존 기능의 변경
  - `Deprecated`: 곧 제거될 기능
  - `Removed`: 제거된 기능
  - `Fixed`: 버그 수정
  - `Security`: 보안 관련 수정
- 게임 개발자(엔진 사용자)에게 영향을 주는 변경을 중심으로 적습니다. 공개 API가 깨지는 변경에는 **호환성 깨짐**이라고 표시합니다.
- 버전을 낼 때는 `[Unreleased]`의 내용을 `## [x.y.z] - YYYY-MM-DD` 절로 옮기고, 비어 있는 `[Unreleased]`를 새로 남깁니다.

## [Unreleased]

### Added

- Cargo 워크스페이스 구성: `crates/*`, `examples/*`
- `dotbox-core`: 엔진 본체 crate와 `Engine` 골격
- `dotbox-sim`: PC 실행기 라이브러리와 `run(engine)` 골격
- `examples/hello`: 엔진 동작 확인용 최소 예제
- README에 사용 방법 섹션 추가
- `dotbox-core`: 공통 오류 `Error`와 `Result`
- `dotbox-core::hal`: 하드웨어 트레잇 `Clock`, `Display`, `InputSource`, `Buzzer`, `Link`, `Storage`와 `Rgb`, `RawInput`
- `dotbox-core::display`: 고정 크기 `FrameBuffer`와 화면 레이아웃. 합체·분리와 관계없이 좌표계를 유지한다
  - `ScreenLayout::Wide`(기본): 32×16 화면을 두 패널에 나눠 표시
  - `ScreenLayout::Mirrored`: 16×16 화면을 두 패널에 똑같이 표시
  - `copy_panel(side, out)`으로 각 모듈이 자기 패널의 16×16 픽셀을 꺼낸다
- `dotbox-core::tick`: 고정 틱 스케줄러 `FixedTicker`(`TICK_MS` 20, 루프당 최대 3틱, 밀린 시간 폐기, 시계 순환 처리)
