# Application icons

`icon.svg` is the artwork and the source of truth; edit it, never the PNGs.
The mark is a play triangle cut into three pieces that fade as they go — the
video getting lighter. `src/app/icon.svg` is a copy for the webview favicon,
and `src/components/Logo.tsx` is a trim of the same three paths without the
background tile, for use on the app's own surface.

Regenerate after changing the artwork:

```bash
pnpm tauri icon src-tauri/icons/icon.svg
rm -rf src-tauri/icons/{android,ios} src-tauri/icons/Square*Logo.png \
       src-tauri/icons/StoreLogo.png src-tauri/icons/64x64.png
cp src-tauri/icons/icon.svg src/app/icon.svg
```

The removed files are for Android, iOS, and the Microsoft Store, none of which
karui targets.

**The PNGs must be RGBA.** `tauri::generate_context!` rejects anything else at
compile time — *"icon 32x32.png is not RGBA"*. `tauri icon` writes RGBA, but a
hand-made replacement may not. Verify with:

```bash
python3 -c "import struct,glob
for f in sorted(glob.glob('*.png')):
    print(f, struct.unpack('>B', open(f,'rb').read()[25:26])[0])"   # must print 6
```
