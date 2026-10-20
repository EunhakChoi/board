// A minimal ustar writer: enough to hand a run its input files through the Docker API.

export interface TarFile {
  /** Relative path, `/`-separated, under 100 bytes. */
  path: string;
  data: Buffer;
  mode?: number;
}

const BLOCK = 512;

export function tar(files: TarFile[], owner = { uid: 0, gid: 0 }): Buffer {
  const parts: Buffer[] = [];
  const dirs = new Set<string>();
  for (const file of files) {
    const segments = file.path.split("/");
    for (let i = 1; i < segments.length; i++) {
      const dir = `${segments.slice(0, i).join("/")}/`;
      if (dirs.has(dir)) continue;
      dirs.add(dir);
      parts.push(header(dir, 0, 0o755, "5", owner));
    }
    parts.push(header(file.path, file.data.length, file.mode ?? 0o644, "0", owner));
    parts.push(file.data, Buffer.alloc(pad(file.data.length)));
  }
  parts.push(Buffer.alloc(BLOCK * 2));
  return Buffer.concat(parts);
}

function pad(size: number): number {
  return (BLOCK - (size % BLOCK)) % BLOCK;
}

function header(
  path: string,
  size: number,
  mode: number,
  type: string,
  owner: { uid: number; gid: number },
): Buffer {
  if (Buffer.byteLength(path) > 99) throw new Error(`tar path too long: ${path}`);
  const block = Buffer.alloc(BLOCK);
  block.write(path, 0, "utf8");
  octal(block, 100, 8, mode);
  octal(block, 108, 8, owner.uid);
  octal(block, 116, 8, owner.gid);
  octal(block, 124, 12, size);
  octal(block, 136, 12, Math.floor(Date.now() / 1000));
  block.fill(" ", 148, 156);
  block.write(type, 156, "ascii");
  block.write("ustar\u000000", 257, "ascii");
  let sum = 0;
  for (const byte of block) sum += byte;
  octal(block, 148, 7, sum);
  return block;
}

function octal(block: Buffer, offset: number, length: number, value: number): void {
  block.write(`${value.toString(8).padStart(length - 1, "0")}\u0000`, offset, length, "ascii");
}
