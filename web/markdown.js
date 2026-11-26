"use strict";
(() => {
  const listBreak = (source, start, end = start) => {
    const lineStart = source.lastIndexOf("\n", start - 1) + 1;
    const prefix = source.slice(lineStart, start);
    const marker = prefix.match(/^([ \t]*(?:>[ \t]*)*)([ \t]*)([-+*]|\d{1,9}[.)])([ \t]+)(?:\[[ xX]\]([ \t]+))?/);
    if (!marker) return null;
    let fence;
    for (const line of source.slice(0, lineStart).split("\n")) {
      const match = line.match(/^[ \t>]*(?:(?:[-+*]|\d{1,9}[.)])[ \t]+)?(`{3,}|~{3,})(.*)$/);
      if (!match) continue;
      if (!fence) fence = match[1];
      else if (match[1][0] === fence[0] && match[1].length >= fence.length && !match[2].trim()) fence = null;
    }
    if (fence) return null;
    const lineEnd = source.indexOf("\n", end);
    const remainder = source.slice(end, lineEnd < 0 ? source.length : lineEnd);
    if (!prefix.slice(marker[0].length).trim() && !remainder.trim()) {
      return { start: lineStart, end, text: marker[1].includes(">") ? marker[1] : "" };
    }
    const bullet = /^\d/.test(marker[3]) ? String(Number.parseInt(marker[3], 10) + 1) + marker[3].slice(-1) : marker[3];
    return { start, end, text: "\n" + marker[1] + marker[2] + bullet + marker[4] + (marker[5] ? "[ ]" + marker[5] : "") };
  };
  if (typeof module !== "undefined") module.exports = { listBreak };
  else window.EStickerMarkdown = { listBreak };
})();
