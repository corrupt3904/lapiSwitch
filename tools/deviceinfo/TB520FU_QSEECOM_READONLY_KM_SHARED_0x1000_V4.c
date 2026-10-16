#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <linux/fs.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

#define TB520_DEVICEINFO_SIZE 0xEA8U
#define TB520_KM_SHARED_SIZE    0x1000U
#define QSEE_SB_SIZE          0x1000U
#define QSEE_HEAP             "/dev/dma_heap/qcom,qseecom-ta"
#define QSEE_LIB              "/vendor/lib64/libQSEEComAPI.so"
#define DEFAULT_TA            "/dev/block/bootdevice/by-name/keymaster_a"
#define DEFAULT_OUT           "/tmp/deviceinfo_0xea8.bin"

#define KM_GET_VERSION            0x200U
#define KM_READ_KM_DEVICE_STATE   0x202U

/* Linux dma-heap UAPI, kept local so Android NDK headers are not required. */
struct dma_heap_allocation_data {
    uint64_t len;
    uint32_t fd;
    uint32_t fd_flags;
    uint64_t heap_flags;
};
#define DMA_HEAP_IOC_MAGIC 'H'
#define DMA_HEAP_IOCTL_ALLOC _IOWR(DMA_HEAP_IOC_MAGIC, 0x0, struct dma_heap_allocation_data)

/* dma-buf CPU synchronization UAPI. */
struct dma_buf_sync { uint64_t flags; };
#define DMA_BUF_BASE 'b'
#define DMA_BUF_IOCTL_SYNC _IOW(DMA_BUF_BASE, 0, struct dma_buf_sync)
#define DMA_BUF_SYNC_READ   (1ULL << 0)
#define DMA_BUF_SYNC_WRITE  (2ULL << 0)
#define DMA_BUF_SYNC_RW     (DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE)
#define DMA_BUF_SYNC_END    (1ULL << 2)
#define DMA_BUF_SYNC_START  (0ULL << 2)

struct QSEECom_handle {
    unsigned char *ion_sbuffer;
};
struct QSEECom_ion_fd_data {
    int32_t fd;
    uint32_t cmd_buf_offset;
};
struct QSEECom_ion_fd_info {
    struct QSEECom_ion_fd_data data[4];
};

typedef int (*qsee_start_app_v2_fn)(struct QSEECom_handle **,
                                    const char *, unsigned char *,
                                    uint32_t, uint32_t);
typedef int (*qsee_shutdown_app_fn)(struct QSEECom_handle **);
typedef int (*qsee_send_cmd_fn)(struct QSEECom_handle *, void *, uint32_t,
                                void *, uint32_t);
typedef int (*qsee_send_modified_cmd_64_fn)(struct QSEECom_handle *,
                                             void *, uint32_t,
                                             void *, uint32_t,
                                             struct QSEECom_ion_fd_info *);

typedef struct {
    uint32_t cmd_id;
} __attribute__((packed)) km_ver_req_t;

typedef struct {
    int32_t status;
    uint32_t major;
    uint32_t minor;
    uint32_t app_major;
    uint32_t app_minor;
} __attribute__((packed)) km_ver_rsp_t;

typedef struct {
    uint32_t cmd_id;
    uint64_t shared_buf_ptr;
    uint32_t buf_size;
} __attribute__((packed)) km_rw_req_t;

typedef struct {
    int32_t status;
    uint32_t unknown;
    int32_t rsp_status;
} __attribute__((packed)) km_rw_rsp_t;

_Static_assert(sizeof(km_rw_req_t) == 16, "km_rw_req_t must be 16 bytes");
_Static_assert(sizeof(km_ver_rsp_t) == 20, "km_ver_rsp_t must be 20 bytes");

static int dmabuf_sync(int fd, uint64_t flags)
{
    struct dma_buf_sync s = { .flags = flags };
    if (ioctl(fd, DMA_BUF_IOCTL_SYNC, &s) < 0) {
        fprintf(stderr, "[-] DMA_BUF_IOCTL_SYNC flags=0x%" PRIx64 ": %s\n",
                flags, strerror(errno));
        return -1;
    }
    return 0;
}

static int alloc_qsee_ta_buf(size_t logical, int *out_fd, void **out_map, size_t *out_len)
{
    int heapfd = -1, fd = -1;
    void *map = MAP_FAILED;
    long pagesz = sysconf(_SC_PAGESIZE);
    size_t len = (logical + (size_t)pagesz - 1) & ~((size_t)pagesz - 1);
    struct dma_heap_allocation_data a;

    *out_fd = -1;
    *out_map = NULL;
    *out_len = 0;

    heapfd = open(QSEE_HEAP, O_RDONLY | O_CLOEXEC);
    if (heapfd < 0) {
        fprintf(stderr, "[-] open %s: %s\n", QSEE_HEAP, strerror(errno));
        return -1;
    }

    memset(&a, 0, sizeof(a));
    a.len = len;
    a.fd_flags = O_RDWR | O_CLOEXEC;
    if (ioctl(heapfd, DMA_HEAP_IOCTL_ALLOC, &a) < 0) {
        fprintf(stderr, "[-] DMA_HEAP_IOCTL_ALLOC(%s, 0x%zx): %s\n",
                QSEE_HEAP, len, strerror(errno));
        close(heapfd);
        return -1;
    }
    close(heapfd);
    fd = (int)a.fd;

    map = mmap(NULL, len, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (map == MAP_FAILED) {
        fprintf(stderr, "[-] mmap qseecom-ta dma-buf: %s\n", strerror(errno));
        close(fd);
        return -1;
    }

    if (dmabuf_sync(fd, DMA_BUF_SYNC_START | DMA_BUF_SYNC_RW) < 0)
        goto fail;
    memset(map, 0, len);
    if (dmabuf_sync(fd, DMA_BUF_SYNC_END | DMA_BUF_SYNC_RW) < 0)
        goto fail;

    *out_fd = fd;
    *out_map = map;
    *out_len = len;
    return 0;

fail:
    munmap(map, len);
    close(fd);
    return -1;
}

static int get_file_size(int fd, uint64_t *out)
{
    struct stat st;
    *out = 0;
    if (ioctl(fd, BLKGETSIZE64, out) == 0 && *out)
        return 0;
    if (fstat(fd, &st) == 0 && st.st_size > 0) {
        *out = (uint64_t)st.st_size;
        return 0;
    }
    return -1;
}

static int read_entire_file(const char *path, unsigned char **out, uint32_t *out_len)
{
    int fd = -1;
    uint64_t sz64 = 0;
    size_t done = 0;
    unsigned char *p = NULL;

    *out = NULL;
    *out_len = 0;
    fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) {
        fprintf(stderr, "[-] open TA %s: %s\n", path, strerror(errno));
        return -1;
    }
    if (get_file_size(fd, &sz64) < 0 || sz64 == 0 || sz64 > 64U * 1024U * 1024U) {
        fprintf(stderr, "[-] unreasonable/unknown TA size for %s: 0x%" PRIx64 "\n", path, sz64);
        close(fd);
        return -1;
    }
    p = (unsigned char *)malloc((size_t)sz64);
    if (!p) {
        close(fd);
        return -1;
    }
    while (done < (size_t)sz64) {
        ssize_t n = pread(fd, p + done, (size_t)sz64 - done, (off_t)done);
        if (n < 0) {
            if (errno == EINTR) continue;
            fprintf(stderr, "[-] read TA: %s\n", strerror(errno));
            free(p); close(fd); return -1;
        }
        if (n == 0) break;
        done += (size_t)n;
    }
    close(fd);
    if (done == 0 || done > UINT32_MAX) {
        free(p);
        return -1;
    }
    *out = p;
    *out_len = (uint32_t)done;
    return 0;
}

static int save_blob(const char *path, const void *buf, size_t len)
{
    int fd = open(path, O_CREAT | O_TRUNC | O_WRONLY | O_CLOEXEC, 0600);
    size_t done = 0;
    if (fd < 0) {
        fprintf(stderr, "[-] open output %s: %s\n", path, strerror(errno));
        return -1;
    }
    while (done < len) {
        ssize_t n = write(fd, (const uint8_t *)buf + done, len - done);
        if (n < 0) {
            if (errno == EINTR) continue;
            fprintf(stderr, "[-] write output: %s\n", strerror(errno));
            close(fd); return -1;
        }
        done += (size_t)n;
    }
    close(fd);
    return 0;
}

static void hexdump(const uint8_t *p, size_t len)
{
    for (size_t off = 0; off < len; off += 16) {
        printf("%04zx  ", off);
        for (size_t i = 0; i < 16; i++) {
            if (off + i < len) printf("%02x ", p[off + i]);
            else printf("   ");
            if (i == 7) putchar(' ');
        }
        printf(" |");
        for (size_t i = 0; i < 16 && off + i < len; i++) {
            unsigned char c = p[off + i];
            putchar((c >= 0x20 && c <= 0x7e) ? c : '.');
        }
        printf("|\n");
    }
}

static void preview_ascii(const char *name, const uint8_t *p, size_t off, size_t n)
{
    printf("%-16s @ 0x%03zx: ", name, off);
    for (size_t i = 0; i < n; i++) {
        unsigned char c = p[off + i];
        putchar((c >= 0x20 && c <= 0x7e) ? c : '.');
    }
    putchar('\n');
}

static void usage(const char *argv0)
{
    fprintf(stderr,
        "Usage: %s [--ta PATH] [-o FILE]\n\n"
        "READ-ONLY TB520FU DeviceInfo dumper using stock libQSEEComAPI.so.\n"
        "Commands implemented: 0x200 GET_VERSION, 0x202 READ_KM_DEVICE_STATE.\n"
        "No write command is implemented.\n\n"
        "Defaults:\n"
        "  --ta  %s\n"
        "  -o    %s\n", argv0, DEFAULT_TA, DEFAULT_OUT);
}

int main(int argc, char **argv)
{
    const char *ta_path = DEFAULT_TA;
    const char *out_path = DEFAULT_OUT;
    void *lib = NULL;
    qsee_start_app_v2_fn start_v2 = NULL;
    qsee_shutdown_app_fn shutdown_app = NULL;
    qsee_send_cmd_fn send_cmd = NULL;
    qsee_send_modified_cmd_64_fn send_mod64 = NULL;
    struct QSEECom_handle *h = NULL;
    unsigned char *ta = NULL;
    uint32_t ta_len = 0;
    int data_fd = -1;
    void *data_map = NULL;
    size_t data_alloc = 0;
    int rc = 1;

    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--ta") && i + 1 < argc) ta_path = argv[++i];
        else if (!strcmp(argv[i], "-o") && i + 1 < argc) out_path = argv[++i];
        else if (!strcmp(argv[i], "-h") || !strcmp(argv[i], "--help")) {
            usage(argv[0]); return 0;
        } else {
            usage(argv[0]); return 2;
        }
    }

    fprintf(stderr, "[i] TB520FU read-only QSEECom checker V4 (full 0x1000 dump)\n");
    fprintf(stderr, "[i] logical DeviceInfo size: 0x%X (%u)\n",
            TB520_DEVICEINFO_SIZE, TB520_DEVICEINFO_SIZE);
    fprintf(stderr, "[i] KeyMint 0x202 shared-buffer size: 0x%X (%u)\n",
            TB520_KM_SHARED_SIZE, TB520_KM_SHARED_SIZE);
    fprintf(stderr, "[i] TA: %s\n", ta_path);
    fprintf(stderr, "[i] DMA heap: %s\n", QSEE_HEAP);
    fprintf(stderr, "[i] only commands 0x200 and 0x202 are used\n");

    if (read_entire_file(ta_path, &ta, &ta_len) < 0)
        goto out;
    fprintf(stderr, "[+] read TA image: 0x%x bytes\n", ta_len);

    lib = dlopen(QSEE_LIB, RTLD_NOW | RTLD_LOCAL);
    if (!lib) {
        fprintf(stderr, "[-] dlopen %s: %s\n", QSEE_LIB, dlerror());
        goto out;
    }
    start_v2 = (qsee_start_app_v2_fn)dlsym(lib, "QSEECom_start_app_V2");
    shutdown_app = (qsee_shutdown_app_fn)dlsym(lib, "QSEECom_shutdown_app");
    send_cmd = (qsee_send_cmd_fn)dlsym(lib, "QSEECom_send_cmd");
    send_mod64 = (qsee_send_modified_cmd_64_fn)dlsym(lib, "QSEECom_send_modified_cmd_64");
    if (!start_v2 || !shutdown_app || !send_cmd || !send_mod64) {
        fprintf(stderr, "[-] missing QSEECom symbol(s)\n");
        goto out;
    }

    {
        int ret = start_v2(&h, "keymaster", ta, ta_len, QSEE_SB_SIZE);
        fprintf(stderr, "[i] QSEECom_start_app_V2 ret=%d handle=%p\n", ret, (void *)h);
        if (ret != 0 || !h) {
            fprintf(stderr, "[-] could not open/load keymaster through QSEECom V2\n");
            goto out;
        }
    }

    /* GET_VERSION first, with exact request/response lengths. */
    {
        km_ver_req_t req = { .cmd_id = KM_GET_VERSION };
        km_ver_rsp_t rsp;
        memset(&rsp, 0, sizeof(rsp));
        int ret = send_cmd(h, &req, sizeof(req), &rsp, sizeof(rsp));
        fprintf(stderr,
                "[+] GET_VERSION transport=%d status=%d version=%u.%u app=%u.%u\n",
                ret, rsp.status, rsp.major, rsp.minor, rsp.app_major, rsp.app_minor);
        if (ret != 0 || rsp.status != 0) {
            fprintf(stderr, "[-] GET_VERSION failed; refusing to issue 0x202\n");
            goto out;
        }
    }

    if (alloc_qsee_ta_buf(TB520_KM_SHARED_SIZE,
                          &data_fd, &data_map, &data_alloc) < 0)
        goto out;
    fprintf(stderr, "[+] qseecom-ta dma-buf fd=%d map=%p alloc=0x%zx\n",
            data_fd, data_map, data_alloc);

    /* READ_KM_DEVICE_STATE: QSEECom patches shared_buf_ptr at byte offset 4
       from the dma-buf FD before the command reaches the secure app.
       TB520 KeyMint requires a 0x1000-byte shared buffer for command 0x202;
       the Lenovo DeviceInfo payload itself is the first 0xEA8 bytes. */
    {
        km_rw_req_t req;
        km_rw_rsp_t rsp;
        struct QSEECom_ion_fd_info info;
        memset(&req, 0, sizeof(req));
        memset(&rsp, 0, sizeof(rsp));
        memset(&info, 0, sizeof(info));
        for (int i = 0; i < 4; i++) info.data[i].fd = -1;

        req.cmd_id = KM_READ_KM_DEVICE_STATE;
        req.shared_buf_ptr = 0; /* patched by QSEECom modified-command path */
        req.buf_size = TB520_KM_SHARED_SIZE;

        info.data[0].fd = data_fd;
        info.data[0].cmd_buf_offset = (uint32_t)offsetof(km_rw_req_t, shared_buf_ptr);

        int ret = send_mod64(h, &req, sizeof(req), &rsp, sizeof(rsp), &info);
        fprintf(stderr,
                "[+] READ_KM_DEVICE_STATE transport=%d status=%d unknown=%u rsp_status=%d\n",
                ret, rsp.status, rsp.unknown, rsp.rsp_status);
        if (ret != 0 || rsp.status != 0 || rsp.rsp_status != 0) {
            fprintf(stderr, "[-] 0x202 did not return a successful DeviceInfo read\n");
            goto out;
        }
    }

    if (dmabuf_sync(data_fd, DMA_BUF_SYNC_START | DMA_BUF_SYNC_READ) < 0)
        goto out;

    if (save_blob(out_path, data_map, TB520_KM_SHARED_SIZE) < 0)
        goto out_sync;
    fprintf(stderr, "[+] saved full shared buffer: 0x%X bytes to %s\n",
            TB520_KM_SHARED_SIZE, out_path);

    preview_ascii("region_magic_1", (const uint8_t *)data_map, 0xCC8, 8);
    preview_ascii("region",         (const uint8_t *)data_map, 0xD10, 3);
    preview_ascii("region_magic_2", (const uint8_t *)data_map, 0xD20, 8);

    printf("\n===== KM_SHARED_BUFFER_BEGIN size=0x%X (%u bytes) =====\n",
           TB520_KM_SHARED_SIZE, TB520_KM_SHARED_SIZE);
    printf("[layout] logical Lenovo DeviceInfo: 0x0000..0x%04X (%u bytes)\n",
           TB520_DEVICEINFO_SIZE - 1, TB520_DEVICEINFO_SIZE);
    printf("[layout] extra shared-buffer tail : 0x%04X..0x%04X (%u bytes)\n",
           TB520_DEVICEINFO_SIZE, TB520_KM_SHARED_SIZE - 1,
           TB520_KM_SHARED_SIZE - TB520_DEVICEINFO_SIZE);
    hexdump((const uint8_t *)data_map, TB520_KM_SHARED_SIZE);
    printf("===== KM_SHARED_BUFFER_END size=0x%X (%u bytes) =====\n",
           TB520_KM_SHARED_SIZE, TB520_KM_SHARED_SIZE);
    fflush(stdout);
    rc = 0;

out_sync:
    (void)dmabuf_sync(data_fd, DMA_BUF_SYNC_END | DMA_BUF_SYNC_READ);
out:
    if (data_map) munmap(data_map, data_alloc);
    if (data_fd >= 0) close(data_fd);
    if (h && shutdown_app) {
        int sret = shutdown_app(&h);
        fprintf(stderr, "[i] QSEECom_shutdown_app ret=%d\n", sret);
    }
    if (lib) dlclose(lib);
    free(ta);
    return rc;
}
