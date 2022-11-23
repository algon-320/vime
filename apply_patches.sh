#!/bin/bash

case "${1:-}" in
"" | "apply")
    (cd toyterm       && git apply ../patches/toyterm.patch)
    (cd winit         && git apply ../patches/winit.patch)
    (cd xcb-imdkit-rs && git apply ../patches/xcb-imdkit-rs.patch)
    ;;

"restore")
    (cd toyterm       && git restore .)
    (cd winit         && git restore .)
    (cd xcb-imdkit-rs && git restore .)
    ;;

"update")
    (cd toyterm       && git diff > ../patches/toyterm.patch)
    (cd winit         && git diff > ../patches/winit.patch)
    (cd xcb-imdkit-rs && git diff > ../patches/xcb-imdkit-rs.patch)
    ;;
esac
