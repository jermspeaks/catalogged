import { APIRequestContext, expect } from '@playwright/test';
import zlib from 'node:zlib';

/** A valid solid-colour PNG (the browser decodes it and the admin re-encodes it to WebP). */
export function png(w = 40, h = 60, rgb: [number, number, number] = [114, 25, 90]): Buffer {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (b: Buffer) => {
    let c = 0xffffffff;
    for (const x of b) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type: string, data: Buffer) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  const row = Buffer.concat([Buffer.from([0]), Buffer.from(Array.from({ length: w }, () => rgb).flat())]);
  const raw = Buffer.concat(Array.from({ length: h }, () => row));
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0)),
  ]);
}

/** Smallest thing the Worker accepts as a WebP (it checks the RIFF/WEBP magic and size only). */
export function fakeWebp(extra = 16): Buffer {
  return Buffer.concat([Buffer.from('RIFF'), Buffer.from([0, 0, 0, 0]), Buffer.from('WEBP'), Buffer.alloc(extra)]);
}

export type Seed = { kind: string; title: string; status?: string; rating?: number; year?: number; review?: string; consumed_at?: string; creator?: string };

export async function seed(request: APIRequestContext, e: Seed): Promise<string> {
  const r = await request.post('/admin/api/entries', { data: { status: 'consumed', ...e } });
  expect(r.status(), await r.text()).toBe(200);
  return (await r.json()).slug;
}

export async function entry(request: APIRequestContext, kind: string, slug: string) {
  const r = await request.get(`/admin/api/entries/${kind}/${slug}`);
  return { status: r.status(), body: r.ok() ? await r.json() : null };
}
