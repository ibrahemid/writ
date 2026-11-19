import { crc32, deflateSync, inflateSync } from "node:zlib";

const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const PNG_BIT_DEPTH = 8;
const PNG_COLOR_TYPE_RGBA = 6;
const BYTES_PER_PIXEL = 4;
const ICO_HEADER_BYTES = 6;
const ICO_ENTRY_BYTES = 16;
const ICO_MAX_SIZE = 256;
const ICNS_MAGIC = "icns";
const ICNS_HEADER_BYTES = 8;
const RLE_MIN_RUN = 3;
const RLE_MAX_RUN = 130;
const RLE_MAX_LITERAL = 128;

export const ICO_SIZES = Object.freeze([16, 24, 32, 48, 64, 256]);

export const ICNS_LAYOUT = Object.freeze(
  [
    { type: "is32", size: 16, encoding: "rle", mask: "s8mk" },
    { type: "ic11", size: 32, encoding: "png" },
    { type: "il32", size: 32, encoding: "rle", mask: "l8mk" },
    { type: "ic12", size: 64, encoding: "png" },
    { type: "ic07", size: 128, encoding: "png" },
    { type: "ic13", size: 256, encoding: "png" },
    { type: "ic08", size: 256, encoding: "png" },
    { type: "ic14", size: 512, encoding: "png" },
    { type: "ic09", size: 512, encoding: "png" },
    { type: "ic10", size: 1024, encoding: "png" },
  ].map((entry) => Object.freeze(entry)),
);

export class IconFormatError extends Error {
  constructor(message) {
    super(message);
    this.name = "IconFormatError";
  }
}

function validateImage(image, label) {
  const { width, height, rgba } = image ?? {};
  if (!Number.isInteger(width) || !Number.isInteger(height) || width < 1 || height < 1) {
    throw new IconFormatError(`${label} must have positive integer dimensions, got ${width}x${height}`);
  }
  if (!(rgba instanceof Uint8Array) || rgba.length !== width * height * BYTES_PER_PIXEL) {
    throw new IconFormatError(`${label} must carry ${width * height * BYTES_PER_PIXEL} RGBA bytes`);
  }
}

function buildPngChunk(type, data) {
  const header = Buffer.alloc(8);
  header.writeUInt32BE(data.length, 0);
  header.write(type, 4, "latin1");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(data, crc32(header.subarray(4))), 0);
  return Buffer.concat([header, data, crc]);
}

function predictPaeth(left, up, upLeft) {
  const estimate = left + up - upLeft;
  const toLeft = Math.abs(estimate - left);
  const toUp = Math.abs(estimate - up);
  const toUpLeft = Math.abs(estimate - upLeft);
  if (toLeft <= toUp && toLeft <= toUpLeft) return left;
  return toUp <= toUpLeft ? up : upLeft;
}

function predictByte(filter, left, up, upLeft) {
  switch (filter) {
    case 0:
      return 0;
    case 1:
      return left;
    case 2:
      return up;
    case 3:
      return (left + up) >> 1;
    case 4:
      return predictPaeth(left, up, upLeft);
    default:
      throw new IconFormatError(`unknown PNG filter type ${filter}`);
  }
}

function filterScanlines(image) {
  const { width, height, rgba } = image;
  const stride = width * BYTES_PER_PIXEL;
  const filtered = Buffer.alloc((stride + 1) * height);
  const candidates = Array.from({ length: 5 }, () => Buffer.alloc(stride));
  for (let y = 0; y < height; y++) {
    const row = y * stride;
    let bestFilter = 0;
    let bestScore = Infinity;
    for (let filter = 0; filter < 5; filter++) {
      const line = candidates[filter];
      let score = 0;
      for (let i = 0; i < stride; i++) {
        const left = i >= BYTES_PER_PIXEL ? rgba[row + i - BYTES_PER_PIXEL] : 0;
        const up = y > 0 ? rgba[row - stride + i] : 0;
        const upLeft = y > 0 && i >= BYTES_PER_PIXEL ? rgba[row - stride + i - BYTES_PER_PIXEL] : 0;
        const value = (rgba[row + i] - predictByte(filter, left, up, upLeft)) & 0xff;
        line[i] = value;
        score += value < 128 ? value : 256 - value;
      }
      if (score < bestScore) {
        bestScore = score;
        bestFilter = filter;
      }
    }
    const target = y * (stride + 1);
    filtered[target] = bestFilter;
    candidates[bestFilter].copy(filtered, target + 1);
  }
  return filtered;
}

function unfilterScanlines(filtered, width, height) {
  const stride = width * BYTES_PER_PIXEL;
  if (filtered.length !== (stride + 1) * height) {
    throw new IconFormatError(`PNG image data is ${filtered.length} bytes, expected ${(stride + 1) * height}`);
  }
  const rgba = new Uint8Array(stride * height);
  for (let y = 0; y < height; y++) {
    const source = y * (stride + 1);
    const filter = filtered[source];
    const row = y * stride;
    for (let i = 0; i < stride; i++) {
      const left = i >= BYTES_PER_PIXEL ? rgba[row + i - BYTES_PER_PIXEL] : 0;
      const up = y > 0 ? rgba[row - stride + i] : 0;
      const upLeft = y > 0 && i >= BYTES_PER_PIXEL ? rgba[row - stride + i - BYTES_PER_PIXEL] : 0;
      rgba[row + i] = (filtered[source + 1 + i] + predictByte(filter, left, up, upLeft)) & 0xff;
    }
  }
  return rgba;
}

export function encodePng(image) {
  validateImage(image, "PNG image");
  const header = Buffer.alloc(13);
  header.writeUInt32BE(image.width, 0);
  header.writeUInt32BE(image.height, 4);
  header[8] = PNG_BIT_DEPTH;
  header[9] = PNG_COLOR_TYPE_RGBA;
  return Buffer.concat([
    PNG_SIGNATURE,
    buildPngChunk("IHDR", header),
    buildPngChunk("IDAT", deflateSync(filterScanlines(image), { level: 9 })),
    buildPngChunk("IEND", Buffer.alloc(0)),
  ]);
}

export function decodePng(bytes) {
  const buffer = Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (buffer.length < PNG_SIGNATURE.length || !buffer.subarray(0, PNG_SIGNATURE.length).equals(PNG_SIGNATURE)) {
    throw new IconFormatError("data is not a PNG file: signature mismatch");
  }
  let cursor = PNG_SIGNATURE.length;
  let header = null;
  const imageData = [];
  let ended = false;
  while (!ended) {
    if (cursor + 12 > buffer.length) throw new IconFormatError(`PNG chunk at byte ${cursor} is truncated`);
    const length = buffer.readUInt32BE(cursor);
    const type = buffer.toString("latin1", cursor + 4, cursor + 8);
    const dataStart = cursor + 8;
    const dataEnd = dataStart + length;
    if (dataEnd + 4 > buffer.length) throw new IconFormatError(`PNG chunk ${type} overruns the file`);
    const expectedCrc = buffer.readUInt32BE(dataEnd);
    if (crc32(buffer.subarray(cursor + 4, dataEnd)) !== expectedCrc) {
      throw new IconFormatError(`PNG chunk ${type} fails its CRC check`);
    }
    const data = buffer.subarray(dataStart, dataEnd);
    if (header === null && type !== "IHDR") throw new IconFormatError(`PNG starts with ${type} instead of IHDR`);
    if (type === "IHDR") {
      if (length !== 13) throw new IconFormatError(`PNG IHDR is ${length} bytes, expected 13`);
      header = {
        width: data.readUInt32BE(0),
        height: data.readUInt32BE(4),
        bitDepth: data[8],
        colorType: data[9],
        compression: data[10],
        filterMethod: data[11],
        interlace: data[12],
      };
    } else if (type === "IDAT") {
      imageData.push(data);
    } else if (type === "IEND") {
      ended = true;
    }
    cursor = dataEnd + 4;
  }
  const { width, height, bitDepth, colorType, compression, filterMethod, interlace } = header;
  if (bitDepth !== PNG_BIT_DEPTH || colorType !== PNG_COLOR_TYPE_RGBA) {
    throw new IconFormatError(`PNG is bit depth ${bitDepth} colour type ${colorType}; only 8-bit RGBA is supported`);
  }
  if (compression !== 0 || filterMethod !== 0 || interlace !== 0) {
    throw new IconFormatError("PNG uses a non-standard compression, filter method or interlacing");
  }
  if (imageData.length === 0) throw new IconFormatError("PNG has no IDAT chunk");
  let inflated;
  try {
    inflated = inflateSync(Buffer.concat(imageData));
  } catch (error) {
    throw new IconFormatError(`PNG image data does not inflate: ${error.message}`);
  }
  return { width, height, rgba: unfilterScanlines(inflated, width, height) };
}

export function encodeIco(images) {
  if (!Array.isArray(images) || images.length === 0) throw new IconFormatError("ICO needs at least one image");
  const frames = images.map((image, index) => {
    validateImage(image, `ICO image ${index}`);
    if (image.width > ICO_MAX_SIZE || image.height > ICO_MAX_SIZE) {
      throw new IconFormatError(`ICO image ${index} is ${image.width}x${image.height}; the limit is ${ICO_MAX_SIZE}`);
    }
    return encodePng(image);
  });
  const directory = Buffer.alloc(ICO_HEADER_BYTES + ICO_ENTRY_BYTES * frames.length);
  directory.writeUInt16LE(0, 0);
  directory.writeUInt16LE(1, 2);
  directory.writeUInt16LE(frames.length, 4);
  let offset = directory.length;
  frames.forEach((frame, index) => {
    const entry = ICO_HEADER_BYTES + ICO_ENTRY_BYTES * index;
    directory[entry] = images[index].width % ICO_MAX_SIZE;
    directory[entry + 1] = images[index].height % ICO_MAX_SIZE;
    directory[entry + 2] = 0;
    directory[entry + 3] = 0;
    directory.writeUInt16LE(0, entry + 4);
    directory.writeUInt16LE(32, entry + 6);
    directory.writeUInt32LE(frame.length, entry + 8);
    directory.writeUInt32LE(offset, entry + 12);
    offset += frame.length;
  });
  return Buffer.concat([directory, ...frames]);
}

export function parseIco(bytes) {
  const buffer = Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (buffer.length < ICO_HEADER_BYTES) throw new IconFormatError("ICO file is shorter than its header");
  if (buffer.readUInt16LE(0) !== 0 || buffer.readUInt16LE(2) !== 1) {
    throw new IconFormatError("data is not an ICO file: header reserved/type mismatch");
  }
  const count = buffer.readUInt16LE(4);
  if (ICO_HEADER_BYTES + ICO_ENTRY_BYTES * count > buffer.length) {
    throw new IconFormatError(`ICO directory of ${count} entries overruns the file`);
  }
  return Array.from({ length: count }, (_, index) => {
    const entry = ICO_HEADER_BYTES + ICO_ENTRY_BYTES * index;
    const size = buffer.readUInt32LE(entry + 8);
    const offset = buffer.readUInt32LE(entry + 12);
    if (offset + size > buffer.length) throw new IconFormatError(`ICO entry ${index} overruns the file`);
    return {
      width: buffer[entry] === 0 ? ICO_MAX_SIZE : buffer[entry],
      height: buffer[entry + 1] === 0 ? ICO_MAX_SIZE : buffer[entry + 1],
      colorCount: buffer[entry + 2],
      planes: buffer.readUInt16LE(entry + 4),
      bitCount: buffer.readUInt16LE(entry + 6),
      size,
      offset,
      data: buffer.subarray(offset, offset + size),
    };
  });
}

export function encodeIcnsRle(image) {
  validateImage(image, "ICNS RLE image");
  const pixels = image.width * image.height;
  const output = [];
  for (let channel = 0; channel < 3; channel++) {
    let literal = [];
    const flushLiteral = () => {
      if (literal.length === 0) return;
      output.push(literal.length - 1, ...literal);
      literal = [];
    };
    let index = 0;
    while (index < pixels) {
      const value = image.rgba[index * BYTES_PER_PIXEL + channel];
      let run = 1;
      while (
        index + run < pixels &&
        run < RLE_MAX_RUN &&
        image.rgba[(index + run) * BYTES_PER_PIXEL + channel] === value
      ) {
        run++;
      }
      if (run >= RLE_MIN_RUN) {
        flushLiteral();
        output.push(0x80 + run - RLE_MIN_RUN, value);
        index += run;
      } else {
        literal.push(value);
        index++;
        if (literal.length === RLE_MAX_LITERAL) flushLiteral();
      }
    }
    flushLiteral();
  }
  return Buffer.from(output);
}

export function decodeIcnsRle(bytes, pixelCount) {
  if (!Number.isInteger(pixelCount) || pixelCount < 1) {
    throw new IconFormatError(`ICNS RLE pixel count must be a positive integer, got ${pixelCount}`);
  }
  const rgb = new Uint8Array(pixelCount * 3);
  let cursor = 0;
  for (let channel = 0; channel < 3; channel++) {
    let index = 0;
    while (index < pixelCount) {
      if (cursor >= bytes.length) throw new IconFormatError(`ICNS RLE data ends inside channel ${channel}`);
      const control = bytes[cursor++];
      const literal = control < 0x80;
      const count = literal ? control + 1 : control - 0x80 + RLE_MIN_RUN;
      if (index + count > pixelCount) throw new IconFormatError(`ICNS RLE run overflows channel ${channel}`);
      if (cursor + (literal ? count : 1) > bytes.length) {
        throw new IconFormatError(`ICNS RLE data ends inside a run in channel ${channel}`);
      }
      for (let step = 0; step < count; step++) {
        rgb[(index + step) * 3 + channel] = literal ? bytes[cursor + step] : bytes[cursor];
      }
      cursor += literal ? count : 1;
      index += count;
    }
  }
  if (cursor !== bytes.length) throw new IconFormatError(`ICNS RLE data has ${bytes.length - cursor} trailing bytes`);
  return rgb;
}

function extractAlpha(image) {
  const alpha = Buffer.alloc(image.width * image.height);
  for (let index = 0; index < alpha.length; index++) alpha[index] = image.rgba[index * BYTES_PER_PIXEL + 3];
  return alpha;
}

function buildIcnsElement(type, data) {
  const header = Buffer.alloc(ICNS_HEADER_BYTES);
  header.write(type, 0, "latin1");
  header.writeUInt32BE(ICNS_HEADER_BYTES + data.length, 4);
  return Buffer.concat([header, data]);
}

export function encodeIcns(imagesBySize) {
  const elements = [];
  for (const { type, size, encoding, mask } of ICNS_LAYOUT) {
    const image = imagesBySize.get(size);
    if (image === undefined) throw new IconFormatError(`ICNS ${type} needs a ${size}x${size} image`);
    validateImage(image, `ICNS ${type} image`);
    if (image.width !== size || image.height !== size) {
      throw new IconFormatError(`ICNS ${type} needs ${size}x${size}, got ${image.width}x${image.height}`);
    }
    if (encoding === "png") {
      elements.push(buildIcnsElement(type, encodePng(image)));
    } else {
      elements.push(buildIcnsElement(type, encodeIcnsRle(image)));
      elements.push(buildIcnsElement(mask, extractAlpha(image)));
    }
  }
  const body = Buffer.concat(elements);
  const header = Buffer.alloc(ICNS_HEADER_BYTES);
  header.write(ICNS_MAGIC, 0, "latin1");
  header.writeUInt32BE(ICNS_HEADER_BYTES + body.length, 4);
  return Buffer.concat([header, body]);
}

export function parseIcns(bytes) {
  const buffer = Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (buffer.length < ICNS_HEADER_BYTES || buffer.toString("latin1", 0, 4) !== ICNS_MAGIC) {
    throw new IconFormatError("data is not an ICNS file: magic mismatch");
  }
  if (buffer.readUInt32BE(4) !== buffer.length) {
    throw new IconFormatError(`ICNS header declares ${buffer.readUInt32BE(4)} bytes, file has ${buffer.length}`);
  }
  const elements = [];
  let cursor = ICNS_HEADER_BYTES;
  while (cursor < buffer.length) {
    if (cursor + ICNS_HEADER_BYTES > buffer.length) throw new IconFormatError(`ICNS element at byte ${cursor} is truncated`);
    const type = buffer.toString("latin1", cursor, cursor + 4);
    const length = buffer.readUInt32BE(cursor + 4);
    if (length < ICNS_HEADER_BYTES || cursor + length > buffer.length) {
      throw new IconFormatError(`ICNS element ${type} declares an invalid length ${length}`);
    }
    elements.push({ type, data: buffer.subarray(cursor + ICNS_HEADER_BYTES, cursor + length) });
    cursor += length;
  }
  return elements;
}

export function decodeIcnsImage(elements, type) {
  const layout = ICNS_LAYOUT.find((entry) => entry.type === type);
  if (layout === undefined) throw new IconFormatError(`ICNS type ${type} is not part of the app icon layout`);
  const findElement = (wanted) => {
    const element = elements.find((candidate) => candidate.type === wanted);
    if (element === undefined) throw new IconFormatError(`ICNS file has no ${wanted} element`);
    return element;
  };
  const { size } = layout;
  if (layout.encoding === "png") {
    const image = decodePng(findElement(type).data);
    if (image.width !== size || image.height !== size) {
      throw new IconFormatError(`ICNS ${type} holds ${image.width}x${image.height}, expected ${size}x${size}`);
    }
    return image;
  }
  const pixelCount = size * size;
  const rgb = decodeIcnsRle(findElement(type).data, pixelCount);
  const alpha = findElement(layout.mask).data;
  if (alpha.length !== pixelCount) {
    throw new IconFormatError(`ICNS ${layout.mask} holds ${alpha.length} bytes, expected ${pixelCount}`);
  }
  const rgba = new Uint8Array(pixelCount * BYTES_PER_PIXEL);
  for (let index = 0; index < pixelCount; index++) {
    rgba[index * BYTES_PER_PIXEL] = rgb[index * 3];
    rgba[index * BYTES_PER_PIXEL + 1] = rgb[index * 3 + 1];
    rgba[index * BYTES_PER_PIXEL + 2] = rgb[index * 3 + 2];
    rgba[index * BYTES_PER_PIXEL + 3] = alpha[index];
  }
  return { width: size, height: size, rgba };
}
