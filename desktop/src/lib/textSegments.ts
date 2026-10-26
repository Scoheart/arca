export const TEXT_SEGMENT_CHARS = 32 * 1024;
const TEXT_SEGMENT_LINES = 800;

// Only offsets are retained. Every UTF-16 code unit remains accessible, and a
// segment boundary never splits a surrogate pair.
export function textSegments(text: string): number[] {
  const offsets = [0];
  let start = 0;
  let lines = 0;
  for (let index = 0; index < text.length; index++) {
    if (text.charCodeAt(index) === 10) lines++;
    if (index + 1 - start < TEXT_SEGMENT_CHARS && lines < TEXT_SEGMENT_LINES) continue;
    const char = text.charCodeAt(index);
    if (char >= 0xd800 && char <= 0xdbff && index + 1 < text.length) index++;
    if (index + 1 < text.length) offsets.push(index + 1);
    start = index + 1; lines = 0;
  }
  return offsets;
}
