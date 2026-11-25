export interface Window {
  start: number;
  end: number;
  padTop: number;
  padBottom: number;
  total: number;
}

/**
 * Prefix-sum windowing for variable-height chat rows. `heights` is indexed by
 * row; unknown rows use `estimate` until their real height is measured.
 */
export function computeWindow(
  heights: number[],
  scrollTop: number,
  viewportHeight: number,
  estimate = 150,
  overscan = 5,
): Window {
  const count = heights.length;
  if (!count) return { start: 0, end: 0, padTop: 0, padBottom: 0, total: 0 };

  const offsets: number[] = new Array(count + 1);
  offsets[0] = 0;
  for (let i = 0; i < count; i++) offsets[i + 1] = offsets[i] + (heights[i] || estimate);
  const total = offsets[count];

  let lo = 0;
  let hi = count;
  const top = Math.max(0, scrollTop);
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (offsets[mid + 1] <= top) lo = mid + 1;
    else hi = mid;
  }
  let end = lo;
  const bottom = top + Math.max(viewportHeight, 240);
  while (end < count && offsets[end] < bottom) end++;

  const start = Math.max(0, lo - overscan);
  const last = Math.min(count, end + overscan);
  return { start, end: last, padTop: offsets[start], padBottom: total - offsets[last], total };
}
