# DotBox Engine

ESP32 기반 도트매트릭스 휴대용 게임기 **DotBox**용 게임 엔진입니다. Rust로 작성했고, 소스 코드로만 배포합니다.

이 저장소에는 엔진과 개발 도구(`dotbox` CLI)가 들어 있습니다. 게임은 별도 저장소에서 엔진을 의존성으로 가져다 씁니다.

> 현재 엔진은 초기 단계입니다. 아래 API(`Game`, `dotbox_sim::run`)는 바뀔 수 있고,
> ESP32 실행기(`dotbox-esp`)는 아직 없습니다. 변경 내역은 [CHANGELOG.md](CHANGELOG.md)에 기록합니다.

## 사용 방법

### 1. 준비

- Rust stable (edition 2024를 지원하는 1.85 이상)
- 실기기용 펌웨어를 빌드할 때만 ESP 툴체인이 필요합니다.

  ```sh
  cargo install espup ldproxy espflash
  espup install
  ```

### 2. 저장소 구조

```
crates/
  dotbox-core/    # 엔진 본체. 하드웨어 독립. 게임은 이 crate에 의존한다
                  # 화면 버퍼, 하드웨어 트레잇, 고정 틱, 모듈 역할 레코드
  dotbox-sim/     # PC 실행기. 창에 32×16 화면을 그리고 키보드를 입력으로 쓴다
examples/
  hello/          # 엔진 동작 확인용 2인 예제. 두 플레이어가 각자 점을 움직인다
platform/
  dotbox-esp/     # (예정) ESP32 실행기. esp 툴체인이 따로 필요해 workspace에서 뺀다
tools/
  dotbox-tools/   # 개발 도구 라이브러리. CLI와 Studio가 함께 쓰는 실제 구현
  dotbox-cli/     # `dotbox` 명령줄 도구. 프로젝트 생성, 환경 점검, 실행, 플래시
  dotbox-studio/  # (예정) GUI 개발 도구
docs/             # 명세 (CLI, 모듈 연결, 합체·분리 모드, 오류 처리)
```

### 3. 빌드와 테스트

저장소 루트에서 실행합니다.

```sh
cargo build                       # 엔진, 도구, 예제 전체 빌드
cargo test                        # 테스트 실행 (CLI 통합 테스트 포함)
cargo run -p hello                # hello 예제를 PC 시뮬레이터 창으로 실행
cargo run -p dotbox-cli -- doctor # CLI를 설치하지 않고 실행
```

### 4. `dotbox` CLI로 게임 만들기 (권장)

```sh
cargo install --git https://github.com/hayoung06-sss/dotbox.git dotbox-cli

dotbox new snake            # 게임 프로젝트 생성 (game, sim, firmware, assets)
cd snake
dotbox doctor               # 개발 환경 점검
dotbox run                  # PC 시뮬레이터로 실행
dotbox flash --side both    # 두 모듈에 같은 펌웨어를 굽고 역할 기록 (dotbox-esp 완성 후)
dotbox monitor --side left  # 시리얼 로그
```

엔진을 함께 고치면서 개발할 때는 `dotbox new snake --engine-path ../DotBoxEngine`으로 로컬 엔진을 씁니다.
명령 전체와 프로젝트 구조는 [docs/cli.md](docs/cli.md)에 있습니다.

### 5. 직접 엔진 의존성 추가하기

CLI 없이 쓸 때는 게임 crate의 `Cargo.toml`에 엔진을 git 의존성으로 추가합니다.

```toml
[dependencies]
dotbox-core = { git = "https://github.com/hayoung06-sss/dotbox.git" }
dotbox-sim  = { git = "https://github.com/hayoung06-sss/dotbox.git" }
```

엔진을 로컬에서 함께 고치면서 개발할 때는 경로 의존성을 씁니다.

```toml
[dependencies]
dotbox-core = { path = "../DotBoxEngine/crates/dotbox-core" }
dotbox-sim  = { path = "../DotBoxEngine/crates/dotbox-sim" }
```

게임은 `dotbox_core::Game` 트레잇을 구현합니다. 게임 쪽이 `main`을 갖고, 게임을 만드는 함수를 실행기에 넘깁니다.

```rust
fn main() -> dotbox_core::Result<()> {
    // PC에서 실행. 게임이 여럿이면 || Games::new().with(Snake::new()).with(Pong::new())
    dotbox_sim::run(MyGame::new)
}
```

실기기에서는 같은 게임 코드를 두고 `dotbox_sim::run` 대신 `dotbox_esp::run`을 호출합니다(예정).
게임 API와 두 모듈의 동기화 방식은 [docs/game-api.md](docs/game-api.md)에 있고,
[examples/hello](examples/hello/src/main.rs)가 가장 작은 예입니다.

시뮬레이터 조작: 왼쪽 모듈(P1) WASD + Space, 오른쪽 모듈(P2) 방향키 + Enter, F2 링크 끊기/잇기, Esc 종료.

### 6. 예제 추가하기

`examples/` 아래에 폴더를 만들면 워크스페이스에 자동으로 포함됩니다.

```
examples/my-example/
  Cargo.toml
  src/main.rs
```

```toml
[package]
name = "my-example"
version.workspace = true
edition.workspace = true
publish = false

[dependencies]
dotbox-core.workspace = true
dotbox-sim.workspace = true
```

실행은 `cargo run -p my-example`로 합니다.

### 7. 실기기용 펌웨어 빌드 (예정)

`dotbox-esp`가 준비되면 `dotbox new`가 만든 `firmware/`에서 빌드합니다. 필요한 파일
(`rust-toolchain.toml`, `.cargo/config.toml`, `sdkconfig.defaults`, `partitions.csv`)은 `dotbox new`가 만들어 줍니다.

```sh
dotbox build                # 펌웨어 빌드
dotbox flash --side both    # 두 모듈에 굽고 역할(왼쪽/오른쪽) 기록
```

Windows에서는 ESP-IDF 빌드가 경로 길이 제한에 걸릴 수 있어, `dotbox new`가 펌웨어 target 디렉토리를
`C:/dbx/<게임 이름>`으로 잡습니다. 프로젝트 경로에 한글, 공백, OneDrive가 들어가면 빌드가 실패하므로
`dotbox doctor`로 먼저 확인하세요.
