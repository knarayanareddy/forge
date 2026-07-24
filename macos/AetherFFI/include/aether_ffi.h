#ifndef AETHER_FFI_H
#define AETHER_FFI_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

const char *aether_version(void);
uint16_t aether_daemon_default_port(void);
char *aether_ffi_daemon_ipc(void);
void aether_free_string(char *s);

#ifdef __cplusplus
}
#endif

#endif /* AETHER_FFI_H */
