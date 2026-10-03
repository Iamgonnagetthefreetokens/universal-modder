// Tiny vector flag DSL. Every national flag in the game is a list of ops drawn into a
// 0..1 unit square, so flags scale cleanly at any DPR and need no image assets.
//
// ops:
//   ['h', y0, y1, color]                 horizontal band
//   ['v', x0, x1, color]                 vertical band
//   ['r', x, y, w, h, color]             rectangle
//   ['c', cx, cy, r, color]              circle
//   ['p', [[x,y],...], color]            polygon
//   ['s', cx, cy, r, points, color]      star (inner radius 0.382r)
//   ['l', x0, y0, x1, y1, color, weight] line
//   cross(...) helper       -> St George / Nordic crosses as rect ops

export const hb = (...colors) => {
  const ops = [];
  const n = colors.length;
  for (let i = 0; i < n; i++) ops.push(['h', i / n, (i + 1) / n, colors[i]]);
  return ops;
};

export const vb = (...colors) => {
  const ops = [];
  const n = colors.length;
  for (let i = 0; i < n; i++) ops.push(['v', i / n, (i + 1) / n, colors[i]]);
  return ops;
};

// stripes with explicit weights, e.g. hws(['#00247D', 1, '#fff', 1, '#CF142B', 2, ...])
export const hws = (list) => {
  const total = list.filter((_, i) => i % 2 === 1).reduce((a, b) => a + b, 0);
  const ops = [];
  let acc = 0;
  for (let i = 0; i < list.length; i += 2) {
    const h = list[i + 1] / total;
    ops.push(['h', acc, acc + h, list[i]]);
    acc += h;
  }
  return ops;
};

// n alternating horizontal stripes starting with `first`
export const altH = (n, first, second) => {
  const ops = [];
  for (let i = 0; i < n; i++) ops.push(['h', i / n, (i + 1) / n, i % 2 === 0 ? first : second]);
  return ops;
};

export function drawFlag(ctx, ops, x, y, w, h, { radius = 0, outline = true } = {}) {
  ctx.save();
  ctx.beginPath();
  if (radius > 0 && ctx.roundRect) ctx.roundRect(x, y, w, h, radius);
  else ctx.rect(x, y, w, h);
  ctx.clip();
  ctx.fillStyle = '#f4f4f4';
  ctx.fillRect(x, y, w, h);
  for (const op of ops || []) paintOp(ctx, op, x, y, w, h);
  ctx.restore();
  if (outline) {
    ctx.save();
    ctx.strokeStyle = 'rgba(0,0,0,0.35)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    if (radius > 0 && ctx.roundRect) ctx.roundRect(x + 0.5, y + 0.5, w - 1, h - 1, radius);
    else ctx.rect(x + 0.5, y + 0.5, w - 1, h - 1);
    ctx.stroke();
    ctx.restore();
  }
}

function paintOp(ctx, op, x, y, w, h) {
  const X = (u) => x + u * w;
  const Y = (u) => y + u * h;
  ctx.fillStyle = op[3] || op[op.length - 1];
  switch (op[0]) {
    case 'h':
      ctx.fillStyle = op[3];
      ctx.fillRect(X(0), Y(op[1]), w, (op[2] - op[1]) * h + 0.6);
      break;
    case 'v':
      ctx.fillStyle = op[3];
      ctx.fillRect(X(op[1]), Y(0), (op[2] - op[1]) * w + 0.6, h);
      break;
    case 'r':
      ctx.fillStyle = op[5];
      ctx.fillRect(X(op[1]), Y(op[2]), op[3] * w, op[4] * h);
      break;
    case 'c':
      ctx.fillStyle = op[4];
      ctx.beginPath();
      ctx.arc(X(op[1]), Y(op[2]), op[3] * Math.min(w, h), 0, Math.PI * 2);
      ctx.fill();
      break;
    case 'p':
      ctx.fillStyle = op[2];
      ctx.beginPath();
      op[1].forEach(([px, py], i) => (i ? ctx.lineTo(X(px), Y(py)) : ctx.moveTo(X(px), Y(py))));
      ctx.closePath();
      ctx.fill();
      break;
    case 's':
      star(ctx, X(op[1]), Y(op[2]), op[3] * Math.min(w, h), op[4] | 0, 5);
      ctx.fillStyle = op[5];
      ctx.fill();
      break;
    case 'l':
      ctx.strokeStyle = op[5];
      ctx.lineWidth = op[6] * h;
      ctx.beginPath();
      ctx.moveTo(X(op[1]), Y(op[2]));
      ctx.lineTo(X(op[3]), Y(op[4]));
      ctx.stroke();
      break;
    default:
      break;
  }
}

// a proper cross: arm thickness t, centre (cx,cy), full width/height
export function cross(cx, cy, t, color, { vOff = 0 } = {}) {
  return [
    ['r', 0, cy - t, 1, 2 * t, color],
    ['r', cx - t + vOff, 0, 2 * t, 1, color],
  ];
}

function star(ctx, cx, cy, r, points, innerRatio = 0.4) {
  ctx.beginPath();
  const n = points < 3 ? 5 : points;
  for (let i = 0; i < n * 2; i++) {
    const rad = i % 2 ? r * innerRatio : r;
    const a = (Math.PI / n) * i - Math.PI / 2;
    const px = cx + Math.cos(a) * rad;
    const py = cy + Math.sin(a) * rad;
    if (i === 0) ctx.moveTo(px, py);
    else ctx.lineTo(px, py);
  }
  ctx.closePath();
}

export { star };
