#ifndef UI_SERVICE_H
#define UI_SERVICE_H

#include <stdbool.h>
#include <stdint.h>

/*
 * UIService.framework public NXU ABI.
 *
 * Applications do not include this header. NXU owns devices and memory;
 * UIService receives a versioned host table containing borrowed surfaces,
 * presentation callbacks and normalized input events.
 */

#define UI_SERVICE_API_VERSION 1U

#define UI_SERVICE_BUTTON_IDLE 0U
#define UI_SERVICE_BUTTON_HOVERED 1U
#define UI_SERVICE_BUTTON_PRESSED 2U
#define UI_SERVICE_BUTTON_DISABLED 3U

#define UI_SERVICE_ABI_VERSION_V1 1U
#define UI_SERVICE_ABI_VERSION_V2 2U
#define UI_SERVICE_ABI_VERSION_V3 3U
#define UI_SERVICE_ABI_VERSION_V4 4U
#define UI_SERVICE_ABI_VERSION_V5 5U
#define UI_SERVICE_ABI_VERSION UI_SERVICE_ABI_VERSION_V5

#define UI_SERVICE_PIXEL_FORMAT_XRGB8888 1U

#define UI_SERVICE_HOST_CAP_PRESENT (1ULL << 0U)
#define UI_SERVICE_HOST_CAP_DAMAGE (1ULL << 1U)
#define UI_SERVICE_HOST_CAP_INPUT (1ULL << 2U)
#define UI_SERVICE_HOST_CAP_TIME (1ULL << 3U)
#define UI_SERVICE_HOST_CAP_FS (1ULL << 4U)
/* The host sends UI_SERVICE_EVENT_KEY_DOWN events (optional, any input host). */
#define UI_SERVICE_HOST_CAP_KEYBOARD (1ULL << 5U)
/* The host fills in UIServiceActivity through get_activity (optional, v5). */
#define UI_SERVICE_HOST_CAP_ACTIVITY (1ULL << 6U)
#define UI_SERVICE_HOST_CAPABILITIES_V1 (UI_SERVICE_HOST_CAP_PRESENT | UI_SERVICE_HOST_CAP_DAMAGE)
#define UI_SERVICE_HOST_CAPABILITIES_V2 (UI_SERVICE_HOST_CAPABILITIES_V1 | UI_SERVICE_HOST_CAP_INPUT)
#define UI_SERVICE_HOST_CAPABILITIES_V3 (UI_SERVICE_HOST_CAPABILITIES_V2 | UI_SERVICE_HOST_CAP_TIME)
#define UI_SERVICE_HOST_CAPABILITIES_V4 UI_SERVICE_HOST_CAPABILITIES_V3
/* V5's filesystem capability is optional even on a v5 host (About sevOS has
 * no use for it), so it is not folded into a "V5 requires FS" default set;
 * a host adds UI_SERVICE_HOST_CAP_FS itself when it actually implements
 * list_directory. */

#define UI_SERVICE_STATUS_OK 0U
#define UI_SERVICE_STATUS_INVALID_ARGUMENT 1U
#define UI_SERVICE_STATUS_BAD_VERSION 2U
#define UI_SERVICE_STATUS_UNSUPPORTED 3U
#define UI_SERVICE_STATUS_NO_SURFACE 4U
#define UI_SERVICE_STATUS_PRESENT_FAILED 5U

#define UI_SERVICE_EVENT_NONE 0U
#define UI_SERVICE_EVENT_POINTER_MOVED 1U
#define UI_SERVICE_EVENT_POINTER_DOWN 2U
#define UI_SERVICE_EVENT_POINTER_UP 3U
/* The scroll wheel (or a trackpad's scroll) turned: `reserved` holds the signed
 * number of notches (an int32_t, positive = wheel up), `x`/`y` the pointer. */
#define UI_SERVICE_EVENT_SCROLL 4U
/* A key went down or auto-repeats: `button` holds the evdev key code,
 * `reserved` the character it types (a Unicode scalar, 0 for none),
 * `x`/`y` the pointer. Only with UI_SERVICE_HOST_CAP_KEYBOARD. */
#define UI_SERVICE_EVENT_KEY_DOWN 5U

#define UI_SERVICE_POINTER_BUTTON_NONE 0U
#define UI_SERVICE_POINTER_BUTTON_PRIMARY 1U
#define UI_SERVICE_POINTER_BUTTON_SECONDARY 2U
#define UI_SERVICE_POINTER_BUTTON_MIDDLE 3U

typedef struct {
    uint32_t struct_size;
    uint32_t abi_version;
} UIServiceABIHeader;

typedef struct {
    int32_t x;
    int32_t y;
    uint32_t width;
    uint32_t height;
} UIServiceDamageRect;

typedef struct {
    uint32_t *pixels;
    uint32_t width;
    uint32_t height;
    uint32_t stride_pixels;
    uint32_t pixel_format;
} UIServiceSurfaceDescriptor;

typedef struct {
    uint32_t event_type;
    uint32_t struct_size;
    int32_t x;
    int32_t y;
    uint32_t button;
    /* Zero for every pointer event; the signed scroll amount, in notches, for
     * UI_SERVICE_EVENT_SCROLL (an int32_t stored as its uint32_t bit pattern,
     * so the struct's size and layout are unchanged). */
    uint32_t reserved;
} UIServiceHostEvent;

typedef uint32_t (*UIServiceGetSurfaceFn)(void *context, UIServiceSurfaceDescriptor *surface);
typedef uint32_t (*UIServicePresentFn)(void *context, const UIServiceDamageRect *damage);
typedef uint32_t (*UIServicePollEventFn)(void *context, UIServiceHostEvent *event);
/* Seconds since 1970-01-01 00:00:00 UTC, written through *unix_seconds. */
typedef uint32_t (*UIServiceGetTimeFn)(void *context, uint64_t *unix_seconds);

#define UI_SERVICE_FS_NAME_MAX 63U

#define UI_SERVICE_FS_KIND_REGULAR 0U
#define UI_SERVICE_FS_KIND_DIRECTORY 1U
#define UI_SERVICE_FS_KIND_OTHER 2U

#define UI_SERVICE_FS_STATUS_OK 0U
#define UI_SERVICE_FS_STATUS_NOT_FOUND 1U
#define UI_SERVICE_FS_STATUS_NOT_A_DIRECTORY 2U
#define UI_SERVICE_FS_STATUS_ERROR 3U

/*
 * One directory entry. `name` is NUL-terminated and truncated to
 * UI_SERVICE_FS_NAME_MAX bytes if the real name is longer; `name_length` is
 * its length before any truncation/NUL. `kind` is one of
 * UI_SERVICE_FS_KIND_*. `size_bytes` is 0 for anything but a regular file.
 *
 * This layout is mirrored bit-for-bit by `ui_core::fs::DirEntry` on the
 * Rust side (`#[repr(C)]`, same field order/types) so the host can write
 * directly into an app-provided buffer with no intermediate copy -- keep
 * the two in sync if this ever changes.
 */
typedef struct {
    uint32_t name_length;
    char name[UI_SERVICE_FS_NAME_MAX + 1U];
    uint32_t kind;
    uint64_t size_bytes;
} UIServiceDirEntry;

/*
 * List up to `capacity` entries of the directory at `path` (an absolute
 * path, `path_len` bytes, not necessarily NUL-terminated) into `entries`.
 * `*count_out` receives the number actually written (<= capacity).
 * `*truncated_out` is set true when the directory holds more entries than
 * `capacity` allowed -- the first `*count_out` are still valid, just not
 * the whole directory. Returns a UI_SERVICE_FS_STATUS_* code.
 */
typedef uint32_t (*UIServiceListDirectoryFn)(
    void *context,
    const char *path,
    uint32_t path_len,
    UIServiceDirEntry *entries,
    uint32_t capacity,
    uint32_t *count_out,
    bool *truncated_out
);

/*
 * What Activity Monitor shows: the machine's CPUs and memory, and one
 * UIServiceProcessInfo per live process. Mirrored bit-for-bit by
 * `ui_core::activity` (repr(C), same order). Every 64-bit field sits at a
 * multiple of 8 so the layout is the same on i386, where a C uint64_t in a
 * struct is only 4-aligned.
 *
 * CPU time is counted in scheduler ticks, not seconds: a process's share of
 * the machine is how far its cpu_ticks moved against one CPU's `ticks` over
 * the same interval (100% = one whole CPU), which needs no tick rate.
 */
#define UI_SERVICE_ACTIVITY_NAME_MAX 31U
#define UI_SERVICE_ACTIVITY_CPU_MAX 8U

/* UIServiceProcessInfo.state */
#define UI_SERVICE_PROCESS_STATE_RUNNING 0U   /* one of its threads is on a CPU */
#define UI_SERVICE_PROCESS_STATE_RUNNABLE 1U  /* ready, waiting for a CPU */
#define UI_SERVICE_PROCESS_STATE_SLEEPING 2U  /* every thread is waiting */
#define UI_SERVICE_PROCESS_STATE_STOPPED 3U
#define UI_SERVICE_PROCESS_STATE_OTHER 4U     /* being created or exiting */

/* UIServiceProcessInfo.flags */
#define UI_SERVICE_PROCESS_FLAG_KERNEL (1U << 0U)
#define UI_SERVICE_PROCESS_FLAG_SYSTEM (1U << 1U)

typedef struct {
    uint64_t uniqueid;
    /* Scheduler ticks its live threads have run, all CPUs together. */
    uint64_t cpu_ticks;
    uint32_t pid;
    uint32_t ppid;
    uint32_t state;
    uint32_t flags;
    uint32_t threads;
    /* The highest-priority (lowest-numbered) MLFQ level of its threads. */
    uint32_t mlfq_level;
    /* The CPU its most recently scheduled thread ran on. */
    uint32_t last_cpu;
    uint32_t name_length;
    char name[UI_SERVICE_ACTIVITY_NAME_MAX + 1U];
} UIServiceProcessInfo;

typedef struct {
    /* Scheduler ticks this CPU took, and how many found it running something. */
    uint64_t ticks;
    uint64_t busy_ticks;
    uint64_t context_switches;
    uint32_t online;
    uint32_t reserved;
} UIServiceCpuInfo;

typedef struct {
    uint32_t struct_size;
    uint32_t cpu_count;
    uint64_t uptime_us;
    uint64_t page_size;
    uint64_t total_pages;
    uint64_t free_pages;
    uint64_t heap_pages;
    uint32_t process_count;
    uint32_t thread_count;
    UIServiceCpuInfo cpus[UI_SERVICE_ACTIVITY_CPU_MAX];
} UIServiceActivity;

/*
 * Fill `activity` and up to `capacity` entries of `processes`; `*count_out`
 * receives how many were written (activity->process_count is the total,
 * which can be larger). Returns a UI_SERVICE_STATUS_* code.
 */
typedef uint32_t (*UIServiceGetActivityFn)(
    void *context,
    UIServiceActivity *activity,
    UIServiceProcessInfo *processes,
    uint32_t capacity,
    uint32_t *count_out
);

typedef struct {
    UIServiceABIHeader header;
    uint64_t capabilities;
    void *context;
    UIServiceGetSurfaceFn get_surface;
    UIServicePresentFn present;
} UIServiceHostV1;

typedef struct {
    UIServiceABIHeader header;
    uint64_t capabilities;
    void *context;
    UIServiceGetSurfaceFn get_surface;
    UIServicePresentFn present;
    UIServicePollEventFn poll_event;
} UIServiceHostV2;

typedef struct {
    UIServiceABIHeader header;
    uint64_t capabilities;
    void *context;
    UIServiceGetSurfaceFn get_surface;
    UIServicePresentFn present;
    UIServicePollEventFn poll_event;
    UIServiceGetTimeFn get_time;
} UIServiceHostV3;

typedef struct {
    UIServiceABIHeader header;
    uint64_t capabilities;
    void *context;
    UIServiceGetSurfaceFn get_surface;
    UIServicePresentFn present;
    UIServicePollEventFn poll_event;
    UIServiceGetTimeFn get_time;
    /*
     * Physical pixels per design point, as thousandths (e.g. 2000 for a
     * genuine 2x-Retina host, 1000 for a 1x/non-Retina one). 0 means
     * "unknown"; UIService keeps its historical 2x default in that case.
     * Lets UIService size window chrome, fonts and control metrics for the
     * host's actual density instead of assuming every canvas is 2x.
     */
    uint32_t content_scale_permille;
} UIServiceHostV4;

typedef struct {
    UIServiceABIHeader header;
    uint64_t capabilities;
    void *context;
    UIServiceGetSurfaceFn get_surface;
    UIServicePresentFn present;
    UIServicePollEventFn poll_event;
    UIServiceGetTimeFn get_time;
    uint32_t content_scale_permille;
    /*
     * Read-only directory listing. NULL, with UI_SERVICE_HOST_CAP_FS unset,
     * on a host that has no filesystem to offer (or nothing worth exposing
     * yet) -- an app that needs it treats that as "unavailable", not an
     * error, and falls back to whatever static content it shipped with.
     */
    UIServiceListDirectoryFn list_directory;
    /*
     * Processes, CPUs and memory for Activity Monitor. NULL, with
     * UI_SERVICE_HOST_CAP_ACTIVITY unset, on a host with nothing to report.
     */
    UIServiceGetActivityFn get_activity;
} UIServiceHostV5;

/*
 * The login screen's own host table, next to (not inside) the app host: only
 * UIServiceRunLogin sees these callbacks. The passcode is checked by the
 * host (on NXU, by tepOS over the Trusted Enclave mailbox); UIService never
 * stores it and wipes its copy after each call.
 */
#define UI_SERVICE_LOGIN_ABI_VERSION_V1 1U

#define UI_SERVICE_PASSCODE_MIN 4U
#define UI_SERVICE_PASSCODE_MAX 64U

#define UI_SERVICE_AUTH_OK 0U
#define UI_SERVICE_AUTH_DENIED 1U       /* wrong passcode */
#define UI_SERVICE_AUTH_RETRY_LATER 2U  /* too soon after failures: *wait_seconds */
#define UI_SERVICE_AUTH_LOCKED 3U       /* until a recovery reset on the enclave */
#define UI_SERVICE_AUTH_UNAVAILABLE 4U  /* the checker cannot be reached */
#define UI_SERVICE_AUTH_NOT_SET 5U      /* no passcode yet */
#define UI_SERVICE_AUTH_INVALID 6U      /* bad length */
#define UI_SERVICE_AUTH_ERROR 7U        /* anything else: treated like UNAVAILABLE */

typedef struct {
    uint32_t passcode_set;
    uint32_t failures;
    uint32_t locked;
    uint32_t wait_seconds;
} UIServiceAuthStatus;

typedef uint32_t (*UIServiceAuthStatusFn)(void *context, UIServiceAuthStatus *status);
typedef uint32_t (*UIServiceAuthVerifyFn)(void *context, const uint8_t *passcode, uint32_t length, uint32_t *wait_seconds);
/* old_length 0 when no passcode is set yet (first boot). */
typedef uint32_t (*UIServiceAuthSetFn)(
    void *context,
    const uint8_t *old_passcode,
    uint32_t old_length,
    const uint8_t *passcode,
    uint32_t length,
    uint32_t *wait_seconds
);

typedef struct {
    UIServiceABIHeader header;
    void *context;
    UIServiceAuthStatusFn auth_status;
    UIServiceAuthVerifyFn auth_verify;
    UIServiceAuthSetFn auth_set;
} UIServiceLoginHostV1;

uint32_t UIServiceAPIVersion(void);
uint32_t UIServiceABIVersion(void);
uint32_t UIServiceHasInter(void);
uint32_t UIServiceHostV1Size(void);
uint32_t UIServiceHostV2Size(void);
uint32_t UIServiceHostV3Size(void);
uint32_t UIServiceHostV4Size(void);
uint32_t UIServiceHostV5Size(void);
uint32_t UIServiceValidateHost(const UIServiceHostV1 *host);
uint32_t UIServiceValidateHostV2(const UIServiceHostV2 *host);
uint32_t UIServiceValidateHostV3(const UIServiceHostV3 *host);
uint32_t UIServiceValidateHostV4(const UIServiceHostV4 *host);
uint32_t UIServiceValidateHostV5(const UIServiceHostV5 *host);
uint32_t UIServiceHostClear(const UIServiceHostV1 *host, uint32_t xrgb8888);
/*
 * The desktop: wallpaper, menu bar, and the windows of the app processes that
 * connect through the host kernel's UI session bridge (NXU's
 * drivers/video/ui_service_bridge.c, which it links against); the Dock is one
 * of them. No app is linked in. Returns only if the desktop could not start.
 */
uint32_t UIServiceRunDesktop(const UIServiceHostV5 *host);
/*
 * Fullscreen setup (first boot: welcome, then create a passcode) or login.
 * Returns UI_SERVICE_STATUS_OK only once the passcode was accepted (or set);
 * while the checker is unavailable it keeps waiting, it never lets anyone in.
 */
uint32_t UIServiceLoginHostV1Size(void);
uint32_t UIServiceRunLogin(const UIServiceHostV5 *host, const UIServiceLoginHostV1 *login);

/* Bring-up helpers. Normal applications should use the Rust app API. */
uint32_t UIServiceClear(
    uint32_t *pixels,
    uint32_t width,
    uint32_t height,
    uint32_t stride_pixels,
    uint32_t xrgb8888
);

uint32_t UIServiceDrawButton(
    uint32_t *pixels,
    uint32_t surface_width,
    uint32_t surface_height,
    uint32_t stride_pixels,
    int32_t x,
    int32_t y,
    uint32_t width,
    uint32_t height,
    uint32_t state
);

uint32_t UIServiceDrawDemo(
    uint32_t *pixels,
    uint32_t width,
    uint32_t height,
    uint32_t stride_pixels
);

#endif
