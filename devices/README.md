# 지원 기기

기기마다 폴더 하나를 둡니다. 폴더 이름은 모델명과 같아야 합니다.

```
devices/
  TB520FU/
    device.toml      기기 정보와 처리 방식
    deviceinfo.toml  DeviceInfo 필드 맵 (쓰기 지원 기기에 필요)
    image.png        기기 화면·왼쪽 메뉴의 기기 이미지 (선택)
  TB522FU/
    ...
```

빌드할 때 `crates/lapiswitch-devices`가 이 폴더를 모두 읽어 프로그램에 넣습니다.
파일이 잘못되면 빌드가 실패하고 어느 기기·항목이 문제인지 알려 줍니다.
코드는 모델 이름으로 갈라지지 않고, 이 파일의 값과 선택한 처리 방식만 따릅니다.

## device.toml

| 항목 | 내용 |
|---|---|
| `model`, `codename` | 모델명(폴더 이름과 같음)과 코드네임. ADB 모델명·펌웨어 지문·TWRP `ro.product.device`에서 기기를 알아보는 데 씁니다. |
| `deviceinfo_read` | `true`면 DeviceInfo 덤프를 지원합니다. 백업·TWRP·원본 복원 흐름을 실행하므로 `codename`과 `[firmware]`·`[installed]`·`[twrp]`·`[country]`가 필요합니다. `deviceinfo.toml` 없이도 덤프할 수 있습니다. `false`면 DeviceInfo 필드 맵만 둡니다. |
| `deviceinfo_write` | `true`면 DeviceInfo 쓰기(펌웨어 플래싱·DeviceInfo 수정)를 지원하며, 이 기기를 지원 기기로 표시합니다. `deviceinfo_read = true`와 `deviceinfo.toml`이 필요합니다. `false`면 덤프만 지원합니다. |
| `[firmware]` | EDL 로더 이름, 필수 이미지, `region_marker`(펌웨어 vendor_boot DTB에서 리전을 읽는 방식) |
| `[installed]` | 현재 설치된 펌웨어의 허용 범위. `scheme`과 `[[installed.allow]]` 항목 |
| `[twrp]` | 사용할 TWRP Release의 태그·이름·크기·SHA-256·URL |
| `[country]` | 국가코드 변경 방식과 그 위치 |

### 처리 방식 선택

| 항목 | 값 | 동작 |
|---|---|---|
| `[firmware] region_marker` | `region_country` | DTB 최상위 `region_country` 속성 (TB520FU) |
| | `product_region/build_region` | `product_region` 노드의 `build_region` 속성 (TB522FU) |
| `[installed] scheme` | `zui_version` | `ZUI_18.5.10.037_260408_ROW`처럼 계열·버전 번호·리전. `region`, `family`, `max`, `label` |
| | `build_date` | `10.028.260924C`처럼 빌드 날짜와 리전 글자. `region`, `suffix`, `max`(YYYY-MM-DD), `label` |
| `[country] method` | `devinfo_persist` | devinfo·persist 패치 (TB520FU) |
| | `oemowninfo` | 지정한 파티션(`partition`, `lun`, `size`)의 `offset`부터 `field_size` 바이트 필드만 변경. 이 파티션은 작업 전에 원본도 백업합니다. (TB522FU) |

기존 방식을 쓰는 기기는 폴더만 추가하면 됩니다. 새로운 방식이 필요한 기기만 코드에 방식을 추가하고 여기서 고릅니다.

## 기기 추가

1. `devices/<모델명>/` 폴더를 만들고 기존 기기 파일을 복사해 고칩니다.
2. 덤프부터 하려면 `deviceinfo_read = true`, `deviceinfo_write = false`로 두고 `deviceinfo.toml` 없이 시작할 수 있습니다. 필드 맵만 먼저 넣을 때는 둘 다 `false`로 두고 `[firmware]` 등은 넣지 않습니다.
3. 빌드하면 다음을 자동으로 검사합니다.
   - 필드가 0부터 `logical_size`까지 빈틈·겹침 없이 덮는지, 중복 key가 없는지
   - 필수 필드(`magic`, `is_unlocked`, `is_unlock_critical`, `region`, `rollback_index[0..31]`)와 형식별 크기
   - `deviceinfo_read = true`이면 `codename`, `[firmware]`, `[installed]`, `[twrp]`, `[country]`가 모두 있는지, 알 수 없는 항목이나 값이 없는지
   - `deviceinfo_write = true`이면 `deviceinfo_read = true`이고 `deviceinfo.toml`이 있는지
4. TWRP Release의 라이선스 고지를 `crates/lapiswitch-gui/assets/licenses.json`에 추가합니다. 테스트가 모든 기기의 TWRP 고지를 확인합니다.
5. 실제 덤프가 있으면 `LAPISWITCH_DEVICEINFO_<모델명>` 환경 변수에 경로를 지정하고
   `cargo test -p lapiswitch-core -- --ignored real_dump`로 필드 맵을 덤프와 맞춰 볼 수 있습니다.

실제 DeviceInfo 덤프와 기기 고유 값(FRS/UDS 캐시 등)은 이 폴더에 넣지 마세요.
