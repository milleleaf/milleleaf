// Runs inside the page (Playwright `page.evaluate`). Walks an OX HTML deck and returns plain
// data: slides, frames with measured geometry, text runs, tables, and every loss it notices.
// Elements that need bytes from the Node side (images, SVG, canvas screenshots) are tagged with
// `data-ox-tmp` and returned as `asset` requests.
//
// Must stay self-contained: Playwright serialises this function's source into the page.

function extractDeck() {
  const issues = [];
  const deckEl = document.querySelector('[data-ox-deck]');
  const W = Number(deckEl?.dataset.oxWidth) || 1280;
  const H = Number(deckEl?.dataset.oxHeight) || 720;
  let slideEls = [...document.querySelectorAll('[data-ox-slide]')];
  if (!slideEls.length) {
    slideEls = [...document.querySelectorAll('section')];
    issues.push({ code: 'no-slide-markers', severity: 'warning', message: 'no [data-ox-slide] elements; treated every <section> as a slide' });
  }
  let tmp = 0;
  const tmpId = (el) => (el.dataset.oxTmp ||= String(++tmp));
  const r2 = (n) => Math.round(n * 100) / 100;

  const TEXT_BLOCKS = new Set(['H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'P', 'UL', 'OL', 'BLOCKQUOTE', 'PRE', 'FIGCAPTION', 'DL']);
  const MEDIA_UNSUPPORTED = new Set(['VIDEO', 'AUDIO', 'IFRAME', 'OBJECT', 'EMBED']);
  const SKIP = new Set(['SCRIPT', 'STYLE', 'TEMPLATE', 'NOSCRIPT', 'ASIDE']);

  function color(c) {
    if (!c) return null;
    const m = c.match(/rgba?\(([^)]+)\)/);
    if (!m) return c;
    const [r, g, b, a = 1] = m[1].split(/[\s,/]+/).filter(Boolean).map(Number);
    if (a === 0) return null;
    const hex = '#' + [r, g, b].map((x) => Math.round(x).toString(16).padStart(2, '0')).join('');
    return a < 1 ? hex + Math.round(a * 255).toString(16).padStart(2, '0') : hex;
  }
  function firstStop(bgImage) {
    const m = bgImage.match(/rgba?\([^)]+\)/);
    return m ? color(m[0]) : null;
  }
  function align(cs) {
    const a = cs.textAlign;
    if (a === 'start') return cs.direction === 'rtl' ? 'right' : 'left';
    if (a === 'end') return cs.direction === 'rtl' ? 'left' : 'right';
    return ['left', 'center', 'right', 'justify'].includes(a) ? a : 'left';
  }
  function fontOf(cs) {
    return cs.fontFamily.split(',')[0].trim().replace(/^["']|["']$/g, '');
  }
  const fonts = new Set();

  function styleChecks(el, cs, loss) {
    if (cs.boxShadow && cs.boxShadow !== 'none') loss.push({ code: 'shadow', severity: 'info', message: 'box-shadow dropped' });
    if (cs.filter && cs.filter !== 'none') loss.push({ code: 'filter', severity: 'warning', message: `CSS filter "${cs.filter}" dropped` });
    if (cs.backdropFilter && cs.backdropFilter !== 'none') loss.push({ code: 'filter', severity: 'warning', message: 'backdrop-filter dropped' });
    if (cs.mixBlendMode && cs.mixBlendMode !== 'normal') loss.push({ code: 'blend', severity: 'warning', message: `mix-blend-mode ${cs.mixBlendMode} dropped` });
    if (Number(cs.opacity) < 1) loss.push({ code: 'opacity', severity: 'info', message: `opacity ${cs.opacity} dropped` });
    if (cs.zIndex !== 'auto') loss.push({ code: 'z-index', severity: 'info', message: 'z-index ignored; frames stack in document order' });
  }

  function geometry(el, sr, loss) {
    const r = el.getBoundingClientRect();
    let g = { x: r.left - sr.left, y: r.top - sr.top, w: r.width, h: r.height };
    const t = getComputedStyle(el).transform;
    if (t && t !== 'none') {
      const m = t.match(/^matrix\(([^)]+)\)$/);
      if (m) {
        const [a, b, c, d] = m[1].split(',').map(Number);
        const sx = Math.hypot(a, b), sy = Math.hypot(c, d);
        const rot = (Math.atan2(b, a) * 180) / Math.PI;
        if (Math.abs(sx - 1) > 0.01 || Math.abs(sy - 1) > 0.01 || Math.abs(a * c + b * d) > 0.01) {
          loss.push({ code: 'transform', severity: 'warning', message: 'scale/skew transform flattened to its bounding box' });
        } else if (Math.abs(rot) > 0.01 && el.offsetWidth) {
          const cx = g.x + g.w / 2, cy = g.y + g.h / 2;
          g = { x: cx - el.offsetWidth / 2, y: cy - el.offsetHeight / 2, w: el.offsetWidth, h: el.offsetHeight, rotation: r2(rot) };
        }
      } else {
        loss.push({ code: 'transform', severity: 'warning', message: '3D transform flattened to its bounding box' });
      }
    }
    for (const k of ['x', 'y', 'w', 'h']) g[k] = r2(g[k]);
    if (g.x < -1 || g.y < -1 || g.x + g.w > W + 1 || g.y + g.h > H + 1) {
      loss.push({ code: 'off-slide', severity: 'warning', message: 'frame extends beyond the slide' });
    }
    return g;
  }

  // ---- text ---------------------------------------------------------------------------
  function runStyle(el) {
    const cs = getComputedStyle(el);
    fonts.add(fontOf(cs));
    const s = {};
    if (Number(cs.fontWeight) >= 600) s.bold = true;
    if (cs.fontStyle === 'italic' || cs.fontStyle === 'oblique') s.italic = true;
    if (cs.textDecorationLine.includes('underline')) s.underline = true;
    const c = color(cs.color);
    if (c) s.color = c;
    s.font = fontOf(cs);
    s.sizePt = Math.round(parseFloat(cs.fontSize) * 0.75 * 2) / 2;
    return s;
  }
  const sameStyle = (a, b) => ['bold', 'italic', 'underline', 'color', 'font', 'sizePt'].every((k) => a[k] === b[k]);

  // Collects runs under `node`, skipping nested lists (they become their own paragraphs).
  function collectRuns(node, runs, fields) {
    for (const ch of node.childNodes) {
      if (ch.nodeType === Node.TEXT_NODE) {
        const text = ch.textContent.replace(/\s+/g, ' ');
        if (!text) continue;
        const st = runStyle(ch.parentElement);
        const last = runs[runs.length - 1];
        if (last && last.text !== undefined && sameStyle(last, st)) last.text += text;
        else runs.push({ text, ...st });
      } else if (ch.nodeType === Node.ELEMENT_NODE) {
        if (ch.tagName === 'UL' || ch.tagName === 'OL' || SKIP.has(ch.tagName)) continue;
        if (ch.tagName === 'BR') { runs.push({ text: '\n', ...runStyle(ch.parentElement) }); continue; }
        if (getComputedStyle(ch).display === 'none') continue;
        if (ch.dataset.oxLink) {
          fields.push({ name: ch.dataset.oxField || null, path: ch.dataset.oxPath || null, link: ch.dataset.oxLink, mode: ch.dataset.oxLinkMode || 'live', text: ch.textContent.trim() });
          runs.push({ fieldIndex: fields.length - 1, style: runStyle(ch) });
          continue;
        }
        collectRuns(ch, runs, fields);
      }
    }
  }
  function trimRuns(runs) {
    const textRuns = runs.filter((r) => r.text !== undefined);
    if (textRuns.length) {
      textRuns[0].text = textRuns[0].text.replace(/^\s+/, '');
      const l = textRuns[textRuns.length - 1];
      l.text = l.text.replace(/\s+$/, '');
    }
    return runs.filter((r) => r.text === undefined || r.text.length);
  }
  function paragraph(el, fields, extra = {}) {
    const runs = [];
    collectRuns(el, runs, fields);
    const p = { runs: trimRuns(runs), align: align(getComputedStyle(el)), ...extra };
    // A rendered OX bullet paragraph starts with "• "; turn it back into a bullet.
    if (el.hasAttribute('data-ox-bullet')) {
      p.bullet = true;
      p.level = Number(el.dataset.oxLevel) || 0;
      if (p.runs[0]?.text?.startsWith('• ')) {
        p.runs[0].text = p.runs[0].text.slice(2);
        if (!p.runs[0].text) p.runs.shift();
      }
    }
    return p;
  }
  function paragraphsOf(el, fields, level = 0) {
    const tag = el.tagName;
    if (tag === 'UL' || tag === 'OL') {
      const out = [];
      for (const li of el.children) {
        if (li.tagName !== 'LI') continue;
        out.push(paragraph(li, fields, { bullet: tag === 'UL', numbered: tag === 'OL', level }));
        for (const sub of li.children) if (sub.tagName === 'UL' || sub.tagName === 'OL') out.push(...paragraphsOf(sub, fields, level + 1));
      }
      return out;
    }
    if (tag === 'PRE') {
      const st = runStyle(el);
      return el.textContent.split('\n').map((line) => ({ runs: line ? [{ text: line, ...st }] : [], align: 'left' }));
    }
    const blockKids = [...el.children].filter((c) => TEXT_BLOCKS.has(c.tagName) || c.tagName === 'DIV');
    if (blockKids.length && tag !== 'P' && !/^H\d$/.test(tag)) return blockKids.flatMap((c) => paragraphsOf(c, fields, level));
    return [paragraph(el, fields)];
  }
  function roleOf(el) {
    if (el.dataset.oxRole) return el.dataset.oxRole;
    if (/^H[12]$/.test(el.tagName)) return 'title';
    if (/^H[3-6]$/.test(el.tagName)) return 'subtitle';
    if (el.tagName === 'FIGCAPTION') return 'caption';
    return 'body';
  }

  // ---- tables -------------------------------------------------------------------------
  function tableOf(el) {
    const table = el.tagName === 'TABLE' ? el : el.querySelector('table');
    const rows = [];
    for (const tr of table ? table.rows : []) {
      const row = [];
      for (const cell of tr.cells) {
        const cs = getComputedStyle(cell);
        const c = { text: cell.textContent.replace(/\s+/g, ' ').trim() };
        if (cell.tagName === 'TH') c.header = true;
        if (cell.colSpan > 1) c.colspan = cell.colSpan;
        if (cell.rowSpan > 1) c.rowspan = cell.rowSpan;
        if (Number(cs.fontWeight) >= 600) c.bold = true;
        const bg = color(cs.backgroundColor);
        if (bg) c.fill = bg;
        c.align = align(cs);
        row.push(c);
      }
      rows.push(row);
    }
    const cs = getComputedStyle(table || el);
    fonts.add(fontOf(cs));
    return { rows, style: { font: fontOf(cs), sizePt: Math.round(parseFloat(cs.fontSize) * 0.75 * 2) / 2 } };
  }

  // ---- frames -------------------------------------------------------------------------
  function shapeOf(el, cs, loss) {
    let fill = color(cs.backgroundColor);
    if (cs.backgroundImage && cs.backgroundImage !== 'none') {
      const isGradient = cs.backgroundImage.includes('gradient');
      fill = fill || firstStop(cs.backgroundImage);
      loss.push({ code: isGradient ? 'gradient' : 'background-image', severity: 'warning',
        message: isGradient ? 'CSS gradient not carried; filled with its first colour stop' : 'CSS background image dropped' });
    }
    const bw = parseFloat(cs.borderTopWidth) || 0;
    const stroke = bw > 0 && cs.borderTopStyle !== 'none' ? color(cs.borderTopColor) : null;
    // `border-radius: 50%` computes to "50%": an ellipse. A pixel radius is a rounded rectangle
    // (a pill when it is half the short side).
    const rawRadius = cs.borderTopLeftRadius;
    const pct = rawRadius.endsWith('%') ? parseFloat(rawRadius) : 0;
    const r = el.getBoundingClientRect();
    const radius = pct ? (Math.min(r.width, r.height) * pct) / 100 : parseFloat(rawRadius) || 0;
    const kind = pct >= 50 ? 'ellipse' : radius > 0 ? 'roundRect' : 'rect';
    const s = { kind };
    if (fill) s.fill = fill;
    if (stroke) { s.stroke = stroke; s.strokeWidth = bw; }
    if (kind === 'roundRect') s.radius = radius;
    return s;
  }
  const hasBox = (cs) =>
    color(cs.backgroundColor) || (cs.backgroundImage && cs.backgroundImage !== 'none') ||
    ((parseFloat(cs.borderTopWidth) || 0) > 0 && cs.borderTopStyle !== 'none' && color(cs.borderTopColor));
  const hasDirectText = (el) => [...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent.trim());

  const slides = [];
  for (const [si, slide] of slideEls.entries()) {
    const sr = slide.getBoundingClientRect();
    const scs = getComputedStyle(slide);
    const sIssues = [];
    if (Math.abs(sr.width - W) > 1 || Math.abs(sr.height - H) > 1) {
      sIssues.push({ code: 'slide-size', severity: 'warning', message: `slide is ${r2(sr.width)}×${r2(sr.height)}, deck is ${W}×${H}` });
    }
    let background = color(scs.backgroundColor);
    if (scs.backgroundImage && scs.backgroundImage !== 'none') {
      background = firstStop(scs.backgroundImage) || background;
      sIssues.push({ code: 'gradient', severity: 'warning', message: 'slide background gradient/image not carried; used its first colour' });
    }
    const notes = [...slide.querySelectorAll('[data-ox-notes]')].map((n) => n.textContent.trim()).join('\n') || null;
    const frames = [];
    const push = (el, kind, data, loss) => {
      frames.push({ kind, name: el.dataset.oxName || el.id || null, path: el.dataset.oxPath || null,
        link: el.dataset.oxLink || null, linkMode: el.dataset.oxLinkMode || 'live', ...data, loss });
    };

    const walk = (parent) => {
      for (const el of parent.children) {
        if (SKIP.has(el.tagName) || el.hasAttribute('data-ox-notes')) continue;
        const cs = getComputedStyle(el);
        if (cs.display === 'none' || cs.visibility === 'hidden') continue;
        const loss = [];
        styleChecks(el, cs, loss);
        const explicit = el.dataset.oxFrame;
        const tag = el.tagName.toUpperCase();
        const g = () => geometry(el, sr, loss);

        // A chart drawn from project data: keep the link and the chart spec, not the picture.
        if (el.dataset.oxChart && el.dataset.oxLink) {
          const values = (el.dataset.oxValues || '').split(',').map((v) => v.trim()).filter(Boolean);
          const title = el.getAttribute('aria-label');
          push(el, 'chart', { geom: g(), chart: { kind: el.dataset.oxChart, category: el.dataset.oxCategory || '', values, ...(title && title !== 'chart' ? { title } : {}) } }, loss);
          continue;
        }
        if (explicit === 'text' || (!explicit && TEXT_BLOCKS.has(tag))) {
          const fields = [];
          const paragraphs = paragraphsOf(el, fields);
          if (el.scrollHeight > el.clientHeight + 2 && el.clientHeight > 0 && cs.overflow !== 'visible') {
            loss.push({ code: 'overflow', severity: 'warning', message: 'text overflows its box' });
          }
          if (hasBox(cs)) push(el, 'shape', { geom: g(), shape: shapeOf(el, cs, loss) }, []);
          push(el, 'text', { geom: g(), role: roleOf(el), paragraphs, fields }, loss);
          continue;
        }
        if (explicit === 'table' || (!explicit && tag === 'TABLE')) {
          push(el, 'table', { geom: g(), table: tableOf(el) }, loss);
          continue;
        }
        if (tag === 'IMG') {
          push(el, explicit === 'vector' ? 'vector' : 'picture', { geom: g(), alt: el.alt || null, asset: { tmp: tmpId(el), src: el.currentSrc || el.src } }, loss);
          continue;
        }
        if (tag === 'SVG') {
          let svg = el.outerHTML;
          if (!/xmlns=/.test(svg)) svg = svg.replace(/^<svg/, '<svg xmlns="http://www.w3.org/2000/svg"');
          push(el, 'vector', { geom: g(), alt: el.getAttribute('aria-label') || null, asset: { tmp: tmpId(el), svg } }, loss);
          continue;
        }
        if (tag === 'CANVAS') {
          loss.push({ code: 'canvas', severity: 'info', message: 'canvas has no structure; captured as a picture' });
          push(el, 'picture', { geom: g(), alt: el.getAttribute('aria-label') || null, asset: { tmp: tmpId(el), screenshot: true } }, loss);
          continue;
        }
        if (MEDIA_UNSUPPORTED.has(tag)) {
          loss.push({ code: 'unsupported-element', severity: 'warning', message: `<${tag.toLowerCase()}> isn't in the profile` });
          push(el, 'unsupported', { geom: g(), asset: { tmp: tmpId(el), screenshot: true } }, loss);
          continue;
        }
        if (explicit === 'shape' || hasBox(cs)) {
          push(el, 'shape', { geom: g(), shape: shapeOf(el, cs, loss) }, loss);
          if (hasDirectText(el)) {
            const fields = [];
            push(el, 'text', { geom: g(), role: roleOf(el), paragraphs: paragraphsOf(el, fields), fields }, []);
            frames[frames.length - 1].name = frames[frames.length - 1].name ? frames[frames.length - 1].name + '-text' : null;
            continue;
          }
          walk(el);
          continue;
        }
        if (hasDirectText(el)) {
          const fields = [];
          push(el, 'text', { geom: g(), role: roleOf(el), paragraphs: paragraphsOf(el, fields), fields }, loss);
          continue;
        }
        if (loss.length) frames.push({ kind: 'container-loss', loss, name: null });
        walk(el);
      }
    };
    walk(slide);
    slides.push({ index: si, name: slide.dataset.oxName || slide.id || null, path: slide.dataset.oxPath || null, background, notes, frames, loss: sIssues });
  }
  return { W, H, title: document.title || null, deckPath: deckEl?.dataset.oxPath || null, slides, issues, fonts: [...fonts] };
}

module.exports = { extractDeck };
