import { convertFileSrc } from '@tauri-apps/api/core';
import katex from 'katex';

export interface VizBlockData {
  id: string;
  title: string;
  code: string;
  type: string;
}

export function escapeHtml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

export function highlightCode(code: string, _lang: string): string {
  const escaped = escapeHtml(code);
  const tokens: string[] = [];
  const hold = (html: string) => {
    const idx = tokens.length;
    tokens.push(html);
    return `@@HL_${idx}@@`;
  };

  let out = escaped
    .replace(/(\/\/[^\n]*|#[^\n]*)/g, (m) => hold(`<span class="text-[#6B7280] italic">${m}</span>`))
    .replace(/(&quot;[^&\n]*?&quot;|&#39;[^&\n]*?&#39;)/g, (m) => hold(`<span class="text-[#A5D6FF]">${m}</span>`));

  out = out
    .replace(/\b(const|let|var|function|return|if|else|for|while|loop|import|from|export|default|class|interface|type|extends|implements|new|this|async|await|try|catch|finally|throw|fn|pub|struct|enum|impl|match|mut|use|crate|mod|def|self|None|True|False|package|func|select|switch|case|break|continue|where|unsafe|extern)\b/g, '<span class="text-[#FF7B72] font-medium">$1</span>')
    .replace(/\b(true|false|null|undefined|nil|Some|Ok|Err)\b/g, '<span class="text-[#79C0FF]">$1</span>')
    .replace(/\b(0x[0-9a-fA-F]+|\d+(?:\.\d+)?)\b/g, '<span class="text-[#FFA657]">$1</span>');

  return out.replace(/@@HL_(\d+)@@/g, (_, i) => tokens[Number(i)] || '');
}

export function extractVizBlocks(content: string): { cleanContent: string; vizBlocks: VizBlockData[] } {
  const vizBlocks: VizBlockData[] = [];
  let blockIndex = 0;

  const cleanContent = content.replace(
    /^```(viz|html:viz|chart|interactive)[ \t]*([^\n]*)\r?\n([\s\S]*?)^```[ \t]*$/gm,
    (_, type, rawTitle, code) => {
      const id = `viz-${Date.now()}-${blockIndex++}`;
      const title = rawTitle.trim() || `Interactive Visualization #${blockIndex}`;
      const cleanCode = code.trim();
      vizBlocks.push({ id, title, code: cleanCode, type: type.trim() });
      return `<div class="interactive-viz-mount my-4" data-viz-id="${id}" data-viz-title="${escapeHtml(title)}"></div>`;
    }
  );

  return { cleanContent, vizBlocks };
}

export function renderKatex(expr: string, displayMode: boolean): string {
  try {
    return katex.renderToString(expr.trim(), {
      displayMode,
      throwOnError: false,
      strict: false,
      output: 'htmlAndMathml'
    });
  } catch {
    return `<code class="px-1.5 py-0.5 rounded-[4px] font-mono text-[12px] bg-white/[0.07] text-[#E5E5E8]">${escapeHtml(expr)}</code>`;
  }
}


function resolveMediaSrc(raw: string): string {
  const clean = raw.replace(/&amp;/g, '&').trim();
  if (/^(https?:|data:|blob:|asset:)/i.test(clean)) {
    return clean;
  }
  const localPath = clean.startsWith('file:///')
    ? decodeURIComponent(clean.replace(/^file:\/\/\//i, '')).replace(/\//g, '\\')
    : clean;
  try {
    return convertFileSrc(localPath);
  } catch {
    return clean;
  }
}

function buildAudioVoiceHtml(src: string): string {
  const resolved = resolveMediaSrc(src);
  const bars = [35, 62, 48, 85, 54, 92, 42, 74, 88, 50, 68, 95, 44, 78, 60, 82, 46, 70, 52, 38];
  const barHtml = bars
    .map(
      (h, idx) =>
        `<span class="omni-wave-bar w-[2.5px] rounded-full bg-white/25 transition-colors duration-150" data-bar-idx="${idx}" style="height:${Math.round(h * 0.2)}px"></span>`
    )
    .join('');
  return `<div class="omni-voice-msg inline-flex items-center gap-2.5 my-2 py-1.5 pr-3 pl-1.5 rounded-full bg-white/[0.05] max-w-[280px] select-none"><audio class="omni-voice-audio hidden" preload="metadata" src="${escapeHtml(resolved)}"></audio><button type="button" class="omni-voice-play w-7 h-7 rounded-full bg-[#EDEDEF] hover:bg-white text-[#111113] flex items-center justify-center shrink-0 transition-transform active:scale-95" aria-label="Play audio"><svg class="omni-icon-play w-3.5 h-3.5 ml-0.5" viewBox="0 0 24 24" fill="currentColor"><polygon points="6 4 20 12 6 20 6 4"></polygon></svg><svg class="omni-icon-pause w-3.5 h-3.5 hidden" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16"></rect><rect x="14" y="4" width="4" height="16"></rect></svg></button><div class="omni-voice-wave flex items-center gap-[3px] h-5 cursor-pointer">${barHtml}</div><span class="omni-voice-time font-mono text-[10.5px] text-[#8E8E95] shrink-0">0:00</span></div>`;
}

function buildVideoHtml(src: string): string {
  const resolved = resolveMediaSrc(src);
  return `<video src="${escapeHtml(resolved)}" controls playsinline preload="metadata" class="my-2 max-w-[420px] max-h-[300px] rounded-[12px] block bg-black"></video>`;
}

function buildImageHtml(src: string, alt: string): string {
  const resolved = resolveMediaSrc(src);
  return `<img src="${escapeHtml(resolved)}" alt="${escapeHtml(alt)}" loading="lazy" class="my-2 max-w-[320px] max-h-[320px] rounded-[12px] object-cover block" />`;
}

function markHighlightClass(color: string): string {
  switch (color.toLowerCase()) {
    case 'yellow':
      return 'bg-[#FACC15]/20 text-[#FEF08A]';
    case 'green':
      return 'bg-[#22C55E]/20 text-[#BBF7D0]';
    case 'red':
      return 'bg-[#EF4444]/20 text-[#FECACA]';
    case 'blue':
      return 'bg-[#38BDF8]/20 text-[#BAE6FD]';
    default:
      return 'bg-[#A855F7]/25 text-[#E9D5FF]';
  }
}

export function renderMarkdown(content: string): string {
  if (!content) return '';

  const slots: string[] = [];
  const hold = (html: string): string => {
    const idx = slots.length;
    slots.push(html);
    return `\u0000SLOT_${idx}\u0000`;
  };

  let text = content.replace(/\r\n/g, '\n');

  text = text.replace(/<div class="interactive-viz-mount[^>]*><\/div>/g, (m) => hold(m));

  text = text.replace(/^```([a-zA-Z0-9_+\-.]*)[ \t]*\n([\s\S]*?)^```[ \t]*$/gm, (_, rawLang, code) => {
    const lang = (rawLang || '').trim().toLowerCase() || 'text';
    const trimmedCode = code.replace(/\n$/, '');
    const codeId = `code-${Math.random().toString(36).substring(2, 9)}`;
    const lineCount = trimmedCode.split('\n').length;
    const lineNumbers = Array.from(
      { length: lineCount },
      (_, i) => `<span class="select-none text-[#55555C] text-right pr-3 block text-[11px] leading-5">${i + 1}</span>`
    ).join('');
    const highlighted = highlightCode(trimmedCode, lang);

    return hold(`
      <div class="code-block my-3 rounded-[8px] border border-white/[0.08] bg-[#121214] overflow-hidden text-sm group">
        <div class="flex items-center justify-between px-3.5 py-1.5 bg-[#18181B] border-b border-white/[0.06]">
          <span class="text-[11px] font-medium tracking-wider text-[#8E8E95] uppercase font-mono">${escapeHtml(lang)}</span>
          <button
            type="button"
            class="copy-code-btn flex items-center gap-1.5 px-2 py-0.5 rounded-[4px] text-[11px] text-[#8E8E95] hover:text-[#EDEDEF] hover:bg-white/[0.06] transition-colors"
            data-clipboard-target="${codeId}"
          >
            <svg class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
            </svg>
            <span class="copy-label">Copy</span>
          </button>
        </div>
        <div class="flex p-3 overflow-x-auto text-[12px] font-mono leading-5">
          <div class="line-numbers select-none border-r border-white/[0.06] mr-3">${lineNumbers}</div>
          <pre class="code-content flex-1 text-[#E5E5E8] overflow-x-auto whitespace-pre"><code>${highlighted}</code></pre>
          <textarea class="hidden-raw-code sr-only" id="${codeId}">${escapeHtml(trimmedCode)}</textarea>
        </div>
      </div>
    `.trim());
  });

  text = text.replace(/`([^`\n]+)`/g, (_, code) => {
    return hold(`<code class="px-1.5 py-0.5 rounded-[4px] text-[12px] font-mono bg-white/[0.07] text-[#E5E5E8]">${escapeHtml(code)}</code>`);
  });

  text = text.replace(/\$\$([\s\S]+?)\$\$/g, (_, math) => {
    return hold(`<div class="katex-block my-3 text-center overflow-x-auto">${renderKatex(math, true)}</div>`);
  });

  text = text.replace(/\\\[([\s\S]+?)\\\]/g, (_, math) => {
    return hold(`<div class="katex-block my-3 text-center overflow-x-auto">${renderKatex(math, true)}</div>`);
  });

  text = text.replace(/\\\(([^\n]+?)\\\)/g, (_, math) => {
    return hold(renderKatex(math, false));
  });

  text = text.replace(/(^|[^\\])\$([^\s$\n](?:[^$\n]*?[^\s$\n])?)\$(?!\d)/g, (_, pre, math) => {
    return `${pre}${hold(renderKatex(math, false))}`;
  });

  const blockquotes: string[] = [];
  text = text.replace(/^>\s?\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]\s*(.*)$/gm, (_, type, firstLine) => {
    const idx = blockquotes.length;
    blockquotes.push(`<div class="my-2.5 p-3 rounded-[8px] border-l-2 border-white/[0.25] bg-[#18181B]"><div class="font-mono text-[10.5px] uppercase tracking-wider text-[#A3A3AA] mb-1">${type}</div><div class="text-[13.5px] text-[#E5E5E8]">${escapeHtml(firstLine)}</div></div>`);
    return `\u0000BQ_${idx}\u0000`;
  });

  text = text.replace(/^>\s?(.*)$/gm, (_, line) => {
    const idx = blockquotes.length;
    blockquotes.push(`<blockquote class="border-l-2 border-white/[0.2] pl-3 py-0.5 my-2 text-[#A3A3AA]">${escapeHtml(line)}</blockquote>`);
    return `\u0000BQ_${idx}\u0000`;
  });

  text = text.replace(/((?:\|[^\n]+\|\n)+)/g, (tableMatch) => {
    const rawLines = tableMatch.trim().split('\n');
    if (rawLines.length < 2) return tableMatch;

    const parseRow = (line: string) =>
      line.trim().replace(/^\|/, '').replace(/\|$/, '').split('|').map((c) => c.trim());

    const headerCols = parseRow(rawLines[0]);
    const isDivider = (line: string) => /^\|?(\s*:?-+:?\s*\|?)+$/.test(line.trim());
    const startIndex = isDivider(rawLines[1]) ? 2 : 1;

    const formatCell = (cell: string) =>
      escapeHtml(cell)
        .replace(/\*\*([^*]+)\*\*/g, '<strong class="font-semibold text-[#EDEDEF]">$1</strong>');

    const thead = `<thead><tr class="border-b border-white/[0.08] bg-[#18181B]">${headerCols
      .map((c) => `<th class="px-3.5 py-2 text-[11.5px] font-medium text-[#A3A3AA] text-left">${formatCell(c)}</th>`)
      .join('')}</tr></thead>`;

    const tbodyRows: string[] = [];
    for (let i = startIndex; i < rawLines.length; i++) {
      if (!rawLines[i].trim()) continue;
      const cols = parseRow(rawLines[i]);
      tbodyRows.push(
        `<tr class="border-b border-white/[0.05] hover:bg-white/[0.02] transition-colors">${cols
          .map((c) => `<td class="px-3.5 py-2 text-[13px] text-[#E5E5E8]">${formatCell(c)}</td>`)
          .join('')}</tr>`
      );
    }

    return hold(`
      <div class="table-container overflow-x-auto my-3 border border-white/[0.08] rounded-[8px] bg-[#121214]">
        <table class="w-full text-left border-collapse">${thead}<tbody>${tbodyRows.join('')}</tbody></table>
      </div>
    `.trim());
  });

  text = text.replace(/^[ \t]*(?:---+|___+|\*\*\*+)[ \t]*$/gm, () =>
    hold('<hr class="my-4 border-0 border-t border-white/[0.08]" />')
  );

  text = text.replace(/<audio\b[^>]*\bsrc=["']([^"']+)["'][^>]*>(?:<\/audio>)?/gi, (_, src) =>
    hold(buildAudioVoiceHtml(src))
  );
  text = text.replace(/<video\b[^>]*\bsrc=["']([^"']+)["'][^>]*>(?:<\/video>)?/gi, (_, src) =>
    hold(buildVideoHtml(src))
  );
  text = text.replace(/<img\b[^>]*\bsrc=["']([^"']+)["'][^>]*>/gi, (m, src) => {
    const altMatch = m.match(/\balt=["']([^"']*)["']/i);
    return hold(buildImageHtml(src, altMatch ? altMatch[1] : ''));
  });
  text = text.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (_, alt, src) => {
    const clean = String(src).trim();
    if (/\.(mp3|wav|ogg|m4a|opus|flac)(?:\?.*)?$/i.test(clean)) {
      return hold(buildAudioVoiceHtml(clean));
    }
    if (/\.(mp4|webm|mov|mkv)(?:\?.*)?$/i.test(clean)) {
      return hold(buildVideoHtml(clean));
    }
    return hold(buildImageHtml(clean, String(alt || '')));
  });
  text = text.replace(/<mark\b([^>]*)>([\s\S]*?)<\/mark>/gi, (_, attrs, inner) => {
    const colMatch = String(attrs).match(/(?:data-color|color)=["']([a-zA-Z]+)["']/i);
    const cls = markHighlightClass(colMatch ? colMatch[1] : 'purple');
    return hold(`<mark class="px-1 py-0.5 rounded-[4px] font-medium ${cls}">${escapeHtml(inner)}</mark>`);
  });

  text = escapeHtml(text);

  text = text.replace(/\u0000BQ_(\d+)\u0000/g, (_, i) => blockquotes[Number(i)] || '');

  text = text.replace(/^###### (.*)$/gm, '<h6 class="text-xs font-semibold uppercase tracking-wider text-[#A3A3AA] mt-3 mb-1">$1</h6>');
  text = text.replace(/^##### (.*)$/gm, '<h5 class="text-[13px] font-semibold text-[#EDEDEF] mt-3 mb-1">$1</h5>');
  text = text.replace(/^#### (.*)$/gm, '<h4 class="text-[14px] font-semibold text-[#EDEDEF] mt-3.5 mb-1.5">$1</h4>');
  text = text.replace(/^### (.*)$/gm, '<h3 class="text-[15px] font-semibold text-[#EDEDEF] mt-4 mb-1.5">$1</h3>');
  text = text.replace(/^## (.*)$/gm, '<h2 class="text-[16px] font-semibold text-[#EDEDEF] mt-5 mb-2 border-b border-white/[0.08] pb-1">$1</h2>');
  text = text.replace(/^# (.*)$/gm, '<h1 class="text-[18px] font-bold text-[#EDEDEF] mt-5 mb-2.5">$1</h1>');

  text = text.replace(/\*\*([^\n*]+?)\*\*/g, '<strong class="font-semibold text-[#EDEDEF]">$1</strong>');
  text = text.replace(/\*([^\n*]+?)\*/g, '<em class="italic text-[#B3B3BA]">$1</em>');
  text = text.replace(/~~([^\n~]+?)~~/g, '<del class="line-through text-[#66666D]">$1</del>');
  text = text.replace(/==([^\n=]+?)==/g, '<mark class="px-1 py-0.5 rounded-[4px] font-medium bg-[#A855F7]/25 text-[#E9D5FF]">$1</mark>');

  text = text.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, label, rawUrl) => {
    const cleanUrl = String(rawUrl).replace(/&amp;/g, '&').trim();
    if (/\.(mp3|wav|ogg|m4a|opus|flac)(?:\?.*)?$/i.test(cleanUrl)) {
      return buildAudioVoiceHtml(cleanUrl);
    }
    if (/\.(mp4|webm|mov|mkv)(?:\?.*)?$/i.test(cleanUrl)) {
      return buildVideoHtml(cleanUrl);
    }
    if (cleanUrl.startsWith('file:///') || /^[a-zA-Z]:[\\/]/.test(cleanUrl)) {
      const cleanPath = cleanUrl.replace(/^file:\/\/\//, '').replace(/\//g, '\\');
      return `<a href="#" data-open-path="${escapeHtml(cleanPath)}" class="omni-file-link text-[#EDEDEF] underline decoration-white/30 hover:text-white hover:decoration-white transition-colors">${label}</a>`;
    }
    return `<a href="${escapeHtml(cleanUrl)}" target="_blank" rel="noopener noreferrer" class="text-[#EDEDEF] underline decoration-white/30 hover:text-white transition-colors">${label}</a>`;
  });

  text = text.replace(/^[ \t]*[-*+]\s+\[ \]\s+(.*)$/gm, '<div class="flex items-center gap-2 my-1"><span class="w-3.5 h-3.5 rounded-[3px] border border-white/[0.2] inline-block shrink-0"></span><span class="text-[13.5px] text-[#E5E5E8]">$1</span></div>');
  text = text.replace(/^[ \t]*[-*+]\s+\[x\]\s+(.*)$/gm, '<div class="flex items-center gap-2 my-1"><span class="w-3.5 h-3.5 rounded-[3px] bg-white/[0.15] border border-white/[0.3] inline-flex items-center justify-center text-[10px] text-[#EDEDEF] shrink-0">✓</span><span class="text-[13.5px] text-[#8E8E95] line-through">$1</span></div>');

  text = text.replace(/^[ \t]*[-*+]\s+(.*)$/gm, '<li class="ml-5 list-disc text-[13.5px] text-[#E5E5E8] my-0.5">$1</li>');
  text = text.replace(/^[ \t]*\d+\.\s+(.*)$/gm, '<li class="ml-5 list-decimal text-[13.5px] text-[#E5E5E8] my-0.5">$1</li>');

  text = text.replace(/\n{3,}/g, '\n\n');
  const paragraphs = text.split('\n\n').map((block) => {
    const trimmed = block.trim();
    if (!trimmed) return '';
    if (
      trimmed.startsWith('<h') ||
      trimmed.startsWith('<li') ||
      trimmed.startsWith('<blockquote') ||
      trimmed.startsWith('<div') ||
      /^\u0000SLOT_\d+\u0000$/.test(trimmed)
    ) {
      return trimmed;
    }
    return `<p class="text-[13.5px] leading-relaxed text-[#E5E5E8] my-2">${trimmed.replace(/\n/g, '<br>')}</p>`;
  });

  let html = paragraphs.filter(Boolean).join('\n');
  for (let pass = 0; pass < 6 && /\u0000SLOT_\d+\u0000/.test(html); pass++) {
    html = html.replace(/\u0000SLOT_(\d+)\u0000/g, (_, i) => slots[Number(i)] || '');
  }
  return html;
}
