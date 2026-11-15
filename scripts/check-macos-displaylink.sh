#!/usr/bin/env bash
# Check the public AppKit API and the exact Rust by-value QuartzCore encoding.
set -euo pipefail
backend_root=${1:?Pass the absolute miniquad checkout path}
build_root=${2:?Pass a separate build directory}
mkdir -p "$build_root"
encoding=$(python3 - "$backend_root/src/native/macos.rs" <<'PY'
import re
import sys
from pathlib import Path

source = Path(sys.argv[1]).read_text()
implementation = source.split('unsafe impl Encode for CAFrameRateRange {', 1)[1].split('\n}', 1)[0]
match = re.search(r'Encoding::from_str\("([^"\n]+)"\)', implementation)
assert match, 'Missing literal Rust CAFrameRateRange encoding'
print(match[1])
PY
)
cat > "$build_root/displaylink-abi.m" <<'OBJC'
#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#import <objc/runtime.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

_Static_assert(sizeof(CAFrameRateRange) == 12, "CAFrameRateRange size");
_Static_assert(_Alignof(CAFrameRateRange) == 4, "CAFrameRateRange alignment");
_Static_assert(offsetof(CAFrameRateRange, minimum) == 0, "minimum offset");
_Static_assert(offsetof(CAFrameRateRange, maximum) == 4, "maximum offset");
_Static_assert(offsetof(CAFrameRateRange, preferred) == 8, "preferred offset");

// Compile the selectors against Apple's headers without creating a window or
// treating a virtual CI display as evidence of physical ProMotion performance.
void checkDisplayLinkAPI(NSView *view, id target, SEL callback) {
    if (@available(macOS 14.0, *)) {
        CADisplayLink *link = [view displayLinkWithTarget:target selector:callback];
        NSInteger maximum = MIN(120, view.window.screen.maximumFramesPerSecond);
        link.preferredFrameRateRange = CAFrameRateRangeMake(MIN(60, maximum), maximum, maximum);
        [link addToRunLoop:NSRunLoop.mainRunLoop forMode:NSRunLoopCommonModes];
        [link invalidate];
    }
}

int main(int argc, const char **argv) {
    if (argc != 2 || strcmp(@encode(CAFrameRateRange), argv[1]) != 0) {
        fprintf(stderr, "Rust encoding differs from Apple SDK: %s\n", @encode(CAFrameRateRange));
        return 1;
    }
    if (@available(macOS 14.0, *)) {
        Method setter = class_getInstanceMethod(CADisplayLink.class, @selector(setPreferredFrameRateRange:));
        if (!setter) { return 2; }
        char *argument = method_copyArgumentType(setter, 2);
        int matches = argument && strcmp(argument, argv[1]) == 0;
        free(argument);
        if (!matches) { return 3; }
    }
    puts("AppKit display link API and Rust CAFrameRateRange ABI match the Apple SDK");
    return 0;
}
OBJC
xcrun clang -std=c11 -fobjc-arc -framework AppKit -framework QuartzCore \
  "$build_root/displaylink-abi.m" -o "$build_root/displaylink-abi"
"$build_root/displaylink-abi" "$encoding"
