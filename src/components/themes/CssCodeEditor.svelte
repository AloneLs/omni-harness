<script lang="ts">
import { onMount } from 'svelte';
import { EditorState } from '@codemirror/state';
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { css } from '@codemirror/lang-css';
import { autocompletion, closeBrackets, closeBracketsKeymap, type CompletionContext } from '@codemirror/autocomplete';
import { foldGutter, foldKeymap, HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import { tags } from '@lezer/highlight';

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
  '[data-omni-button="new-chat"]',
  '[data-omni-button="settings"]',
  '[data-omni-icon]',
  '--color-bg-base',
  '--color-bg-sidebar',
  '--color-bg-surface',
  '--color-bg-card',
  '--color-accent-primary',
  '--color-text-primary',
  '--font-sans',
  '--font-mono'
];

function omniCompletions(context: CompletionContext) {
  const word = context.matchBefore(/[\[\w:-]+/);
  if (!word || (word.from === word.to && !context.explicit)) return null;
  return {
    from: word.from,
    options: OMNI_SELECTORS.map((s) => ({
      label: s,
      type: s.startsWith('--') ? 'variable' : 'class'
    }))
  };
}

const omniTheme = EditorView.theme({
  '&': {
    backgroundColor: '#0A0A0D',
    color: '#E5E5E8',
    height: '100%',
    fontSize: '12px',
    fontFamily: '"JetBrains Mono", "Cascadia Code", monospace'
  },
  '.cm-content': {
    caretColor: '#8B5CF6',
    padding: '10px 0'
  },
  '.cm-gutters': {
    backgroundColor: '#08080B',
    color: '#52525B',
    borderRight: '1px solid rgba(255,255,255,0.06)'
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'rgba(255,255,255,0.03)',
    color: '#A3A3AA'
  },
  '.cm-activeLine': {
    backgroundColor: 'rgba(255,255,255,0.025)'
  },
  '.cm-selectionBackground, ::selection': {
    backgroundColor: 'rgba(139, 92, 246, 0.25) !important'
  },
  '.cm-cursor': {
    borderLeftColor: '#A78BFA'
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
      EditorView.updateListener.of((u) => {
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
</script>

<div bind:this={containerEl} class="w-full h-full overflow-hidden rounded-[6px] border border-white/[0.08]"></div>
