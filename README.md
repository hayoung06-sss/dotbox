# DotBox Engine

ESP32 기반 도트매트릭스 휴대용 게임기 **DotBox**용 게임 엔진입니다. Rust로 작성했고, 소스 코드로만 배포합니다.

이 저장소에는 엔진만 들어 있습니다. 게임은 별도 crate나 저장소에서 엔진을 의존성으로 가져다 씁니다.

> 현재 엔진은 골격 단계입니다. 아래 API(`Engine`, `dotbox_sim::run`)는 바뀔 수 있고,
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
  dotbox-sim/     # PC 실행기. 창에 32×16 화면을 그리고 키보드를 입력으로 쓴다
examples/
  hello/          # 엔진 동작 확인용 최소 예제
platform/
  dotbox-esp/     # (예정) ESP32 실행기
```

### 3. 빌드와 테스트

저장소 루트에서 실행합니다.

```sh
cargo build           # 엔진과 예제 전체 빌드
cargo test            # 테스트 실행
cargo run -p hello    # hello 예제를 PC 시뮬레이터로 실행
```

### 4. 외부 게임에서 엔진 쓰기

게임 crate의 `Cargo.toml`에 엔진을 git 의존성으로 추가합니다.

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

게임 쪽이 `main`을 갖고, 실행할 플랫폼의 실행기를 호출합니다.

```rust
use dotbox_core::Engine;

fn main() {
    // PC에서 실행
    dotbox_sim::run(Engine::new());
}
```

실기기에서는 같은 게임 코드를 두고 `dotbox_sim::run` 대신 `dotbox_esp::run`을 호출합니다(예정).
게임 로직은 `dotbox-core`의 트레잇만 사용하므로 플랫폼에 따라 고칠 필요가 없습니다.

### 5. 예제 추가하기

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

### 6. 실기기용 펌웨어 빌드 (예정)

`dotbox-esp`가 준비되면 게임 저장소에 펌웨어용 바이너리를 따로 두고 다음 파일을 갖춥니다.

- `rust-toolchain.toml`: `channel = "esp"`
- `.cargo/config.toml`: `target = "xtensa-esp32-espidf"`, `runner = "espflash flash --monitor"`
- `sdkconfig.defaults`

그다음 `cargo run --release`로 빌드, 플래시, 모니터를 한 번에 실행합니다.

Windows에서는 ESP-IDF 빌드가 경로 길이 제한에 걸릴 수 있습니다.
`.cargo/config.toml`에서 `CARGO_TARGET_DIR`를 `C:\dbx` 같은 짧은 ASCII 경로로 지정하세요.
경로에 한글, 공백, OneDrive가 들어가면 빌드가 실패합니다.
