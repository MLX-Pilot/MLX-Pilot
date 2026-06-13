/* MLX Pilot — Markdown rendering (core).
 *
 * Self-contained Markdown -> HTML renderer (inline + block: headings, lists,
 * quotes, code fences, tables, links, emphasis). Depends only on esc() from
 * dom.js for HTML escaping.
 */

import { esc } from './dom.js';

export function stashHtmlToken(tokens, html) {
  const token = `\uE000${tokens.length}\uE001`;
  tokens.push(html);
  return token;
}

export function restoreHtmlTokens(text, tokens) {
  return String(text || '').replace(/\uE000(\d+)\uE001/g, (_, index) => tokens[Number(index)] || '');
}

export function sanitizeHref(href) {
  const value = String(href || '').trim();
  if (/^(https?:|mailto:)/i.test(value)) return esc(value);
  return '#';
}

export function renderInlineMarkdown(text) {
  if (!text) return '';
  const tokens = [];
  let output = String(text);

  output = output.replace(/`([^`\n]+)`/g, (_, code) => stashHtmlToken(tokens, `<code>${esc(code)}</code>`));
  output = output.replace(/\[([^\]]+)\]\(([^)\s]+)(?:\s+"([^"]*)")?\)/g, (_, label, href, title) => {
    const titleAttr = title ? ` title="${esc(title)}"` : '';
    return stashHtmlToken(tokens, `<a href="${sanitizeHref(href)}" target="_blank" rel="noopener"${titleAttr}>${renderInlineMarkdown(label)}</a>`);
  });

  output = esc(output);
  output = output.replace(/(^|[\s(])\*\*([^*]+)\*\*(?=$|[\s).,!?:;])/g, '$1<strong>$2</strong>');
  output = output.replace(/(^|[\s(])__([^_]+)__(?=$|[\s).,!?:;])/g, '$1<strong>$2</strong>');
  output = output.replace(/(^|[\s(])\*([^*]+)\*(?=$|[\s).,!?:;])/g, '$1<em>$2</em>');
  output = output.replace(/(^|[\s(])_([^_]+)_(?=$|[\s).,!?:;])/g, '$1<em>$2</em>');
  output = output.replace(/~~([^~]+)~~/g, '<del>$1</del>');
  output = output.replace(/(https?:\/\/[^\s<]+)/g, '<a href="$1" target="_blank" rel="noopener">$1</a>');
  return restoreHtmlTokens(output, tokens);
}

export function splitTableRow(line) {
  return String(line || '')
    .trim()
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
    .map(cell => cell.trim());
}

export function isTableSeparator(line) {
  return /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?\s*$/.test(line || '');
}

export function isMarkdownBlockBoundary(line, nextLine) {
  const trimmed = String(line || '').trim();
  if (!trimmed) return true;
  if (/^\uE000\d+\uE001$/.test(trimmed)) return true;
  if (/^#{1,6}\s+/.test(trimmed)) return true;
  if (/^>\s?/.test(trimmed)) return true;
  if (/^([-+*]|\d+\.)\s+/.test(trimmed)) return true;
  if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmed)) return true;
  if (trimmed.includes('|') && isTableSeparator(nextLine || '')) return true;
  return false;
}

export function renderMarkdown(text) {
  const source = String(text || '').replace(/\r\n?/g, '\n').trim();
  if (!source) return '';

  const blockTokens = [];
  const normalized = source.replace(/```([\w.+-]*)\n?([\s\S]*?)```/g, (_, lang, code) => {
    const language = (lang || 'code').trim();
    const body = esc((code || '').replace(/\n$/, ''));
    return stashHtmlToken(blockTokens, `<div class="code-block"><div class="code-header"><span class="code-lang">${esc(language)}</span><button class="code-copy">Copiar</button></div><pre><code>${body}</code></pre></div>`);
  });

  const lines = normalized.split('\n');
  const blocks = [];

  for (let index = 0; index < lines.length;) {
    const line = lines[index];
    const trimmed = line.trim();

    if (!trimmed) {
      index += 1;
      continue;
    }

    if (/^\uE000\d+\uE001$/.test(trimmed)) {
      blocks.push(trimmed);
      index += 1;
      continue;
    }

    const heading = trimmed.match(/^(#{1,6})\s+(.+)$/);
    if (heading) {
      const level = heading[1].length;
      blocks.push(`<h${level}>${renderInlineMarkdown(heading[2].trim())}</h${level}>`);
      index += 1;
      continue;
    }

    if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmed)) {
      blocks.push('<hr>');
      index += 1;
      continue;
    }

    if (/^>\s?/.test(trimmed)) {
      const quoteLines = [];
      while (index < lines.length && /^>\s?/.test(lines[index].trim())) {
        quoteLines.push(lines[index].trim().replace(/^>\s?/, ''));
        index += 1;
      }
      blocks.push(`<blockquote>${renderMarkdown(quoteLines.join('\n'))}</blockquote>`);
      continue;
    }

    const listItem = trimmed.match(/^([-+*]|\d+\.)\s+(.+)$/);
    if (listItem) {
      const ordered = /\d+\./.test(listItem[1]);
      const tag = ordered ? 'ol' : 'ul';
      const items = [];
      while (index < lines.length) {
        const current = lines[index].trim().match(/^([-+*]|\d+\.)\s+(.+)$/);
        if (!current) break;
        items.push(`<li>${renderInlineMarkdown(current[2])}</li>`);
        index += 1;
      }
      blocks.push(`<${tag}>${items.join('')}</${tag}>`);
      continue;
    }

    if (trimmed.includes('|') && isTableSeparator(lines[index + 1] || '')) {
      const header = splitTableRow(lines[index]);
      index += 2;
      const rows = [];
      while (index < lines.length && lines[index].includes('|') && lines[index].trim()) {
        rows.push(splitTableRow(lines[index]));
        index += 1;
      }
      const headHtml = `<thead><tr>${header.map(cell => `<th>${renderInlineMarkdown(cell)}</th>`).join('')}</tr></thead>`;
      const bodyHtml = rows.length
        ? `<tbody>${rows.map(row => `<tr>${row.map(cell => `<td>${renderInlineMarkdown(cell)}</td>`).join('')}</tr>`).join('')}</tbody>`
        : '';
      blocks.push(`<div class="markdown-table-wrap"><table>${headHtml}${bodyHtml}</table></div>`);
      continue;
    }

    const paragraph = [];
    while (index < lines.length) {
      const current = lines[index];
      const next = lines[index + 1];
      if (!current.trim()) break;
      if (paragraph.length > 0 && isMarkdownBlockBoundary(current, next)) break;
      paragraph.push(current.trim());
      index += 1;
    }
    blocks.push(`<p>${renderInlineMarkdown(paragraph.join('\n')).replace(/\n/g, '<br>')}</p>`);
  }

  return restoreHtmlTokens(blocks.join(''), blockTokens);
}
