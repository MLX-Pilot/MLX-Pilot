import pptxgen from "pptxgenjs";
import { jsPDF } from "jspdf";
import { toPng } from "html-to-image";
import {
  previewHtml,
  baseCss,
  slideMarkup,
  safeCss,
  escapeHtml,
} from "./document.js";
const color = (s) => (s || "#000000").slice(1);
export async function renderFrame(doc, slide) {
  const frame = document.createElement("iframe");
  frame.setAttribute("sandbox", "allow-same-origin");
  frame.style.cssText =
    "position:fixed;left:-15000px;top:0;width:1280px;height:720px;border:0;";
  const ready = new Promise((resolve, reject) => {
    frame.onload = resolve;
    frame.onerror = reject;
  });
  frame.srcdoc = previewHtml(slide, doc);
  document.body.append(frame);
  try {
    await ready;
    await frame.contentDocument.fonts.ready;
    await Promise.all(
      [...frame.contentDocument.images].map((img) => img.decode()),
    );
    return frame;
  } catch (e) {
    frame.remove();
    throw e;
  }
}
export async function screenshot(frame) {
  return toPng(frame.contentDocument.querySelector(".slide"), {
    pixelRatio: 1.5,
    width: 1280,
    height: 720,
    skipFonts: true,
    cacheBust: false,
  });
}
export async function exportPdf(doc, onProgress = () => {}) {
  const pdf = new jsPDF({
    orientation: "landscape",
    unit: "px",
    format: [1280, 720],
    hotfixes: ["px_scaling"],
    compress: true,
  });
  for (let i = 0; i < doc.slides.length; i++) {
    const frame = await renderFrame(doc, doc.slides[i]);
    try {
      const png = await screenshot(frame);
      if (i) pdf.addPage([1280, 720], "landscape");
      pdf.addImage(png, "PNG", 0, 0, 1280, 720, undefined, "FAST");
    } finally {
      frame.remove();
    }
    onProgress((i + 1) / doc.slides.length);
  }
  pdf.setProperties({
    title: doc.title,
    subject: `MLX Pilot revision ${doc.revision}`,
  });
  return pdf.output("blob");
}
export async function buildPptx(
  doc,
  { raster = false, onProgress = () => {} } = {},
) {
  const pptx = new pptxgen();
  pptx.layout = "LAYOUT_WIDE";
  pptx.author = "MLX Pilot";
  pptx.subject = `Presentation Studio · revision ${doc.revision}`;
  pptx.title = doc.title;
  pptx.lang = "pt-BR";
  pptx.theme = {
    headFontFace: doc.theme.font,
    bodyFontFace: doc.theme.font,
    lang: "pt-BR",
  };
  const scale = 13.333333 / 1280;
  for (let i = 0; i < doc.slides.length; i++) {
    const s = doc.slides[i],
      slide = pptx.addSlide();
    slide.background = { color: color(s.background || doc.theme.background) };
    slide.addNotes(s.notes || "");
    if (raster) {
      const frame = await renderFrame(doc, s);
      try {
        slide.addImage({
          data: await screenshot(frame),
          x: 0,
          y: 0,
          w: 13.333333,
          h: 7.5,
        });
      } finally {
        frame.remove();
      }
    } else
      for (const e of s.elements) {
        const st = e.style || {},
          box = {
            x: e.x * scale,
            y: e.y * scale,
            w: e.w * scale,
            h: e.h * scale,
          };
        const font = {
          fontFace: doc.theme.font,
          fontSize: (st.fontSize || 32) * 0.75,
          bold: st.bold || false,
          italic: st.italic || false,
          color: color(st.color || doc.theme.foreground),
          align: st.align || "left",
          margin: 0,
          breakLine: false,
          transparency: (1 - (st.opacity ?? 1)) * 100,
        };
        if (e.kind === "text")
          slide.addText(e.text, {
            ...box,
            ...font,
            vertAlign: "top",
            ...(st.fill ? { fill: { color: color(st.fill) } } : {}),
          });
        else if (e.kind === "image") {
          let data = doc.assets.find((a) => a.id === e.asset_id)?.data_url;
          if (data) {
            const img = new Image();
            img.src = data;
            await img.decode();
            if (data.startsWith("data:image/webp")) {
              const canvas = document.createElement("canvas");
              canvas.width = img.naturalWidth;
              canvas.height = img.naturalHeight;
              canvas.getContext("2d").drawImage(img, 0, 0);
              data = canvas.toDataURL("image/png");
            }
            const ratio = Math.min(
                box.w / img.naturalWidth,
                box.h / img.naturalHeight,
              ),
              w = img.naturalWidth * ratio,
              h = img.naturalHeight * ratio;
            slide.addImage({
              data,
              x: box.x + (box.w - w) / 2,
              y: box.y + (box.h - h) / 2,
              w,
              h,
            });
          }
        } else if (e.kind === "shape")
          slide.addShape(st.radius ? "roundRect" : "rect", {
            ...box,
            fill: {
              color: color(st.fill || doc.theme.accent),
              transparency: (1 - (st.opacity ?? 1)) * 100,
            },
            line: { transparency: 100 },
            radius: st.radius || 0,
          });
        else if (e.kind === "chart")
          slide.addChart(
            pptx.ChartType.bar,
            [
              {
                name: e.text || "Série",
                labels: e.data.labels,
                values: e.data.values,
              },
            ],
            {
              ...box,
              catAxisLabelColor: color(doc.theme.foreground),
              valAxisLabelColor: color(doc.theme.muted),
              chartColors: [color(doc.theme.accent)],
              showLegend: false,
              showValue: true,
              showTitle: false,
              catAxisLabelFontSize: 16,
              valAxisLabelFontSize: 14,
              showCatName: false,
              showBorder: false,
              showSerName: false,
            },
          );
        else if (e.kind === "table")
          slide.addTable(e.data.rows, {
            ...box,
            ...font,
            fontSize: Math.min(font.fontSize, 20),
            border: { type: "solid", pt: 1, color: color(doc.theme.muted) },
            fill: color(doc.theme.background),
            margin: 8,
            autoPage: false,
          });
      }
    onProgress((i + 1) / doc.slides.length);
  }
  return pptx;
}
export function exportHtml(doc) {
  const pages = doc.slides.map((s) => slideMarkup(s, doc, true)).join("\n");
  return new Blob(
    [
      `<!doctype html><html lang="pt-BR"><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data: blob:; style-src 'unsafe-inline'; script-src 'unsafe-inline'; font-src data:;"><meta name="viewport" content="width=device-width,initial-scale=1"><title>${escapeHtml(doc.title)}</title><style>${baseCss}\n${safeCss(doc.css || "")}body{background:#111;overflow:hidden}.slide{display:none;transform-origin:top left}.slide.active{display:block}.slide.active[data-transition=fade]{animation:studio-fade .4s both}.slide.active[data-transition=slide]{animation:studio-page .4s both}@keyframes studio-page{from{opacity:0;margin-left:35px}}@media(prefers-reduced-motion:reduce){.slide.active{animation:none!important}}#hint{position:fixed;bottom:8px;right:15px;color:#aaa;font:12px Arial}@media print{body{overflow:visible}.slide{display:block!important;page-break-after:always;transform:none!important}#hint{display:none}}@page{size:1280px 720px;margin:0}</style></head><body tabindex="0">${pages}<div id="hint">← → slides · F tela cheia · Esc sair</div><script>const slides=[...document.querySelectorAll('.slide')];let index=0;function show(n){index=Math.max(0,Math.min(slides.length-1,n));slides.forEach((s,i)=>{s.classList.toggle('active',i===index);s.style.transform='scale('+Math.min(innerWidth/1280,innerHeight/720)+')';});}addEventListener('keydown',e=>{if(['ArrowRight',' ','PageDown'].includes(e.key))show(index+1);if(['ArrowLeft','PageUp'].includes(e.key))show(index-1);if(e.key.toLowerCase()==='f')document.documentElement.requestFullscreen?.().catch(()=>{});});addEventListener('resize',()=>show(index));show(0);document.body.focus();<\/script></body></html>`,
    ],
    { type: "text/html" },
  );
}
export function download(blob, name) {
  const url = URL.createObjectURL(blob),
    a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
