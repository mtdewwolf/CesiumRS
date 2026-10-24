// Keep the visible label count bounded as the milestone history grows.
const esc = (value) => String(value).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const formatDate = (date) => new Date(`${date}T00:00:00Z`).toLocaleDateString("en-US", {
  month: "short", day: "numeric", year: "numeric", timeZone: "UTC",
});

export default function progressHistory(history, style) {
  const first = history[0], latest = history.at(-1);
  const W = 640, H = 300, left = 60, right = 24, top = 88, bottom = 240;
  const plotWidth = W - left - right;
  const max = Math.max(20, Math.ceil(Math.max(...history.map((h) => h.percent)) / 20) * 20);
  const x = (i) => left + (history.length === 1 ? plotWidth / 2 : i * plotWidth / (history.length - 1));
  const y = (percent) => bottom - (bottom - top) * percent / max;
  const point = (h, i) => `${x(i).toFixed(1)},${y(h.percent).toFixed(1)}`;
  const dates = first.date === latest.date ? formatDate(first.date) : `${formatDate(first.date)} – ${formatDate(latest.date)}`;
  let svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-labelledby="history-title history-description">
${style}
<style>
  .history-heading{font-size:18px;font-weight:600}
  .history-value{font-size:32px;font-weight:650}
  .history-axis{font-size:15px}
  .history-line{fill:none;stroke:#0969da;stroke-width:3;stroke-linecap:round;stroke-linejoin:round}
  .history-point,.history-area{fill:#0969da}
  .history-area{fill-opacity:.08}
  @media (prefers-color-scheme:dark){.history-line{stroke:#58a6ff}.history-point,.history-area{fill:#58a6ff}}
</style>
<title id="history-title">CesiumRS implementation progress: ${latest.percent.toFixed(1)}%</title>
<desc id="history-description">${history.length} chronological milestones, ${esc(dates)}. Progress increased from ${first.percent.toFixed(1)}% to ${latest.percent.toFixed(1)}%. Latest milestone: ${esc(latest.milestone)}. The horizontal axis counts milestones; every milestone is retained in the line.</desc>
<text class="history-heading" x="16" y="28">Implementation progress</text>
<text class="dim" x="16" y="51">${history.length} ${history.length === 1 ? "milestone" : "milestones"} · ${esc(dates)}</text>
<text class="history-value" x="${W - right}" y="35" text-anchor="end">${latest.percent.toFixed(1)}%</text>
<text class="dim" x="${W - right}" y="60" text-anchor="end">implemented</text>
`;
  for (let percent = 0; percent <= max; percent += max <= 20 ? 5 : 20) {
    svg += `<rect class="bg" x="${left}" y="${y(percent).toFixed(1)}" width="${plotWidth}" height="1"/>
<text class="dim history-axis" x="${left - 12}" y="${(y(percent) + 5).toFixed(1)}" text-anchor="end">${percent}%</text>
`;
  }
  const points = history.map(point).join(" ");
  svg += `<polygon class="history-area" points="${x(0).toFixed(1)},${bottom} ${points} ${x(history.length - 1).toFixed(1)},${bottom}"/>
<polyline class="history-line" points="${points}"/>
`;
  history.forEach((h, i) => {
    const endpoint = i === 0 || i === history.length - 1;
    svg += `<circle${endpoint ? ' class="history-point"' : ' fill="transparent"'} cx="${x(i).toFixed(1)}" cy="${y(h.percent).toFixed(1)}" r="${endpoint ? 4 : 3}"><title>${i + 1}. ${esc(h.milestone)} · ${esc(h.date)} · ${h.percent.toFixed(1)}%</title></circle>
`;
  });
  // At most four short, horizontal ticks, even with hundreds of milestones.
  const ticks = [...new Set(Array.from({ length: 4 }, (_, i) => Math.round(i * (history.length - 1) / 3)))];
  for (const i of ticks) {
    svg += `<text class="dim history-axis" x="${x(i).toFixed(1)}" y="263" text-anchor="middle">${i + 1}</text>
`;
  }
  svg += `<text class="dim" x="${left + plotWidth / 2}" y="289" text-anchor="middle">Milestones, chronological order</text>
</svg>
`;
  return svg;
}
