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
- `dotbox-core`: 엔진 본체 crate
- `dotbox-sim`: PC 실행기 `run(make_games)`. 마스터·슬레이브 엔진 두 개를 메모리 링크로 이어 한 창에 두 패널을 그린다.
  키보드 P1 WASD+Space, P2 방향키+Enter, F2 링크 끊기/잇기. `DOTBOX_SIM_HEADLESS_MS`로 창 없이 실행
- `examples/hello`: 두 플레이어가 각자 점을 움직이는 2인 예제
- README에 사용 방법 섹션 추가
- `dotbox-core`: 공통 오류 `Error`와 `Result`
- `dotbox-core::hal`: 하드웨어 트레잇 `Clock`, `Display`, `InputSource`, `Buzzer`, `Link`, `Storage`와 `Rgb`, `RawInput`
- `dotbox-core::display`: 고정 크기 `FrameBuffer`와 화면 레이아웃. 합체·분리와 관계없이 좌표계를 유지한다
  - `ScreenLayout::Wide`(기본): 32×16 화면을 두 패널에 나눠 표시
  - `ScreenLayout::Mirrored`: 16×16 화면을 두 패널에 똑같이 표시
  - `copy_panel(side, out)`으로 각 모듈이 자기 패널의 16×16 픽셀을 꺼낸다
- `dotbox-core::tick`: 고정 틱 스케줄러 `FixedTicker`(`TICK_MS` 16, 루프당 최대 3틱, 밀린 시간 폐기, 시계 순환 처리)
- `dotbox-core` 게임 API(`docs/game-api.md`)
  - `Game` 트레잇(`config`, `init`, `update`, `render`)과 `GameConfig`, `Context`(난수, 효과음, 레이아웃, `exit`)
  - `Games`: 펌웨어 하나에 여러 게임 등록. 게임이 하나면 메뉴 없이 바로 시작
  - `GameState`와 고정 버퍼 직렬화 `StateWriter`/`StateReader`(최대 200바이트)
  - 입력: `Input`/`PlayerInput`(4방향, 아날로그, 버튼 이벤트), 중립 보정·데드존·히스테리시스·디바운스
  - `Engine::step`: 마스터/슬레이브 루프, 핸드셰이크(빌드 일치 확인), `Input`/`State` 메시지, 연결 끊김 감지
  - 효과음 `Note`/`Sound`, 모듈 간 메시지 형식 `protocol`
  - `testing`: 화면 없이 게임을 돌리는 `Harness`, 메모리 링크 `loopback_pair`, 가짜 장치
- `dotbox-core::pairing`: 모듈 역할 레코드 `RoleRecord`(16바이트, CRC32). 펌웨어와 `dotbox flash`가 함께 쓴다
- `dotbox` CLI(`tools/dotbox-cli`)와 구현 라이브러리 `dotbox-tools`. 명세는 `docs/cli.md`
  - `dotbox new`: 게임 프로젝트(game, sim, firmware, assets, `dotbox.toml`) 생성. 게임 템플릿은 `Game` 구현과 테스트를 포함
  - `dotbox doctor`: Rust·ESP 툴체인, 경로(한글·공백·OneDrive·길이), 펌웨어 target-dir, 엔진 버전 점검. `--json` 지원
  - `dotbox run`: 시뮬레이터 실행
  - `dotbox build`, `dotbox flash --side left|right|both`, `dotbox monitor`: 펌웨어 빌드, 굽기와 역할 기록, 시리얼 모니터.
    `dotbox-esp`가 아직 없어 실기기 경로는 검증 전이다
  - `dotbox studio`: PATH의 `dotbox-studio` 실행
- DotBox Studio(`tools/dotbox-studio`): egui 기반 개발 도구 GUI
  - egui 기반 GUI 에디터 기본 뼈대 및 워크스페이스 패키지 추가
  - 32×16·16×16 도트 캔버스 편집기, 팔레트, 패널 경계선 및 실기기 밝기 미리보기 추가
  - TTF/OTF 1비트 글꼴 변환기 및 32×16 실기기 미리보기 탭 추가
  - 단음 부저(Hz/ms) 효과음·멜로디 편집기 및 PC 스피커 구형파 재생 탭 추가
  - 시리얼 포트 감지, `dotbox doctor`/`run` 실행, 좌·우·양쪽 모듈 플래시를 지원하는 기기 패널 탭 추가
- `dotbox-studio`: S1 프로젝트 작업 공간 구현
  - `dotbox-studio [DIR]` 실행 인자
  - 시작 화면(최근 프로젝트·폴더 열기·새 프로젝트 생성)
  - 사용자 설정(`studio.toml`)
  - 개요 탭(`dotbox.toml`·`doctor` 점검·포트/플래시 상태)
  - 프로젝트 설정 탭(`[paths]`·좌/우 포트 저장) 및 하단 출력·문제 패널 추가
- `dotbox-studio`: GUI 독립 편집 명령 객체(`CommandHistory`) 기반 실행 취소/다시 실행(`Ctrl+Z`/`Ctrl+Y`)을 스프라이트·효과음 편집기에 적용하고, 파일 단위 탭 바·미저장(`●`) 표시·닫기 확인 팝업 추가
- `dotbox-studio`: 1장 잔재 정리(실시간 CLI 스트리밍 및 [■ 중지], 기기 패널 경로 고정, 전역 밝기 분리)
- `dotbox-studio`: 에셋 탐색기 기반 스프라이트(.png)·효과음(.sound.toml)·글꼴(.font.toml)·아이콘(icon.png) 즉시 열기 및 저장(Ctrl+S) 지원
- `dotbox-studio`: 스프라이트 편집기 크기 동적 조절(1×1 ~ 32×16) 및 6종 그리기 도구(연필, 지우개, 채우기, 스포이드, 직선, 사각형) 추가
- `dotbox-studio`: 스프라이트 편집기에 애니메이션 프레임(추가·복제·삭제·순서 이동·프레임별 시간·재생 미리보기) 및 프레임 단위 Undo/Redo 지원 추가
- `dotbox-studio`: 글꼴 탭의 원본 TTF/OTF 경로 입력·불러오기 UI 복구, 임의 크기(1×1~32×16) 스프라이트 및 16×16 `icon.png` 불러오기 검사 분리, 앱 종료 시 실행 중 CLI 프로세스 자동 정리
- `dotbox-studio`: 효과음 부저 권장 주파수(31~4000Hz) 범위 검사 및 경고 표시 추가
- `dotbox-studio`: 글꼴 기준선(baseline) 정렬 미리보기 및 글자별 메트릭스·1비트 미니 비트맵 결과 표 추가
- `dotbox-studio`: 문제 패널에 `cargo --message-format=json` 컴파일 진단(에러/경고) 실시간 파싱 및 구조화 목록 표시 연동
- `dotbox-studio`: 하단 패널에 왼쪽/오른쪽 모듈 시리얼 로그 뷰어(나란히/단독 보기, 일시정지, 검색, 파일 저장, 패닉·재부팅 강조) 추가
- `dotbox-studio`: 에셋 탐색기 관리 기능 추가 (새 에셋 만들기 모달, 이름 바꾸기, 복제, 삭제 확인 다이얼로그, OS 탐색기 열기 및 "기타" 파일 분류 표시)
- `dotbox-studio`: 32×16 장면(화면) 미리보기 탭 추가 (임시 배치용 스크래치패드, 프로젝트 스프라이트 및 현재 캔버스 추가, 텍스트 배치 및 내장 3×5 폰트/TTF 변환기 연동, 마우스 드래그 위치 이동)