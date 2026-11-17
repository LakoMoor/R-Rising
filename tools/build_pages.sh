#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
./tools/build_web.sh
# Build outputs are ignored; game installation and scene exports never enter this directory.
mkdir -p target/pages/assets/branding target/pages/assets/screenshots target/pages/viewer
cp site/index.html site/style.css site/app.js target/pages/
cp docs/progress.json target/pages/progress.json
cp docs/branding/cover.png docs/branding/logo.png target/pages/assets/branding/
cp docs/screenshots/native-content.png docs/screenshots/browser-webgpu.png docs/screenshots/native-original-scene.png docs/screenshots/browser-original-scene.png target/pages/assets/screenshots/
cp web/index.html target/pages/viewer/index.html
mkdir -p target/pages/viewer/pkg target/pages/viewer/branding
cp web/pkg/rrising_web.js web/pkg/rrising_web_bg.wasm target/pages/viewer/pkg/
cp docs/branding/logo.png target/pages/viewer/branding/logo.png
printf '%s\n' 'Built GitHub Pages artifact in target/pages/'
