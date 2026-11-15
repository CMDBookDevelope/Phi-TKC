#include "PhiTKCNativeBridge.h"

/* This target supplies declarations only; Rust supplies all implementations. */
_Static_assert(sizeof(uint64_t) == 8, "Rust u64 requires eight bytes");
_Static_assert(sizeof(int32_t) == 4, "Rust i32 requires four bytes");
_Static_assert(sizeof(float) == 4, "Rust f32 requires four bytes");
_Static_assert(sizeof(double) == 8, "Rust f64 requires eight bytes");
_Static_assert(sizeof(size_t) == sizeof(void *), "Rust usize matches pointer width");
