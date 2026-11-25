# Vendored: WebP encoder (WASM)

`webp_enc.js` and `webp_enc.wasm` are the non-SIMD libwebp encoder from `@jsquash/webp` 1.5.0
(`codec/enc/`, Apache-2.0, see `LICENSE-jsquash-webp`; unmodified). The admin page loads them from
`/admin/assets/` only when the browser's canvas cannot encode WebP (Safari and iOS return a PNG).
To update: `npm pack @jsquash/webp` in a scratch directory and copy the two files.
