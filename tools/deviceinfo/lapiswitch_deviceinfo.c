#define main v4_readonly_reference_main
#include "TB520FU_QSEECOM_READONLY_KM_SHARED_0x1000_V4.c"
#undef main

#define KM_WRITE_KM_DEVICE_STATE 0x203U
#define REGION_OFFSET 0xD10U
#include <sys/file.h>
static int reset_rollback;

struct api {
    void *lib;
    qsee_start_app_v2_fn start;
    qsee_shutdown_app_fn stop;
    qsee_send_cmd_fn send;
    qsee_send_modified_cmd_64_fn modified;
    struct QSEECom_handle *handle;
    unsigned char *ta;
    uint32_t ta_len;
};

static void phase(const char *name)
{
    printf("{\"protocol\":1,\"phase\":\"%s\"}\n", name);
    fflush(stdout);
}

static int save_unique(const char *path, const void *data, size_t size)
{
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
    if (fd < 0) { perror(path); return -1; }
    size_t done = 0;
    while (done < size) {
        ssize_t n = write(fd, (const uint8_t *)data + done, size - done);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) { perror("save_unique"); close(fd); return -1; }
        done += (size_t)n;
    }
    int result = fsync(fd);
    if (close(fd) != 0) result = -1;
    return result;
}

static int save_named(const char *prefix, const char *suffix,
                      const uint8_t *data, size_t size)
{
    char path[1024];
    int n = snprintf(path, sizeof(path), "%s.%s", prefix, suffix);
    if (n < 0 || (size_t)n >= sizeof(path)) return -1;
    return save_unique(path, data, size);
}

static int valid_state(const uint8_t *data)
{
    return memcmp(data, "ANDROID-BOOT!", 13) == 0
        && memcmp(data + 0xCC8, "deadbeef", 8) == 0
        && memcmp(data + 0xD20, "beefdead", 8) == 0
        && data[REGION_OFFSET + 3] == 0
        && (memcmp(data + REGION_OFFSET, "PRC", 3) == 0
            || memcmp(data + REGION_OFFSET, "ROW", 3) == 0);
}

static int validate_plan(const uint8_t *before, const uint8_t *planned)
{
    if (!valid_state(before) || !valid_state(planned)) return -1;
    for (size_t i = 0; i < TB520_KM_SHARED_SIZE; i++) {
        if (reset_rollback && i >= 0x898 && i < 0x998) {
            if (planned[i] != 0) return -1;
        } else if ((i < REGION_OFFSET || i >= REGION_OFFSET + 3) && before[i] != planned[i]) return -1;
    }
    return 0;
}

static int start_session(struct api *api)
{
    int result = api->start(&api->handle, "keymaster", api->ta, api->ta_len, QSEE_SB_SIZE);
    if (result || !api->handle) return -1;
    km_ver_req_t request = { .cmd_id = KM_GET_VERSION };
    km_ver_rsp_t response;
    memset(&response, 0, sizeof(response));
    result = api->send(api->handle, &request, sizeof(request), &response, sizeof(response));
    fprintf(stderr, "GET_VERSION transport=%d status=%d version=%u.%u app=%u.%u\n",
            result, response.status, response.major, response.minor,
            response.app_major, response.app_minor);
    return result == 0 && response.status == 0 && response.major == 5
        && response.minor == 3 && response.app_major == 5 && response.app_minor == 3000 ? 0 : -1;
}

static int request_state(struct api *api, uint32_t cmd, int fd)
{
    km_rw_req_t request;
    km_rw_rsp_t response;
    struct QSEECom_ion_fd_info info;
    memset(&request, 0, sizeof(request));
    memset(&response, 0, sizeof(response));
    memset(&info, 0, sizeof(info));
    for (int i = 0; i < 4; i++) info.data[i].fd = -1;
    request.cmd_id = cmd;
    request.buf_size = TB520_KM_SHARED_SIZE;
    response.rsp_status = INT32_MIN;
    info.data[0].fd = fd;
    info.data[0].cmd_buf_offset = (uint32_t)offsetof(km_rw_req_t, shared_buf_ptr);
    int result = api->modified(api->handle, &request, sizeof(request),
                               &response, sizeof(response), &info);
    fprintf(stderr, "cmd=0x%x transport=%d status=%d unknown=%u rsp_status=%d\n",
            cmd, result, response.status, response.unknown, response.rsp_status);
    return result == 0 && response.status == 0 && response.rsp_status == 0 ? 0 : -1;
}

static int read_fresh(struct api *api, uint8_t *output)
{
    int fd = -1, result = -1;
    void *map = NULL;
    size_t length = 0;
    if (alloc_qsee_ta_buf(TB520_KM_SHARED_SIZE, &fd, &map, &length) < 0) return -1;
    if (request_state(api, KM_READ_KM_DEVICE_STATE, fd) < 0) goto done;
    if (dmabuf_sync(fd, DMA_BUF_SYNC_START | DMA_BUF_SYNC_READ) < 0) goto done;
    memcpy(output, map, TB520_KM_SHARED_SIZE);
    result = dmabuf_sync(fd, DMA_BUF_SYNC_END | DMA_BUF_SYNC_READ);
done:
    munmap(map, length);
    close(fd);
    return result;
}

static int self_test(void)
{
    uint8_t before[TB520_KM_SHARED_SIZE], planned[TB520_KM_SHARED_SIZE];
    for (int mode = 0; mode < 2; mode++) for (int which = 0; which < 2; which++) {
        reset_rollback = mode;
        for (size_t i = 0; i < sizeof(before); i++) before[i] = (uint8_t)(i * 17 + 5);
        memcpy(before, "ANDROID-BOOT!", 13); memcpy(before + 0xCC8, "deadbeef", 8); memcpy(before + 0xD20, "beefdead", 8);
        memset(before + REGION_OFFSET, 0, 16); memcpy(before + REGION_OFFSET, which ? "ROW" : "PRC", 3);
        memcpy(planned, before, sizeof(before)); memcpy(planned + REGION_OFFSET, which ? "PRC" : "ROW", 3);
        if (mode) memset(planned + 0x898, 0, 0x100);
        if (validate_plan(before, planned)) return 1;
        for (size_t i = 0; i < sizeof(before); i++) {
            if (i >= REGION_OFFSET && i < REGION_OFFSET + 3) continue;
            planned[i] ^= 1; if (validate_plan(before, planned) == 0) return 2; planned[i] ^= 1;
        }
        memcpy(planned + REGION_OFFSET, before + REGION_OFFSET, 3);
        if (validate_plan(before, planned)) return 3;
    }
    phase("SELF_TEST_PASSED_NO_QSEE_ACCESS"); return 0;
}

int main(int argc, char **argv)
{
    if (argc == 2 && strcmp(argv[1], "--self-test") == 0) return self_test();
    const char *expected_path = NULL, *planned_path = NULL, *prefix = NULL;
    const char *ta_path = DEFAULT_TA;
    int apply = 0, read_only = 0;
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--expected") && i+1 < argc) expected_path = argv[++i];
        else if (!strcmp(argv[i], "--planned") && i+1 < argc) planned_path = argv[++i];
        else if (!strcmp(argv[i], "--out-prefix") && i+1 < argc) prefix = argv[++i];
        else if (!strcmp(argv[i], "--ta") && i+1 < argc) ta_path = argv[++i];
        else if (!strcmp(argv[i], "--apply-once")) apply = 1;
        else if (!strcmp(argv[i], "--read")) read_only = 1;
        else if (!strcmp(argv[i], "--reset-rollback")) reset_rollback = 1;
        else return 2;
    }
    if (!prefix || (read_only && apply) || (!read_only && (!expected_path || !planned_path))) return 2;
    int lock_fd = open("/tmp/lapiswitch-deviceinfo.lock", O_WRONLY | O_CREAT | O_CLOEXEC | O_NOFOLLOW, 0600);
    if (lock_fd < 0 || flock(lock_fd, LOCK_EX | LOCK_NB) != 0) { if (lock_fd >= 0) close(lock_fd); return 3; }
    unsigned char *expected = NULL, *planned = NULL;
    uint32_t expected_len = 0, planned_len = 0;
    struct api api;
    memset(&api, 0, sizeof(api));
    int result = 1, write_result = -1;
    int data_fd = -1;
    void *data_map = NULL;
    size_t data_alloc = 0;
    uint8_t live[TB520_KM_SHARED_SIZE], after[TB520_KM_SHARED_SIZE];
    if (!read_only) {
    if (read_entire_file(expected_path, &expected, &expected_len) < 0
        || read_entire_file(planned_path, &planned, &planned_len) < 0) goto done;
    if (expected_len != TB520_KM_SHARED_SIZE || planned_len != TB520_KM_SHARED_SIZE
        || validate_plan(expected, planned) < 0) {
        fprintf(stderr, "Refusing an invalid or out-of-region write plan.\n");
        goto done;
    }
    phase("PLAN_VALIDATED");
    }
    if (read_entire_file(ta_path, &api.ta, &api.ta_len) < 0) goto done;
    api.lib = dlopen(QSEE_LIB, RTLD_NOW | RTLD_LOCAL);
    if (!api.lib) goto done;
    api.start = (qsee_start_app_v2_fn)dlsym(api.lib, "QSEECom_start_app_V2");
    api.stop = (qsee_shutdown_app_fn)dlsym(api.lib, "QSEECom_shutdown_app");
    api.send = (qsee_send_cmd_fn)dlsym(api.lib, "QSEECom_send_cmd");
    api.modified = (qsee_send_modified_cmd_64_fn)dlsym(api.lib, "QSEECom_send_modified_cmd_64");
    if (!api.start || !api.stop || !api.send || !api.modified) goto done;
    if (start_session(&api) < 0 || read_fresh(&api, live) < 0) goto done;
    if (save_named(prefix, "before.bin", live, sizeof(live)) < 0) goto done;
    if (read_only) { phase("READ_COMPLETE"); result = 0; goto done; }
    if (memcmp(live, expected, sizeof(live)) != 0) {
        fprintf(stderr, "Current state differs from the PC backup. No write issued.\n");
        goto done;
    }
    phase("CURRENT_STATE_MATCHES_PC_BACKUP");
    if (!apply) { phase("DRY_RUN_VERIFIED_NO_WRITE"); result = 0; goto done; }
    if (alloc_qsee_ta_buf(TB520_KM_SHARED_SIZE, &data_fd, &data_map, &data_alloc) < 0) goto done;
    if (dmabuf_sync(data_fd, DMA_BUF_SYNC_START | DMA_BUF_SYNC_RW) < 0) goto done;
    memcpy(data_map, live, sizeof(live));
    memcpy((uint8_t *)data_map + REGION_OFFSET, planned + REGION_OFFSET, 3);
    if (reset_rollback) memset((uint8_t *)data_map + 0x898, 0, 0x100);
    if (memcmp(data_map, planned, sizeof(live)) != 0) {
        (void)dmabuf_sync(data_fd, DMA_BUF_SYNC_END | DMA_BUF_SYNC_RW);
        goto done;
    }
    if (dmabuf_sync(data_fd, DMA_BUF_SYNC_END | DMA_BUF_SYNC_RW) < 0) goto done;
    static const char marker[] = "0x203 will be sent exactly once. Do not automatically retry.\n";
    if (save_named(prefix, "write-attempted.txt", (const uint8_t *)marker, sizeof(marker)-1) < 0) goto done;
    phase("WRITE_REQUEST_BEGIN");
    write_result = request_state(&api, KM_WRITE_KM_DEVICE_STATE, data_fd);
    phase(write_result == 0 ? "WRITE_RESPONSE_OK" : "WRITE_RESPONSE_ERROR_READBACK_REQUIRED");
    result = 40; // An attempt has occurred: no automatic rollback or retry.
    if (read_fresh(&api, after) < 0) goto done;
    if (save_named(prefix, "after_same_session.bin", after, sizeof(after)) < 0) goto done;
    if (memcmp(after, planned, sizeof(after)) != 0) {
        phase("READBACK_MISMATCH");
        goto done;
    }
    phase("SAME_SESSION_READBACK_MATCHES_ALL_4096_BYTES");
    if (api.stop(&api.handle) != 0) goto done;
    if (start_session(&api) < 0 || read_fresh(&api, after) < 0) goto done;
    if (save_named(prefix, "after_new_session.bin", after, sizeof(after)) < 0) goto done;
    if (memcmp(after, planned, sizeof(after)) != 0) {
        phase("NEW_SESSION_READBACK_MISMATCH");
        goto done;
    }
    phase("NEW_SESSION_READBACK_MATCHES_ALL_4096_BYTES");
    if (write_result == 0) result = 0;
done:
    if (data_map) munmap(data_map, data_alloc);
    if (data_fd >= 0) close(data_fd);
    if (api.handle && api.stop) {
        int stop_result = api.stop(&api.handle);
        fprintf(stderr, "QSEECom_shutdown_app=%d\n", stop_result);
    }
    if (api.lib) dlclose(api.lib);
    free(api.ta);
    free(expected);
    free(planned);
    close(lock_fd);
    return result;
}
