#!/usr/bin/env bash
# Run the real window smoke when this host provides its accelerated OpenGL API.
set -euo pipefail
miniquad_manifest=${1:?Pass the miniquad Cargo.toml}
build_root=${2:?Pass a separate build directory}
mkdir -p "$build_root"
export PHI_TKC_APPLE_SMOKE_ARTIFACTS="${PHI_TKC_APPLE_SMOKE_ARTIFACTS:-$build_root}"
mkdir -p "$PHI_TKC_APPLE_SMOKE_ARTIFACTS"

cat > "$build_root/preview-opengl-probe.m" <<'OBJC'
#define GL_SILENCE_DEPRECATION
#import <AppKit/AppKit.h>
#import <OpenGL/gl3.h>
#include <stdio.h>

int main(void) {
    @autoreleasepool {
        NSApplication *app = [NSApplication sharedApplication];
        [app setActivationPolicy:NSApplicationActivationPolicyRegular];
        [app finishLaunching];
        const NSOpenGLPixelFormatAttribute profiles[] = {
            NSOpenGLProfileVersion4_1Core, NSOpenGLProfileVersion3_2Core
        };
        const char *names[] = { "4.1 Core", "3.2 Core" };
        for (NSUInteger i = 0; i < 2; ++i) {
            // Match create_opengl_view and macos_preview_smoke: accelerated,
            // double-buffered, RGBA8/depth24/stencil8, sample_count = 1.
            NSOpenGLPixelFormatAttribute attributes[] = {
                NSOpenGLPFAAccelerated, NSOpenGLPFADoubleBuffer,
                NSOpenGLPFAOpenGLProfile, profiles[i],
                NSOpenGLPFAColorSize, 24, NSOpenGLPFAAlphaSize, 8,
                NSOpenGLPFADepthSize, 24, NSOpenGLPFAStencilSize, 8,
                NSOpenGLPFASampleBuffers, 0, 0
            };
            NSOpenGLPixelFormat *format = [[NSOpenGLPixelFormat alloc]
                initWithAttributes:attributes];
            if (!format) {
                fprintf(stderr, "OPENGL_PROBE %s: pixel format unavailable\n", names[i]);
                continue;
            }
            NSOpenGLContext *context = [[NSOpenGLContext alloc]
                initWithFormat:format shareContext:nil];
            if (!context) {
                fprintf(stderr, "OPENGL_PROBE %s: context unavailable\n", names[i]);
                continue;
            }
            [context makeCurrentContext];
            const GLubyte *version = glGetString(GL_VERSION);
            const GLubyte *renderer = glGetString(GL_RENDERER);
            if (!version || !renderer || glGetError() != GL_NO_ERROR) {
                fprintf(stderr, "OPENGL_PROBE %s: created context is unusable\n", names[i]);
                return 1;
            }
            fprintf(stderr, "OPENGL_PROBE_AVAILABLE %s: %s / %s\n",
                names[i], (const char *)version, (const char *)renderer);
            [NSOpenGLContext clearCurrentContext];
            return 0;
        }
        // Only the explicit absence of both native profiles means unsupported.
        fprintf(stderr, "OPENGL_PROBE_UNAVAILABLE: neither accelerated core profile exists\n");
        return 77;
    }
}
OBJC
# AppKit's NSOpenGL types are deprecated too; silence that expected warning
# while retaining errors for every other compiler warning.
xcrun clang -std=c11 -fobjc-arc -Wall -Wextra -Werror -Wno-deprecated-declarations \
  "$build_root/preview-opengl-probe.m" -framework AppKit -framework OpenGL \
  -o "$build_root/preview-opengl-probe"
probe_status=0
"$build_root/preview-opengl-probe" \
  > "$PHI_TKC_APPLE_SMOKE_ARTIFACTS/macos-preview-opengl-probe.log" 2>&1 || probe_status=$?
cat "$PHI_TKC_APPLE_SMOKE_ARTIFACTS/macos-preview-opengl-probe.log"
case "$probe_status" in
  0)
    # A supported host must pass all window, resize, input, return and Drop checks.
    cargo run --manifest-path "$miniquad_manifest" --example macos_preview_smoke
    ;;
  77)
    echo '::notice::Visible OpenGL preview smoke unavailable: neither native core profile can be created. Metal rendering and encoding checks remain required.'
    ;;
  *)
    echo "::error::OpenGL capability probe failed with status $probe_status"
    exit "$probe_status"
    ;;
esac
