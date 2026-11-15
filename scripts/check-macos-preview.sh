#!/usr/bin/env bash
# Compile the preview event pump's public AppKit/OpenGL APIs against the SDK.
set -euo pipefail
build_root=${1:?Pass a separate build directory}
mkdir -p "$build_root"
cat > "$build_root/preview-api.m" <<'OBJC'
#import <AppKit/AppKit.h>

// This fixture validates SDK declarations and scalar argument types. It does
// not create a window or claim a virtual CI display measures physical FPS.
void checkPreviewAPI(NSApplication *app, NSWindow *window, NSOpenGLView *view,
                     GLint swapInterval) {
    NSOpenGLContext *context = view.openGLContext;
    [context setValues:&swapInterval forParameter:NSOpenGLContextParameterSwapInterval];
    [context makeCurrentContext];
    [window setReleasedWhenClosed:NO];
    [app finishLaunching];
    @autoreleasepool {
        NSDate *deadline = window.isVisible && !window.isMiniaturized ? nil :
            [NSDate dateWithTimeIntervalSinceNow:0.05];
        for (NSUInteger seen = 0; seen < 64; ++seen) {
            NSEvent *event = [app nextEventMatchingMask:NSEventMaskAny
                untilDate:deadline inMode:NSDefaultRunLoopMode dequeue:YES];
            if (!event) { break; }
            [app sendEvent:event];
            deadline = nil;
        }
        [app updateWindows];
        [view setNeedsDisplay:YES];
        [view displayIfNeeded];
        [context flushBuffer];
    }
    [window setDelegate:nil];
    [app setDelegate:nil];
    [window close];
    [window setContentView:nil];
}
OBJC
xcrun clang -std=c11 -fobjc-arc -c "$build_root/preview-api.m" \
  -o "$build_root/preview-api.o"
printf '%s\n' 'Public AppKit preview event pump and OpenGL swap APIs compile against the Apple SDK'
