/**
 * Builds a meeting notes PDF (summary + timestamped transcript) entirely in the webview.
 *
 * pdfmake embeds and subsets the bundled Noto fonts, so Vietnamese diacritics and
 * Chinese characters render without depending on system fonts. pdfmake has no glyph
 * fallback, so every text run is split by script and CJK runs use Noto Sans SC.
 */
import { marked, type Token, type Tokens } from 'marked';
import type { Content, ContentText, TDocumentDefinitions, TFontDictionary } from 'pdfmake/interfaces';
import type { Transcript } from '@/types';

export interface MeetingPdfInput {
  title: string;
  createdAt?: string;
  summaryMarkdown: string;
  transcripts: Transcript[];
}

const FONT_FILES = {
  'NotoSans-Regular.ttf': '/fonts/NotoSans-Regular.ttf',
  'NotoSans-Bold.ttf': '/fonts/NotoSans-Bold.ttf',
  'NotoSans-Italic.ttf': '/fonts/NotoSans-Italic.ttf',
  'NotoSans-BoldItalic.ttf': '/fonts/NotoSans-BoldItalic.ttf',
  'NotoSansSC-Regular.otf': '/fonts/NotoSansSC-Regular.otf',
  'NotoSansSC-Bold.otf': '/fonts/NotoSansSC-Bold.otf',
};

const FONTS: TFontDictionary = {
  NotoSans: {
    normal: 'NotoSans-Regular.ttf',
    bold: 'NotoSans-Bold.ttf',
    italics: 'NotoSans-Italic.ttf',
    bolditalics: 'NotoSans-BoldItalic.ttf',
  },
  NotoSansSC: {
    normal: 'NotoSansSC-Regular.otf',
    bold: 'NotoSansSC-Bold.otf',
    italics: 'NotoSansSC-Regular.otf',
    bolditalics: 'NotoSansSC-Bold.otf',
  },
};

// CJK ideographs, kana, hangul, CJK punctuation and fullwidth forms
const CJK_RUN = /([⺀-鿿가-힯豈-﫿︰-﹏＀-￯]+)/;

let vfsPromise: Promise<Record<string, string>> | null = null;

/** Fetch the bundled fonts once and keep them as a pdfmake virtual file system */
function loadFontVfs(): Promise<Record<string, string>> {
  if (!vfsPromise) {
    vfsPromise = Promise.all(
      Object.entries(FONT_FILES).map(async ([name, url]) => {
        const response = await fetch(url);
        if (!response.ok) throw new Error(`Failed to load font ${url}: ${response.status}`);
        return [name, arrayBufferToBase64(await response.arrayBuffer())] as const;
      })
    )
      .then(Object.fromEntries)
      .catch((error) => {
        vfsPromise = null;
        throw error;
      });
  }
  return vfsPromise;
}

function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

interface RunStyle {
  bold?: boolean;
  italics?: boolean;
  decoration?: 'lineThrough' | 'underline';
  color?: string;
  link?: string;
}

/** Split text into runs so CJK characters use the CJK font */
function scriptRuns(text: string, style: RunStyle = {}): ContentText[] {
  return text
    .split(CJK_RUN)
    .filter(part => part.length > 0)
    .map(part => ({
      text: part,
      ...style,
      ...(CJK_RUN.test(part) ? { font: 'NotoSansSC' } : {}),
    }));
}

function decodeEntities(text: string): string {
  return text
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&');
}

function inlineRuns(tokens: Token[] | undefined, style: RunStyle = {}): ContentText[] {
  if (!tokens) return [];
  return tokens.flatMap((token): ContentText[] => {
    switch (token.type) {
      case 'strong':
        return inlineRuns((token as Tokens.Strong).tokens, { ...style, bold: true });
      case 'em':
        return inlineRuns((token as Tokens.Em).tokens, { ...style, italics: true });
      case 'del':
        return inlineRuns((token as Tokens.Del).tokens, { ...style, decoration: 'lineThrough' });
      case 'link':
        return inlineRuns((token as Tokens.Link).tokens, {
          ...style,
          color: '#1d4ed8',
          link: (token as Tokens.Link).href,
        });
      case 'codespan':
        return scriptRuns(decodeEntities((token as Tokens.Codespan).text), { ...style, color: '#b45309' });
      case 'br':
        return [{ text: '\n' }];
      case 'text': {
        const text = token as Tokens.Text;
        return text.tokens ? inlineRuns(text.tokens, style) : scriptRuns(decodeEntities(text.text), style);
      }
      case 'escape':
      case 'html':
        return scriptRuns(decodeEntities((token as Tokens.Escape).text), style);
      default:
        return 'text' in token && typeof token.text === 'string'
          ? scriptRuns(decodeEntities(token.text), style)
          : [];
    }
  });
}

const HEADING_SIZES: Record<number, number> = { 1: 16, 2: 14, 3: 12.5, 4: 11.5, 5: 11, 6: 11 };

function blockContent(tokens: Token[]): Content[] {
  return tokens.flatMap((token): Content[] => {
    switch (token.type) {
      case 'heading': {
        const heading = token as Tokens.Heading;
        return [{
          text: inlineRuns(heading.tokens, { bold: true }),
          fontSize: HEADING_SIZES[heading.depth] ?? 11,
          margin: [0, heading.depth <= 2 ? 12 : 8, 0, 4],
        }];
      }
      case 'paragraph':
        return [{ text: inlineRuns((token as Tokens.Paragraph).tokens), margin: [0, 0, 0, 6] }];
      case 'text': {
        const text = token as Tokens.Text;
        return [{ text: text.tokens ? inlineRuns(text.tokens) : scriptRuns(decodeEntities(text.text)) }];
      }
      case 'list': {
        const list = token as Tokens.List;
        const items = list.items.map(item => {
          const content = blockContent(item.tokens);
          const prefix = item.task ? (item.checked ? '☑ ' : '☐ ') : '';
          if (prefix && content.length > 0 && typeof content[0] === 'object' && 'text' in content[0]) {
            const first = content[0] as ContentText;
            content[0] = { ...first, text: [{ text: prefix }, ...(Array.isArray(first.text) ? first.text : [first.text])] } as ContentText;
          }
          return content.length === 1 ? content[0] : { stack: content };
        });
        return [list.ordered
          ? { ol: items, start: typeof list.start === 'number' ? list.start : 1, margin: [0, 0, 0, 6] }
          : { ul: items, margin: [0, 0, 0, 6] }];
      }
      case 'table': {
        const table = token as Tokens.Table;
        const header = table.header.map(cell => ({ text: inlineRuns(cell.tokens, { bold: true }), fillColor: '#f3f4f6' }));
        const rows = table.rows.map(row => row.map(cell => ({ text: inlineRuns(cell.tokens) })));
        return [{
          table: { headerRows: 1, widths: header.map(() => '*'), body: [header, ...rows] },
          layout: {
            hLineWidth: () => 0.5,
            vLineWidth: () => 0.5,
            hLineColor: () => '#d1d5db',
            vLineColor: () => '#d1d5db',
            paddingTop: () => 3,
            paddingBottom: () => 3,
          },
          fontSize: 10,
          margin: [0, 2, 0, 8],
        }];
      }
      case 'blockquote':
        return [{
          stack: blockContent((token as Tokens.Blockquote).tokens),
          margin: [12, 0, 0, 6],
          color: '#4b5563',
        }];
      case 'code':
        return [{ text: scriptRuns((token as Tokens.Code).text), fontSize: 9.5, color: '#374151', margin: [8, 0, 0, 6] }];
      case 'hr':
        return [{ canvas: [{ type: 'line', x1: 0, y1: 0, x2: 495, y2: 0, lineWidth: 0.5, lineColor: '#d1d5db' }], margin: [0, 6, 0, 8] }];
      case 'html':
        return [{ text: scriptRuns(decodeEntities((token as Tokens.HTML).text.replace(/<[^>]+>/g, ''))) }];
      default:
        return [];
    }
  });
}

/** Convert summary markdown to pdfmake content */
export function markdownToPdfContent(markdown: string): Content[] {
  return blockContent(marked.lexer(markdown, { gfm: true }));
}

function formatRecordingTime(seconds?: number): string {
  const total = Math.max(0, Math.floor(seconds ?? 0));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  const mmss = `${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
  return hours > 0 ? `${hours}:${mmss}` : mmss;
}

function transcriptContent(transcripts: Transcript[]): Content[] {
  const languages = new Set(transcripts.map(t => t.language).filter(Boolean));
  const showLanguage = languages.size > 1;

  return transcripts
    .filter(t => t.text.trim().length > 0)
    .map(t => ({
      columns: [
        {
          width: showLanguage ? 62 : 42,
          text: [
            { text: t.audio_start_time !== undefined ? formatRecordingTime(t.audio_start_time) : t.timestamp, color: '#6b7280' },
            ...(showLanguage && t.language ? [{ text: ` ${t.language.toUpperCase()}`, color: '#9ca3af', fontSize: 7.5 }] : []),
          ],
          fontSize: 9,
        },
        { width: '*', text: scriptRuns(t.text.trim()) },
      ],
      columnGap: 6,
      margin: [0, 0, 0, 4],
    }));
}

export function buildMeetingDocDefinition(input: MeetingPdfInput): TDocumentDefinitions {
  const lastEnd = input.transcripts.reduce((max, t) => Math.max(max, t.audio_end_time ?? 0), 0);
  const meta = [
    input.createdAt ? new Date(input.createdAt).toLocaleString() : null,
    lastEnd > 0 ? `Duration ${formatRecordingTime(lastEnd)}` : null,
  ].filter(Boolean).join('  ·  ');

  const summary = input.summaryMarkdown.trim()
    ? markdownToPdfContent(input.summaryMarkdown)
    : [{ text: 'No summary has been generated for this meeting.', italics: true, color: '#6b7280' }];

  const transcript = input.transcripts.length
    ? transcriptContent(input.transcripts)
    : [{ text: 'No transcript available.', italics: true, color: '#6b7280' }];

  return {
    pageSize: 'A4',
    pageMargins: [50, 56, 50, 56],
    info: { title: input.title, creator: 'TriNote' },
    defaultStyle: { font: 'NotoSans', fontSize: 10.5, lineHeight: 1.25, color: '#111827' },
    footer: (currentPage, pageCount) => ({
      text: `${currentPage} / ${pageCount}`,
      alignment: 'center',
      fontSize: 8,
      color: '#9ca3af',
      margin: [0, 20, 0, 0],
    }),
    content: [
      { text: scriptRuns(input.title || 'Meeting', { bold: true }), fontSize: 20, margin: [0, 0, 0, 4] },
      ...(meta ? [{ text: scriptRuns(meta), fontSize: 9, color: '#6b7280', margin: [0, 0, 0, 14] } as Content] : []),
      ...summary,
      { text: 'Transcript', fontSize: 16, bold: true, pageBreak: 'before', margin: [0, 0, 0, 10] },
      ...transcript,
    ],
  };
}

/** Render the meeting PDF and return its bytes */
export async function renderMeetingPdf(input: MeetingPdfInput): Promise<Uint8Array> {
  const [{ default: pdfMake }, vfs] = await Promise.all([
    import('pdfmake/build/pdfmake'),
    loadFontVfs(),
  ]);
  const doc = buildMeetingDocDefinition(input);
  return new Promise((resolve, reject) => {
    try {
      pdfMake.createPdf(doc, undefined, FONTS, vfs).getBuffer((buffer: Uint8Array) => resolve(buffer));
    } catch (error) {
      reject(error);
    }
  });
}
