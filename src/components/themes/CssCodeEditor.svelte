<script lang="ts">
import { onMount } from 'svelte';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { css } from '@codemirror/lang-css';
import { autocompletion, closeBrackets, closeBracketsKeymap, type CompletionContext } from '@codemirror/autocomplete';
import { foldGutter, foldKeymap, HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { tags } from '@lezer/highlight';
import { themeStore, DEFAULT_OMNI_CSS } from '../../lib/stores/theme.svelte';

let {
  value = $bindable(''),
  onchange = (_v: string) => {}
}: {
  value: string;
  onchange?: (val: string) => void;
} = $props();

let containerEl: HTMLDivElement | undefined = $state();
let view: EditorView | null = null;
let lastExternal = value;
let selectedInsert = $state<string>('');

const OMNI_SELECTORS = [
  '[data-omni-app]',
  '[data-omni-titlebar]',
  '[data-omni-main]',
  '[data-omni-sidebar]',
  '[data-omni-sidebar-header]',
  '[data-omni-chat-list]',
  '[data-omni-chat-item]',
  '[data-omni-chat-view]',
  '[data-omni-messages]',
  '[data-omni-message="user"]',
  '[data-omni-message="assistant"]',
  '[data-omni-message-bubble]',
  '[data-omni-timeline]',
  '[data-omni-timeline-header]',
  '[data-omni-tool-row]',
  '[data-omni-composer]',
  '[data-omni-composer-box]',
  '[data-omni-composer-input]',
  '[data-omni-button="send"]',
  '[data-omni-button="plus"]',
  '[data-omni-button="new-chat"]',
  '[data-omni-button="settings"]',
  '[data-omni-model-badge]',
  '[data-omni-reasoning-badge]',
  '[data-omni-search-box]',
  '[data-omni-icon]',
  '--color-bg-base',
  '--color-bg-sidebar',
  '--color-bg-surface',
  '--color-bg-card',
  '--color-bg-elevated',
  '--color-bg-hover',
  '--color-border-subtle',
  '--color-border-default',
  '--color-text-primary',
  '--color-text-secondary',
  '--color-text-muted',
  '--font-sans',
  '--font-mono'
];

function omniCompletions(context: CompletionContext) {
  const word = context.matchBefore(/[\[\w:/-]+/);
  if (!word || (word.from === word.to && !context.explicit)) return null;

  const assetOptions = Object.keys(themeStore.editingTheme?.assets || {}).map((assetName) => ({
    label: `omni-asset://${assetName}`,
    type: 'constant',
    detail: 'Asset URL'
  }));

  const selectorOptions = OMNI_SELECTORS.map((s) => ({
    label: s,
    type: s.startsWith('--') ? 'variable' : 'class',
    detail: s.startsWith('--') ? 'Omni CSS variable' : 'Omni DOM selector'
  }));

  return {
    from: word.from,
    options: [...selectorOptions, ...assetOptions]
  };
}

const omniTheme = EditorView.theme({
  '&': {
    backgroundColor: '#0A0A0D',
    color: '#E5E5E8',
    height: '100%',
    fontSize: '12px',
    fontFamily: '"JetBrains Mono", "Cascadia Code", "Consolas", monospace'
  },
  '.cm-content': {
    caretColor: '#EDEDEF',
    padding: '8px 0'
  },
  '.cm-gutters': {
    backgroundColor: '#08080B',
    color: '#52525B',
    borderRight: '1px solid rgba(255,255,255,0.06)'
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'rgba(255,255,255,0.04)',
    color: '#A3A3AA'
  },
  '.cm-activeLine': {
    backgroundColor: 'rgba(255,255,255,0.025)'
  },
  '.cm-selectionBackground, ::selection': {
    backgroundColor: 'rgba(255, 255, 255, 0.14) !important'
  },
  '.cm-cursor': {
    borderLeftColor: '#EDEDEF'
  }
}, { dark: true });

const omniHighlight = HighlightStyle.define([
  { tag: tags.comment, color: '#64748B', fontStyle: 'italic' },
  { tag: tags.tagName, color: '#C084FC' },
  { tag: tags.className, color: '#60A5FA' },
  { tag: tags.attributeName, color: '#A78BFA' },
  { tag: tags.propertyName, color: '#93C5FD' },
  { tag: tags.string, color: '#86EFAC' },
  { tag: tags.number, color: '#FCA5A5' },
  { tag: tags.color, color: '#FDE047' },
  { tag: tags.keyword, color: '#F472B6' },
  { tag: tags.punctuation, color: '#71717A' }
]);

function insertSnippet(snippet: string) {
  if (!view) return;
  const sel = view.state.selection.main;
  view.dispatch({
    changes: { from: sel.from, to: sel.to, insert: snippet },
    selection: { anchor: sel.from + snippet.length }
  });
  view.focus();
}

function handleSelectorInsert(sel: string) {
  if (!sel) return;
  insertSnippet(`${sel} {\n  \n}\n`);
  selectedInsert = '';
}

function resetToDefault() {
  if (confirm('Reset CSS to default OmniHarness theme template?')) {
    value = DEFAULT_OMNI_CSS;
    if (view) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: DEFAULT_OMNI_CSS }
      });
    }
    onchange(DEFAULT_OMNI_CSS);
  }
}

onMount(() => {
  if (!containerEl) return;
  const state = EditorState.create({
    doc: value,
    extensions: [
      lineNumbers(),
      highlightActiveLineGutter(),
      highlightActiveLine(),
      history(),
      foldGutter(),
      closeBrackets(),
      css(),
      autocompletion({ override: [omniCompletions] }),
      syntaxHighlighting(omniHighlight),
      omniTheme,
      keymap.of([...closeBracketsKeymap, ...defaultKeymap, ...historyKeymap, ...foldKeymap, indentWithTab]),
      EditorView.updateListener.of((u: any) => {
        if (u.docChanged) {
          const next = u.state.doc.toString();
          lastExternal = next;
          value = next;
          onchange(next);
        }
      })
    ]
  });
  view = new EditorView({ state, parent: containerEl });
  return () => {
    view?.destroy();
    view = null;
  };
});

$effect(() => {
  if (view && value !== lastExternal) {
    lastExternal = value;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: value }
    });
  }
});

let lineCount = $derived(value.split('\n').length);
let charCount = $derived(value.length);
let availableAssets = $derived(Object.keys(themeStore.editingTheme?.assets || {}));
</script>

<div class="w-full h-full flex flex-col overflow-hidden rounded-[6px] border border-white/[0.08] bg-[#0A0A0D]">
  <div class="h-8 px-2.5 bg-[#0D0D10] border-b border-white/[0.06] flex items-center justify-between gap-2 shrink-0 select-none">
    <div class="flex items-center gap-1.5 overflow-x-auto [scrollbar-width:none]">
      <select
        bind:value={selectedInsert}
        onchange={() => handleSelectorInsert(selectedInsert)}
        class="h-5 px-1.5 rounded-[4px] bg-[#16161A] border border-white/[0.08] text-[10.5px] text-[#A3A3AA] outline-none"
      >
        <option value="">+ Insert Selector...</option>
        {#each OMNI_SELECTORS as sel}
          <option value={sel}>{sel}</option>
        {/each}
      </select>

      {#if availableAssets.length > 0}
        <select
          onchange={(e) => {
            const v = (e.target as HTMLSelectElement).value;
            if (v) {
              insertSnippet(`url("omni-asset://${v}")`);
              (e.target as HTMLSelectElement).value = '';
            }
          }}
          class="h-5 px-1.5 rounded-[4px] bg-[#16161A] border border-white/[0.08] text-[10.5px] text-[#A3A3AA] outline-none"
        >
          <option value="">+ Insert Asset URL...</option>
          {#each availableAssets as ast}
            <option value={ast}>{ast}</option>
          {/each}
        </select>
      {/if}
    </div>

    <div class="flex items-center gap-2 shrink-0">
      <span class="font-mono text-[10px] text-[#52525B]">{lineCount}L · {charCount}C</span>
      <button
        type="button"
        onclick={resetToDefault}
        class="h-5 px-1.5 rounded-[4px] bg-[#18181C] hover:bg-[#222228] text-[10px] font-mono text-[#71717A] hover:text-[#A1A1AA] transition-colors"
        title="Reset CSS to standard template"
      >
        Reset
      </button>
    </div>
  </div>

  <div bind:this={containerEl} class="flex-1 min-h-0 overflow-hidden"></div>
</div>