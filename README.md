# lapiSwitch

Lenovo **TB520FU**(코드네임 **lapis**)의 **PRC ↔ ROW 리전 변환**을 위한 GUI 도구입니다.

TWRP에서 DeviceInfo를 읽고 변경한 뒤 재읽기로 검증하며, 원본 파티션 백업과 펌웨어 설치를 진행합니다. 백업을 이용한 원본 GPT·Recovery 등의 복구도 지원합니다.

## 사용 전 확인

- 지원 기기는 TB520FU입니다. 다른 모델에는 사용하지 마세요.
- **리전 변환 시 사용자 데이터 초기화는 필수입니다.** 중요한 파일은 먼저 별도로 백업하세요.
- 변환하려는 리전과 일치하는 펌웨어 패키지가 필요합니다.
- 작업 중에는 PC와 태블릿의 USB 연결을 유지하세요.
- 원본 복구는 설치된 전체 펌웨어를 과거 버전으로 되돌리는 기능이 아닙니다.

## 빌드 및 실행

저장소 루트에서 실행합니다. Rust 버전은 `rust-toolchain.toml`에 지정되어 있습니다.
Windows MSVC 빌드에는 Visual Studio Build Tools의 C++ 도구와 Windows SDK가 필요합니다.

```powershell
cargo run -p lapiswitch-gui
cargo build --release -p lapiswitch-gui
```

Windows 빌드 결과는 `target/release/lapiSwitch.exe`입니다.
기존 lapiSwitch가 실행 중이면 창을 닫은 뒤 다시 빌드하세요.

기기를 연결하지 않고 GUI를 미리 보려면 다음과 같이 실행합니다.

```powershell
cargo run -p lapiswitch-gui -- --demo
```

## 소스 구성

- `crates/lapiswitch-gui`: 사용자 화면
- `crates/lapiswitch-core`: 변환·검증·백업·복구 로직
- `crates/lapiswitch-cli`: 개발용 CLI
- `assets/lapiswitch_deviceinfo`: 앱에 내장되는 Android helper
- `tools/deviceinfo`: helper 소스와 빌드 안내
- `tools/gpt_Modify*.py`: GPT 검증용 참조 스크립트
- `vendor`: 빌드에 사용하는 LTBox 및 qdl 소스
- `misc/udev`: Linux USB 접근 규칙

테스트 코드와 helper·아이콘 재생성 도구를 함께 포함합니다.
빌드 결과물, 기기 덤프, 개인 백업·로그, 로컬 작업 기록은 저장소에서 제외합니다.

## 라이선스 및 출처

프로젝트 라이선스는 **GPL-3.0-or-later**입니다. [LICENSE](LICENSE)를 확인하세요.

[LTBox](https://github.com/miner7222/LTBox)의 코드와 자산을 재사용합니다.
qdl·국기·글꼴 등 제3자 구성요소의 출처와 라이선스는 각 구성요소의 LICENSE 및 문서를 확인하세요.
GUI 자산의 출처는 [자산 안내](crates/lapiswitch-gui/assets/README.md)에 정리되어 있습니다.
