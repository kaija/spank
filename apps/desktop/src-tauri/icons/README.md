# Icons

`app-icon.svg` is the source of the app icon: the `SpankIcons.app` glyph (laptop with `> <` eyes, from `mockup/spank-icons.js`) on the UI accent purple, laid out on Apple's 1024 px macOS icon grid.

After editing it, regenerate the PNGs and `icon.icns` and copy only the files the app uses:

```bash
pnpm --dir apps/desktop tauri icon "$PWD/apps/desktop/src-tauri/icons/app-icon.svg" -o /tmp/spank-icons
pnpm --dir apps/desktop tauri icon "$PWD/apps/desktop/src-tauri/icons/app-icon.svg" -o /tmp/spank-icons-1024 -p 1024
cp /tmp/spank-icons/{32x32.png,128x128.png,128x128@2x.png,icon.png,icon.icns} apps/desktop/src-tauri/icons/
cp /tmp/spank-icons-1024/1024x1024.png apps/desktop/src-tauri/icons/app-icon.png
```

`tray-template.png` is the menu bar icon (a template image, drawn separately).
