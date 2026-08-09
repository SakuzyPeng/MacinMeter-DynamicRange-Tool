export const EXPORT_WRITE_CHUNK_BYTES = 256 * 1024;
export const PNG_PAGE_MAX_CONTENT_HEIGHT = 4_000;
export const SVG_PAGE_MAX_CONTENT_HEIGHT = 12_000;

const IMAGE_MAX_SCALE = 3;
const IMAGE_MAX_CANVAS_DIMENSION = 16_384;
const IMAGE_MAX_CANVAS_PIXELS = 32 * 1024 * 1024;

export type ByteRange = {
  start: number;
  end: number;
};

export const byteRanges = (
  byteLength: number,
  chunkBytes = EXPORT_WRITE_CHUNK_BYTES,
): ByteRange[] => {
  if (!Number.isSafeInteger(byteLength) || byteLength < 0) {
    throw new RangeError("byteLength must be a non-negative safe integer");
  }
  if (!Number.isSafeInteger(chunkBytes) || chunkBytes <= 0) {
    throw new RangeError("chunkBytes must be a positive safe integer");
  }

  const ranges: ByteRange[] = [];
  for (let start = 0; start < byteLength; start += chunkBytes) {
    ranges.push({ start, end: Math.min(byteLength, start + chunkBytes) });
  }
  return ranges;
};

export const numberedExportPath = (
  path: string,
  pageIndex: number,
  pageCount: number,
): string => {
  if (pageCount <= 1) return path;
  if (
    !Number.isInteger(pageIndex) ||
    pageIndex < 0 ||
    !Number.isInteger(pageCount) ||
    pageIndex >= pageCount
  ) {
    throw new RangeError("page index is outside the export page count");
  }

  const separator = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  const extension = path.lastIndexOf(".");
  const insertion = extension > separator ? extension : path.length;
  const digits = Math.max(2, String(pageCount).length);
  const suffix = `_${String(pageIndex + 1).padStart(digits, "0")}`;
  return `${path.slice(0, insertion)}${suffix}${path.slice(insertion)}`;
};

export const paginateItemIndexes = (
  itemHeights: readonly number[],
  gap: number,
  maxContentHeight = PNG_PAGE_MAX_CONTENT_HEIGHT,
): number[][] => {
  if (!(Number.isFinite(maxContentHeight) && maxContentHeight > 0)) {
    throw new RangeError("maxContentHeight must be positive and finite");
  }
  const safeGap = Number.isFinite(gap) ? Math.max(0, gap) : 0;
  const pages: number[][] = [];
  let page: number[] = [];
  let pageHeight = 0;

  itemHeights.forEach((height, index) => {
    const safeHeight = Number.isFinite(height) ? Math.max(0, height) : 0;
    const nextHeight =
      pageHeight + (page.length === 0 ? 0 : safeGap) + safeHeight;
    if (page.length > 0 && nextHeight > maxContentHeight) {
      pages.push(page);
      page = [];
      pageHeight = 0;
    }
    pageHeight += (page.length === 0 ? 0 : safeGap) + safeHeight;
    page.push(index);
  });

  if (page.length > 0) pages.push(page);
  return pages;
};

export const safeImageScale = (width: number, height: number): number => {
  const safeWidth = Math.max(1, Number.isFinite(width) ? width : 1);
  const safeHeight = Math.max(1, Number.isFinite(height) ? height : 1);
  return Math.min(
    IMAGE_MAX_SCALE,
    IMAGE_MAX_CANVAS_DIMENSION / Math.max(safeWidth, safeHeight),
    Math.sqrt(IMAGE_MAX_CANVAS_PIXELS / (safeWidth * safeHeight)),
  );
};
