# `dotbox` CLI 명세 (v0.1)

> 상태: **1단계 구현됨**(`new`, `doctor`, `run`, `build`, `flash`, `monitor`, `studio`).
> 2단계(`assets`, `font`, 양쪽 동시 모니터)는 아직 구현하지 않았다. 동작을 바꿀 때는 이 문서를 먼저 고친다.

## 1. 목적

`dotbox`는 DotBox 게임 개발자가 쓰는 호스트(PC) 명령줄 도구다. 다음 일을 대신한다.

- 게임 프로젝트 생성. 시뮬레이터용·펌웨어용 crate와 ESP 빌드 설정을 갖춘다.
- 개발 환경 점검. 툴체인과, ESP-IDF 빌드를 깨뜨리는 경로 문제를 미리 찾는다.
- 시뮬레이터 실행, 펌웨어 빌드, 왼쪽/오른쪽 모듈 플래시, 시리얼 모니터.
- 개발 도구 GUI(DotBox Studio) 실행.
- (2단계) 에셋 변환과 검사.

## 2. 구성과 원칙

```
tools/
  dotbox-tools/    # 라이브러리. 모든 기능의 실제 구현
  dotbox-cli/      # 바이너리 `dotbox`. 인자 해석과 출력만 한다
  dotbox-studio/   # GUI 바이너리 `dotbox-studio` (다른 팀원 담당)
```

- **로직은 `dotbox-tools`에 둔다.** `dotbox-cli`는 인자를 해석해 `dotbox-tools` 함수를 부르고 결과를 출력하기만 한다.
  Studio도 같은 함수를 직접 부른다. 따라서 CLI 명령과 Studio 기능은 1:1로 대응한다.
- `dotbox-tools`는 게임의 `build.rs`도 쓰므로 GUI·터미널 관련 의존성을 넣지 않는다.
- 인자 해석은 `clap`(derive)으로 한다.
- 외부 도구(`cargo`, `espflash`, `git`)는 자식 프로세스로 실행한다. 버전 결합을 줄이기 위해서다.
- 외부 명령 구성, 설정 해석, 점검 판정은 순수 함수로 나눠 PC에서 테스트한다.
- 명령 이름은 `dotbox` 하나만 제공한다. `cargo dotbox` 형태는 만들지 않는다.

## 3. 설치와 버전

```sh
cargo install --git https://github.com/hayoung06-sss/dotbox.git dotbox-cli
cargo install --git https://github.com/hayoung06-sss/dotbox.git dotbox-studio   # GUI가 필요할 때만
```

- CLI 버전은 엔진 workspace 버전과 같다. `dotbox --version`으로 확인한다.
- 엔진 릴리스 태그는 `v<버전>`(예: `v0.1.0`)이다. 첫 릴리스 전에는 태그가 없다.
- 게임 프로젝트가 쓰는 엔진 버전(`Cargo.lock`의 `dotbox-core`)과 CLI 버전이 다르면 `doctor`가 경고한다.

## 4. 공통 규칙

### 4.1 전역 옵션

| 옵션 | 설명 |
|---|---|
| `-C, --project <DIR>` | 게임 프로젝트 경로. 생략하면 현재 디렉토리에서 위로 올라가며 `dotbox.toml`을 찾는다. `new`에서는 새 프로젝트 경로의 기준 디렉토리다 |
| `-v, --verbose` | 실행하는 외부 명령을 보여 준다 |
| `-q, --quiet` | 오류와 경고 외 출력을 줄인다 |
| `-h, --help` / `-V, --version` | 도움말 / 버전 |

### 4.2 출력

- 사람이 읽는 메시지와 진행 상황은 **stderr**로, 명령의 결과물(만든 경로, 빌드한 파일, JSON)은 **stdout**으로 낸다.
- 메시지는 한국어로 쓴다. 오류는 `오류: <원인>`과 `도움말: <해결 방법>` 두 줄로 낸다.
- `--json`을 지원하는 명령은 stdout에 JSON 하나만 출력한다. 키는 영어 `snake_case`이고, 필드 추가는 호환으로 본다.

### 4.3 종료 코드

| 코드 | 의미 |
|---|---|
| 0 | 성공 |
| 1 | 작업 실패(빌드 실패, 점검 오류, 프로젝트 없음 등) |
| 2 | 사용법 오류(잘못된 인자, 함께 쓸 수 없는 옵션) |

### 4.4 프로젝트가 필요한 명령

`run`, `build`, `flash`, `monitor`는 게임 프로젝트 안에서만 동작한다.
`new`, `doctor`, `studio`는 프로젝트 밖에서도 동작한다.

## 5. 게임 프로젝트 구조

`dotbox new`가 만드는 구조이고, 다른 명령은 이 구조를 전제로 한다. `<name>`은 프로젝트 이름이다.

```
<name>/
  dotbox.toml            # 프로젝트 설정 (5.1)
  Cargo.toml             # 호스트 workspace: game, sim. firmware는 exclude
  README.md
  .gitignore             # /target, /.dotbox
  game/                  # 패키지 `<name>`(라이브러리). 게임 로직. dotbox-core에만 의존한다
  sim/                   # 패키지 `<name>-sim`(바이너리). dotbox_sim::run 호출
  firmware/              # 패키지 `<name>-firmware`(바이너리). dotbox_esp::run 호출. 별도 툴체인이라 workspace 밖
    Cargo.toml
    build.rs             # embuild로 ESP-IDF 환경 전달
    rust-toolchain.toml  # channel = "esp"
    .cargo/config.toml   # target, linker, runner, target-dir, ESP-IDF 설정
    sdkconfig.defaults   # panic 시 재부팅, 4 MB 플래시, 메인 태스크 스택
    partitions.csv       # 5.3
    espflash.toml        # partition_table 지정
  assets/                # 원본 에셋(2단계). Studio가 편집하고 game/build.rs가 변환한다
  .dotbox/               # 개인 설정 (gitignore)
    local.toml           # 5.2
```

- 게임 로직(`game/`)은 플랫폼을 모른다. `sim/`과 `firmware/`는 실행기를 호출하는 몇 줄짜리 `main`만 가진다.
- `firmware/`의 `rust-toolchain.toml` 덕분에 그 디렉토리에서 실행한 `cargo`는 자동으로 esp 툴체인을 쓴다. CLI는 `+esp`를 붙이지 않는다.
- Windows에서는 펌웨어 target 디렉토리를 `C:/dbx/<name>`, ESP-IDF 도구 설치 위치를 `C:/dbx/espressif`로 둔다.
  `.cargo/config.toml`에 적으므로 CLI 없이 `cargo build`를 직접 실행해도 같은 설정이 적용된다.
- `dotbox-esp`가 아직 없으므로 지금 만든 `firmware/`는 빌드되지 않는다. `dotbox-esp`를 만들 때 템플릿을 함께 검증한다.

### 5.1 `dotbox.toml`

```toml
[project]
name = "my-game"          # 필수. 5.4의 이름 규칙

[paths]                   # 생략하면 아래 기본값. 프로젝트 루트 기준 상대 경로
game = "game"
sim = "sim"
firmware = "firmware"
assets = "assets"
```

알 수 없는 키가 있으면 오류로 처리한다(오타 방지).

### 5.2 `.dotbox/local.toml` (개인 설정, 커밋하지 않음)

```toml
[ports]
left = "COM5"
right = "COM6"

[flashed]                 # 마지막으로 구운 이미지의 build_id (5.5)
left = "3f9a1c0e7b2d4a61"
right = "3f9a1c0e7b2d4a61"
```

- `flash`와 `monitor`는 포트를 지정하지 않으면 `[ports]` 값을 쓴다. 플래시에 성공하면 쓴 포트를 여기에 저장한다.
- `flash`는 성공할 때마다 `[flashed]`를 갱신한다.

### 5.3 파티션 테이블

플래시 4 MB를 가정한다. 보드가 확정되면 다시 확인한다.

| 이름 | 종류 | 오프셋 | 크기 | 용도 |
|---|---|---|---|---|
| `nvs` | data/nvs | 0x9000 | 0x6000 | ESP-IDF 기본 |
| `phy_init` | data/phy | 0xF000 | 0x1000 | ESP-IDF 기본 |
| `dotbox` | data/0x40 | **0x10000** | 0x1000 | 역할 레코드([pairing.md](pairing.md) 2.1) |
| `factory` | app/factory | 0x20000 | 0x3E0000 | 펌웨어 |

### 5.4 이름 규칙

- ASCII 소문자로 시작하고, 소문자·숫자·`-`만 쓴다. `-`로 끝나거나 `--`를 포함할 수 없다. 최대 32자.
- Rust 코드에서는 `-`를 `_`로 바꾼 이름(`my_game`)으로 쓴다.

### 5.5 `build_id`

펌웨어 ELF 파일 SHA-256의 앞 8바이트를 16진수 16자로 쓴 값이다. 두 모듈에 같은 이미지를 구웠는지 CLI가 확인하는 데 쓴다.

## 6. 명령

### 6.1 `dotbox new <PATH>`

새 게임 프로젝트를 `<PATH>`에 만든다. 프로젝트 이름은 `<PATH>`의 마지막 부분이다.

| 옵션 | 설명 |
|---|---|
| `--name <NAME>` | 프로젝트 이름을 따로 지정한다 |
| `--engine-git <URL>` | 엔진 git 저장소. 기본은 공식 저장소 |
| `--engine-rev <REV>` | 고정할 커밋이나 태그. 첫 릴리스 전 기본값은 없음(기본 브랜치) |
| `--engine-path <DIR>` | git 대신 로컬 엔진 저장소를 쓴다. `--engine-git`, `--engine-rev`와 함께 쓸 수 없다 |
| `--no-firmware` | `firmware/`를 만들지 않는다(시뮬레이터만) |
| `--no-git` | `git init`을 하지 않는다 |

- `<PATH>`가 있고 비어 있지 않으면 실패한다.
- `--engine-path`가 엔진 저장소가 아니면(`crates/dotbox-core/Cargo.toml` 없음) 실패한다.
- 만든 뒤 경로 점검(6.2의 `path.*`)에서 문제가 나오면 경고한다. 시뮬레이터는 동작하지만 펌웨어 빌드는 실패할 수 있다.
- `git`이 없거나 `git init`이 실패하면 경고만 하고 계속한다.
- stdout에 만든 프로젝트의 경로를 출력한다.

### 6.2 `dotbox doctor`

개발 환경을 점검하고 항목별로 `ok` / `info` / `warn` / `error`와 해결 방법을 출력한다. `error`가 하나라도 있으면 종료 코드는 1이다.

| 옵션 | 설명 |
|---|---|
| `--host-only` | 시뮬레이터 개발에 필요한 항목만 점검한다 |
| `--json` | 결과를 JSON으로 출력한다 |

| 검사 ID | 내용 | 실패 수준 | `--host-only` | 프로젝트 필요 |
|---|---|---|---|---|
| `rust.version` | `rustc` 1.85 이상(edition 2024) | error | 검사 | |
| `esp.toolchain` | `rustup toolchain list`에 `esp`가 있음 | error | 생략 | |
| `esp.espflash` | `espflash`가 PATH에 있음 | error | 생략 | |
| `esp.ldproxy` | `ldproxy`가 PATH에 있음 | error | 생략 | |
| `serial.ports` | 연결된 USB 시리얼 포트 목록 | info | 생략 | |
| `project` | 프로젝트를 찾았는지 | info | 검사 | |
| `engine.version` | `Cargo.lock`의 `dotbox-core` 버전과 CLI 버전 일치. lock 파일이 없으면 info | warn | 검사 | ✓ |
| `path.ascii` | 프로젝트 경로가 ASCII만 씀 | error | 생략 | ✓ |
| `path.spaces` | 경로에 공백이 없음 | error | 생략 | ✓ |
| `path.onedrive` | 경로에 `OneDrive`로 시작하는 폴더가 없음 | error | 생략 | ✓ |
| `path.length` | 프로젝트 경로가 80자 이하 | warn | 생략 | ✓ |
| `firmware.target_dir` | `firmware/.cargo/config.toml`의 `target-dir`이 ASCII·공백 없음·OneDrive 아님(error), 24자 이하(warn). Windows에서 설정이 없으면 error | error | 생략 | ✓ |

- 경로 길이 기준(80자, 24자)은 잠정값이다. ESP-IDF 빌드로 실측한 뒤 조정한다.
- 프로젝트 밖에서는 "프로젝트 필요" 항목을 건너뛴다. `-C`로 지정한 경로가 프로젝트가 아니면 오류다.
- `firmware/`가 없는 프로젝트(`--no-firmware`)에서는 `firmware.target_dir`를 건너뛴다.

`--json` 출력 형태:

```json
{
  "checks": [
    { "id": "path.onedrive", "level": "error", "message": "...", "hint": "..." }
  ]
}
```

`hint`는 없으면 생략한다.

### 6.3 `dotbox run`

시뮬레이터로 게임을 실행한다. `cargo run --manifest-path sim/Cargo.toml`과 같다.

| 옵션 | 설명 |
|---|---|
| `--release` | 릴리스 빌드로 실행한다 |
| `-- <ARGS>...` | 나머지 인자를 게임 실행 파일에 넘긴다 |

### 6.4 `dotbox build`

펌웨어를 빌드한다. 플래시는 하지 않는다. 두 모듈은 같은 이미지를 쓰므로 모듈을 고르는 옵션이 없다.

| 옵션 | 설명 |
|---|---|
| `--debug` | 디버그 빌드. 기본은 릴리스(ESP32 디버그 빌드는 크고 느리다) |

- 빌드하기 전에 `path.*`(error 수준)와 `firmware.target_dir`을 점검하고, 실패하면 빌드하지 않는다.
- `firmware/`에서 `cargo build --message-format=json-render-diagnostics`를 실행해 ELF 경로를 얻는다.
- stdout에 ELF 경로를, stderr에 `build_id`를 출력한다.

### 6.5 `dotbox flash`

펌웨어를 빌드해 모듈에 굽고 역할을 기록한다. 역할 결정과 펌웨어 일치 확인은 [pairing.md](pairing.md)를 따른다.

| 옵션 | 설명 |
|---|---|
| `--side <left\|right\|both>` | **필수.** 대상 모듈. `both`를 권장한다 |
| `-p, --port <PORT>` | 시리얼 포트. `--side left\|right`에서만 쓴다 |
| `--left-port <PORT>` / `--right-port <PORT>` | 모듈별 포트. `--side both`에서만 쓴다 |
| `--monitor` | 굽고 나서 바로 시리얼 모니터를 연다. `--side left\|right`에서만 쓴다(양쪽 동시는 2단계) |
| `--debug` | 디버그 빌드 |

포트 결정 순서(모듈마다):

1. 명령줄로 지정한 포트
2. `.dotbox/local.toml`의 `[ports]`. 감지된 포트 목록에 없으면 실패한다
3. 자동 감지: USB 시리얼 포트가 **정확히 하나**이고 `--side`가 하나일 때만 그 포트를 쓴다

어느 포트가 어느 모듈인지 알 수 없으면 고르지 않고 실패한다. 두 모듈이 같은 포트로 정해져도 실패한다.
처음에는 `--left-port`/`--right-port`나 `-p`로 한 번 지정하면 그다음부터는 저장된 값을 쓴다.

순서:

1. **모든 대상의 포트를 먼저 정한다.** 하나라도 실패하면 빌드도 하지 않는다. 한쪽만 구워져 불일치가 생기는 것을 막기 위해서다.
2. 펌웨어를 **한 번** 빌드한다(6.4와 같은 점검 포함).
3. 대상 모듈마다:
   1. `espflash flash --port <PORT> --partition-table firmware/partitions.csv <ELF>`
   2. 역할 레코드(16바이트)를 `.dotbox/role-<side>.bin`에 쓰고 `espflash write-bin --port <PORT> 0x10000 <파일>`
   3. `.dotbox/local.toml`의 `[ports]`와 `[flashed]`를 갱신한다
4. 두 모듈의 `[flashed]` 값이 다르면 경고한다: "왼쪽과 오른쪽 모듈의 펌웨어가 다릅니다. `dotbox flash --side both`로 다시 구우세요."
   기록이 없는 쪽은 비교하지 않는다.
5. `--monitor`이면 마지막에 `espflash monitor`를 연다.

기기 쪽 확인(pairing.md 3장)이 최종 안전장치다. CLI 기록은 다른 PC에서 구운 경우를 알 수 없다.

### 6.6 `dotbox monitor`

시리얼 로그를 본다. `espflash monitor --port <PORT>`를 실행한다.

| 옵션 | 설명 |
|---|---|
| `--side <left\|right>` | 기본 `left` |
| `-p, --port <PORT>` | 시리얼 포트. 결정 순서는 6.5와 같다 |

`--side both`(두 포트를 함께 열고 줄마다 `[L]`/`[R]`을 붙이는 기능)는 2단계에서 추가한다.

### 6.7 `dotbox studio [DIR]`

DotBox Studio를 실행한다. `DIR`을 생략하면 현재 프로젝트(없으면 현재 디렉토리)를 연다.

- PATH에서 `dotbox-studio` 실행 파일을 찾아 `dotbox-studio <DIR>`로 실행한다(git의 외부 하위 명령 방식).
  CLI 자체는 GUI 의존성을 갖지 않는다.
- 찾지 못하면 설치 명령을 안내하고 종료 코드 1로 끝낸다.

### 6.8 2단계 명령 (미구현)

| 명령 | 내용 |
|---|---|
| `dotbox assets check` | `assets/`의 원본을 모두 변환해 보고 오류만 보고한다 |
| `dotbox assets build --out <DIR>` | 변환 결과(생성된 Rust 소스)를 `<DIR>`에 쓴다 |
| `dotbox font convert` / `preview` | 글꼴 변환과 미리보기. 스네이크·핑퐁에 필요한 범위(숫자 글꼴)를 먼저 확인한 뒤 범위를 정한다 |

평소 빌드에서는 `game/build.rs`가 `dotbox-tools`를 불러 `OUT_DIR`에 생성한다. 원본·생성 형식은 에셋 형식 문서(미작성)에서 Studio 담당과 정한다.

## 7. `dotbox-tools` 대응표

| 명령 | `dotbox-tools` 함수 |
|---|---|
| `new` | `project::create` |
| `doctor` | `doctor::run` → `Report` |
| `run` | `cargo::run_sim` |
| `build` | `cargo::build_firmware` |
| `flash` | `device::flash` |
| `monitor` | `device::monitor` |
| `studio` | `studio::launch` |

## 미결 사항

- 경로 길이 기준값(80자, 24자). ESP-IDF 빌드로 실측해 정한다.
- 플래시 용량과 파티션 크기. 보드(CMODULE-43) 확정 후 확인한다.
- 펌웨어 템플릿(`build.rs`, `.cargo/config.toml`, `sdkconfig.defaults`)의 실제 빌드 검증. `dotbox-esp`와 함께 한다.
- 에셋 원본·생성 형식. 별도 문서에서 Studio 담당과 합의한다.
