#ifndef PHI_TKC_NATIVE_BRIDGE_H
#define PHI_TKC_NATIVE_BRIDGE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Implemented by the Rust application. Importing these declarations makes
 * Swift use the platform C calling convention for every scalar and pointer. */
void phi_tkc_render_lifecycle(int32_t active);
int32_t phi_tkc_preview_read_frame(uint64_t session, uint8_t *bytes, size_t capacity);
void phi_tkc_preview_touch(uint64_t session, uint64_t contact, int32_t phase,
                          float x, float y, double time);
void phi_tkc_preview_active(uint64_t session, int32_t active);
void phi_tkc_preview_close(uint64_t session);
float phi_tkc_preview_fps(uint64_t session);
void phi_tkc_preview_set_refresh_rate(uint64_t session, uint32_t frames_per_second);
void phi_tkc_preview_frame_tick(uint64_t session);

#ifdef __cplusplus
}
#endif

#endif
