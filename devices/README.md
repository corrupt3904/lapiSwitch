# 지원 기기

기기마다 폴더 하나를 둡니다. 폴더 이름은 모델명과 같아야 합니다.

```
devices/
  TB520FU/
    device.toml      기기 정보: 모델명, 펌웨어 검사, 설치 펌웨어 허용 범위, TWRP
    deviceinfo.toml  DeviceInfo 필드 맵
```

빌드할 때 `crates/lapiswitch-devices`가 이 폴더를 모두 읽어 프로그램에 넣습니다.
파일이 잘못되면 빌드가 실패하고 어느 기기·필드가 문제인지 알려 줍니다.

## 기기 추가

1. `devices/<모델명>/` 폴더를 만들고 `device.toml`, `deviceinfo.toml`을 넣습니다.
   기존 기기 파일을 복사해 고치는 것이 가장 쉽습니다.
2. 필드 맵만 먼저 넣을 때는 `device.toml`에 `flow = false`를 둡니다.
3. 빌드하면 아래 내용을 자동으로 검사합니다.
   - 필드가 0부터 `logical_size`까지 빈틈·겹침 없이 덮는지
   - 필수 필드(`magic`, `is_unlocked`, `is_unlock_critical`, `region`, `rollback_index[0..31]`)와 그 크기·형식
   - 형식별 크기 (`flag` 1바이트, `uint` 1/2/4/8바이트, `region` 16바이트)
   - `flow = true`이면 `[firmware]`, `[[installed_firmware]]`, `[twrp]`가 모두 있는지
4. 실제 덤프가 있으면 `LAPISWITCH_DEVICEINFO_<모델명>` 환경 변수에 경로를 지정하고
   `cargo test -p lapiswitch-core -- --ignored real_dump`로 필드 맵을 덤프와 맞춰 볼 수 있습니다.

현재 작업 흐름(`flow = true`)은 기기 하나만 연결할 수 있습니다.
기기마다 처리 방식(시작 방식, 펌웨어 리전 판별 등)이 다른 부분은 다음 단계에서 이 파일로 옮깁니다.

실제 DeviceInfo 덤프와 기기 고유 값(FRS/UDS 캐시 등)은 이 폴더에 넣지 마세요.
