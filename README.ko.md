<p align="center">
  <img src="crates/lapiswitch-gui/assets/branding/lapiswitch.png" alt="lapiSwitch" width="96">
</p>

<h1 align="center">lapiSwitch</h1>

<p align="center">Lenovo TB520FU(<code>lapis</code>)의 PRC ↔ ROW 리전 변환 도구</p>

<p align="center">
  <img src="https://img.shields.io/badge/Windows-x64-0078D6" alt="Windows x64">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue" alt="GPL-3.0-or-later"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · 한국어 · <a href="https://github.com/corrupt3904/lapiSwitch/releases">다운로드</a>
</p>

lapiSwitch는 **Lenovo TB520FU**(코드네임 `lapis`, Yoga Tab Plus)의 **PRC(중국 내수) ↔ ROW(글로벌)** 리전 변환을 연구·실험하기 위해 개발한 비공식 Windows 도구입니다.

lapiSwitch는 **[@miner7222](https://github.com/miner7222)님의 [LTBox](https://github.com/miner7222/LTBox)를 기반으로 만든 독립 도구**이며, **[@wnduddld0513](https://github.com/wnduddld0513)님이 배포한 [TB520FU용 TWRP](https://github.com/lenovo-sm8650/android_device_lenovo_lapis-twrp/releases/tag/v3.7.1_16-20261014)**를 사용합니다.

TWRP에서 DeviceInfo를 백업·변경·검증한 뒤 원본 GPT·Recovery를 복원하고 선택한 리전의 공식 펌웨어를 설치합니다.

> [!WARNING]
> **사용 전 반드시 확인하세요**
>
> - 펌웨어·리전 변경으로 **부팅 불가(벽돌), 기기 손상 또는 데이터 손실**이 발생할 수 있으며, 복구를 보장하지 않습니다.
> - **리전을 바꾸면 펌웨어 설치가 필수이며 사용자 데이터가 반드시 초기화됩니다.** 태블릿에 저장된 앱·사진·계정 등이 삭제됩니다. 리전을 바꾸지 않는 DeviceInfo 수정은 펌웨어를 설치하지 않고 데이터를 유지합니다.
> - DeviceInfo의 보안·구조 필드를 잘못 바꾸면 부팅되지 않거나 보안 상태가 바뀔 수 있습니다.
> - 변경 작업으로 **제조사 보증이 무효가 되거나 수리·지원이 제한될 수 있습니다.** 적용 여부는 보증 조건과 관련 법령에 따라 달라집니다.
>
> 개인 파일은 별도로 백업하고, 앱이 생성한 원본 백업과 로그도 보관하세요. 작업 과정과 위험을 이해한 뒤 시작하고, 작업 중에는 USB 연결을 유지하세요.

본 소프트웨어는 관련 법령이 허용하는 범위에서 **어떠한 보증 없이 ‘있는 그대로’ 제공**됩니다. 개발자와 기여자는 사용 또는 사용 불능으로 발생한 기기 손상·데이터 손실 및 기타 손해에 대해 법적으로 배제할 수 없는 책임을 제외하고 책임을 지지 않습니다. 자세한 내용은 [LICENSE](LICENSE)의 15·16항을 확인하세요.

## 주요 기능

- **DeviceInfo 수정(기본)** — TWRP에서 읽은 DeviceInfo 구조 맵의 모든 필드(리전, 롤백인덱스, 보안·구조 필드 포함)를 직접 수정합니다. 입력은 필드 형식·크기에 맞아야 하며, 쓰기 전 변경 목록을 확인합니다. 리전을 바꾸면 해당 리전 펌웨어 폴더 선택·검사가 필수이고 펌웨어 설치와 데이터 초기화가 진행됩니다. 리전을 바꾸지 않으면 원본 구성 복원 후 재부팅만 합니다.
- **단계별 펌웨어 플래싱 안내** — 펌웨어 폴더 검사(vendor_boot로 리전 확인), 플래싱 설정, 최종 확인.
- **연결 상태 확인** — **기기** 화면(왼쪽 메뉴 맨 위 기기 카드)에서 ADB·Fastboot·EDL 상태 표시, 드라이버 점검 및 지원되는 설치 기능.
- **백업과 검증** — 원본 파티션 백업과 DeviceInfo 쓰기 후 4096바이트 전체 재읽기 검증.
- **변환 옵션** — 국가 코드 선택, 롤백인덱스 초기화(기본값) 또는 유지. 유지를 선택했는데 설치할 펌웨어의 롤백인덱스가 DeviceInfo에 저장된 값보다 낮으면, TWRP 단계에서 0으로 초기화 후 계속할지 작업을 취소하고 원본 구성을 자동 복구할지 묻습니다.
- **작업 기록 보존** — 저장된 상태와 백업 검증 결과에 따른 이어하기·원본 구성 복구.

지원 언어: 한국어, English, 简体中文, 日本語, Русский. 화면은 Windows 11 스타일이며 라이트·다크 테마를 지원합니다(기본: Windows 설정 따름).

## 지원 조건

| 항목 | 조건 |
|---|---|
| PC 운영체제 | **Windows x64 전용** |
| 태블릿 | **Lenovo TB520FU(`lapis`)**, **TB522FU(`mojito`, 실기기 미검증)** — 다른 모델은 작업 차단 |
| 설치된 내수롬(PRC) | TB520FU: `ZUXOS_1.5.10.186` 이하, 이전 PRC ZUI 계열도 허용 · TB522FU: 빌드 날짜 2026-09-24(`…260924C`) 이하 |
| 설치된 글로벌롬(ROW) | TB520FU: `ZUI_18.5.10.037` 이하 · TB522FU: 빌드 날짜 2026-09-04(`…260904W`) 이하 |
| 설치할 펌웨어 | **목표 리전**과 일치하는 공식 펌웨어의 압축 해제 폴더 |
| 연결 환경 | 안정적인 USB 케이블, 충분한 배터리, 필요한 파일을 내려받을 인터넷 연결 |

기기별 값(모델, 버전 상한, TWRP, 국가코드 위치, DeviceInfo 필드 맵)은 [`devices/`](devices/README.md)에 있습니다. 위 버전 상한은 **현재 태블릿에 설치된 펌웨어**의 기준입니다. 설치할 펌웨어 패키지는 별도로 검사합니다. 순정롬으로 확인된 기기 펌웨어가 상한을 넘으면 변환을 차단합니다. 순정롬 버전·리전을 확인할 수 없는 경우에는 커스텀롬·GSI·테스트 빌드 가능성을 안내하고, 사용자가 계속 진행을 선택해야 합니다. STEP 5·7의 연결 대기는 30초 후 작업 중지가 가능하며, 3분이 지나면 기록을 보존하고 오류로 중단합니다. 각 프리릴리즈의 검증 범위와 제약은 [릴리즈 노트](https://github.com/corrupt3904/lapiSwitch/releases)를 확인하세요.

## 사용 방법

1. [Releases](https://github.com/corrupt3904/lapiSwitch/releases)에서 Windows ZIP과 `.zip.sha256` 체크섬 파일을 내려받습니다. 체크섬을 확인한 뒤 ZIP의 압축을 풉니다.
2. `lapiSwitch.exe`를 실행합니다. 앱의 USB 드라이버 안내를 확인하고, 필요한 경우 지원되는 드라이버를 설치합니다.
3. 태블릿을 Android로 부팅하고 USB 디버깅을 켠 뒤 연결합니다. **기기** 화면(왼쪽 메뉴 맨 위 기기 카드)에서 모델과 연결 상태를 확인하고 USB 디버깅 승인 요청을 허용합니다. 변환은 ADB(또는 EDL)에서 시작하며, Fastboot에서는 시작할 수 없습니다.
4. **DeviceInfo 수정**을 선택하고 EDL 로더를 지정합니다. TWRP에서 DeviceInfo를 읽으면 편집 창에서 값을 바꾸고 변경 내용을 확인한 뒤 적용합니다. 리전을 바꾸는 경우 편집 창에서 해당 리전 펌웨어 폴더를 선택합니다. 펌웨어만 바로 설치하려면 **펌웨어 플래싱 (Region 변경)**을 선택하고 펌웨어 폴더를 지정합니다. 목표 리전은 펌웨어 vendor_boot의 DTB로 정해지며, 이어서 국가 코드와 롤백인덱스 처리를 고른 뒤 최종 확인에서 시작합니다.
5. 작업이 끝날 때까지 USB 연결을 유지하고, 재부팅 확인창 등 앱의 안내에 따라 진행합니다.

전체 흐름은 **검사·원본 백업 → TWRP / DeviceInfo 변경·검증 → 원본 구성 복원 → (리전 변경 시) 펌웨어 설치 → 재부팅**입니다. 편집 창에서 취소하면 DeviceInfo를 바꾸지 않고 원본 구성을 자동으로 복구합니다.

TWRP는 지정된 [lapis용 릴리즈 `v3.7.1_16-20261014`](https://github.com/lenovo-sm8650/android_device_lenovo_lapis-twrp/releases/tag/v3.7.1_16-20261014)에서 별도로 내려받으며, 사용할 때마다 파일 크기와 SHA-256을 확인합니다.

## 백업과 원본 복구

기본 저장 위치는 `%LOCALAPPDATA%\lapiSwitch`입니다.

| 내용 | 하위 경로 |
|---|---|
| 작업 로그 | `logs` |
| 원본 백업 | `backup\region_change` |

**설정 → 로그 및 백업 위치**에서 저장 위치를 바꿀 수 있습니다. 백업 폴더 전체와 작업 기록을 함께 보관하세요.

왼쪽 메뉴의 **원본 복구**는 백업한 GPT·Recovery·vendor_boot 구성과, DeviceInfo가 바뀐 경우 작업 전 DeviceInfo 전체를 복원합니다. **설치된 펌웨어 전체를 이전 버전으로 되돌리는 기능은 아닙니다.**

이어하기와 원본 복구는 확인 가능한 기록을 기준으로 진행됩니다. DeviceInfo 쓰기 결과를 확인할 수 없거나 백업 검증에 실패하면 자동 복구가 거부될 수 있습니다.

이전 버전 앱으로 만든 백업과 작업 기록은 이 버전에서 열 수 없습니다. 중단된 작업이 있다면 업데이트 전에 그 작업을 만든 버전으로 마치거나 원본 복구를 완료하세요.

## 소스에서 빌드

[rustup으로 Rust](https://rustup.rs/)를 설치하고, Visual Studio Build Tools의 **C++ 빌드 도구와 Windows SDK**를 준비합니다. Rust **1.95.0**은 [`rust-toolchain.toml`](rust-toolchain.toml)에 지정되어 있습니다.

저장소 루트에서 실행합니다.

```powershell
cargo build --release --locked -p lapiswitch-gui
```

실행 파일은 `target\release\lapiSwitch.exe`에 생성됩니다. 다시 빌드할 때는 실행 중인 lapiSwitch를 먼저 닫으세요.

기기를 연결하지 않고 화면을 미리 보려면 다음 명령을 사용합니다.

```powershell
cargo run --locked -p lapiswitch-gui -- --demo
```

Android helper와 해당 검증 범위는 [`tools/deviceinfo/README.md`](tools/deviceinfo/README.md)를 참고하세요.

지원 기기 정보(모델명, 펌웨어 검사, 설치 펌웨어 허용 범위, TWRP, DeviceInfo 필드 맵)는 [`devices/`](devices/README.md) 폴더의 기기별 파일에 있으며, 빌드할 때 검사해 프로그램에 포함합니다.

## 크레딧

- **[LTBox](https://github.com/miner7222/LTBox) — [@miner7222](https://github.com/miner7222):** 기기 통신, EDL 플래시, 드라이버·펌웨어 처리와 일부 GUI 자산의 기반입니다. 포함된 `ltbox-core`·`ltbox-device` 소스는 수정 없이 유지합니다. AVB 분석에는 [avbtool-rs](https://github.com/miner7222/avbtool-rs)를 사용합니다.
- **[TeamWin / TWRP](https://twrp.me/), TWRP-Test 및 lapis 기여자:** [TB520FU용 Recovery 빌드](https://github.com/lenovo-sm8650/android_device_lenovo_lapis-twrp/releases/tag/v3.7.1_16-20261014)는 [@wnduddld0513](https://github.com/wnduddld0513)님이 [lenovo-sm8650](https://github.com/lenovo-sm8650/android_device_lenovo_lapis-twrp)에 배포한 릴리즈를 사용합니다. [TB522FU용 Recovery 빌드](https://github.com/lanweikayu/android_device_lenovo_TB522FU/releases/tag/TWRP-3.7.1_16)는 lanweikayu님이 배포한 릴리즈를 사용합니다.
- **기타 구성요소:** [qdlrs](https://github.com/qualcomm/qdlrs), [flag-icons](https://github.com/lipis/flag-icons), [Noto Sans KR](https://github.com/notofonts/noto-cjk), [iced](https://github.com/iced-rs/iced), [`Cargo.lock`](Cargo.lock)에 기록된 Rust 의존성.

## 라이선스

lapiSwitch는 LTBox를 기반으로 개발한 독립 프로젝트이며, **GPL-3.0-or-later**로 배포됩니다. [LICENSE](LICENSE)를 확인하세요. Lenovo의 공식 소프트웨어가 아닙니다.

서드파티 구성요소에는 각각의 라이선스가 적용됩니다. **설정 → 오픈소스 라이선스 → 라이선스 보기**에서 포함된 고지와 라이선스 원문을 오프라인으로 읽을 수 있으며, TWRP / TeamWin 항목에는 원문과 출처가 함께 표시됩니다. GUI 자산의 출처는 [`crates/lapiswitch-gui/assets/README.md`](crates/lapiswitch-gui/assets/README.md)에 정리되어 있습니다.

제품명과 상표의 권리는 각 소유자에게 있습니다.
