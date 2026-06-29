// lib/codemirror/theme.ts
import { EditorView } from "@codemirror/view"

export const essayTheme = EditorView.theme(
  {
    "&": {
      backgroundColor: "var(--essay-bg)",
      color: "var(--essay-text)",
      fontSize: "15px",
      height: "100%",
    },
    ".cm-content": {
      caretColor: "#e0e0e0",
      fontFamily:
        "'Segoe UI', 'Inter', -apple-system, BlinkMacSystemFont, system-ui, sans-serif",
      padding: "32px 48px",
      // 1.6 reads tight without being cramped; closer to Obsidian than
      // the previous 1.85 which felt airy for note-taking.
      lineHeight: "1.6",
      maxWidth: "800px",
      margin: "0 auto",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "#e0e0e0",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
      {
        backgroundColor: "rgba(212, 165, 116, 0.18)",
      },
    // Subtle active-line highlight — closer to Obsidian's whisper
    // (0.04 alpha) than a strong glow. Pairs with the focus ring.
    ".cm-activeLine": {
      backgroundColor: "var(--essay-active-line)",
    },
    ".cm-activeLineGutter": {
      backgroundColor: "var(--essay-active-line)",
      color: "var(--essay-text-muted)",
    },

    // ── Gutters ──────────────────────────────────────
    ".cm-gutters": {
      backgroundColor: "var(--essay-bg)",
      color: "#555",
      border: "none",
      paddingRight: "10px",
    },
    ".cm-foldGutter .cm-gutterElement": {
      color: "#555",
      cursor: "pointer",
      padding: "0 4px",
    },
    ".cm-foldGutter .cm-gutterElement:hover": {
      color: "#888",
    },
    ".cm-foldPlaceholder": {
      backgroundColor: "#1a1a1a",
      border: "1px solid #333",
      color: "#888",
      borderRadius: "3px",
      padding: "0 6px",
      margin: "0 2px",
      fontFamily: "sans-serif",
    },

    // ── Headings (colored per level) ─────────────────
    ".cm-heading": {
      fontWeight: "600",
      fontFamily:
        "'Segoe UI', 'Inter', -apple-system, sans-serif",
    },
    ".cm-heading1": {
      fontSize: "1.7em",
      fontWeight: "700",
      marginTop: "0.7em",
      marginBottom: "0.2em",
      color: "#e06c75",
    },
    ".cm-heading2": {
      fontSize: "1.4em",
      fontWeight: "600",
      marginTop: "0.6em",
      marginBottom: "0.15em",
      color: "#d19a66",
    },
    ".cm-heading3": {
      fontSize: "1.2em",
      fontWeight: "600",
      marginTop: "0.5em",
      color: "#e5c07b",
    },
    ".cm-heading4": {
      fontSize: "1.05em",
      fontWeight: "600",
      color: "#61afef",
    },
    ".cm-heading5": {
      fontSize: "1em",
      fontWeight: "600",
      color: "#c678dd",
    },
    ".cm-heading6": {
      fontSize: "0.95em",
      fontWeight: "600",
      color: "#98c379",
    },

    // ── Inline formatting ────────────────────────────
    ".cm-strong": {
      fontWeight: "700",
      color: "#e8e8e8",
    },
    ".cm-emphasis": {
      fontStyle: "italic",
    },
    ".cm-strikethrough": {
      textDecoration: "line-through",
      color: "#777",
    },

    // ── Inline code ──────────────────────────────────
    ".cm-code": {
      fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace",
      fontSize: "0.9em",
      backgroundColor: "#1e1e2e",
      borderRadius: "3px",
      padding: "1px 5px",
      color: "#e5c07b",
    },

    // ── Code blocks ──────────────────────────────────
    ".cm-codeBlock": {
      backgroundColor: "var(--essay-code-bg)",
      borderRadius: "6px",
      border: "1px solid var(--essay-border)",
      padding: "14px 18px",
      fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
      fontSize: "0.88em",
      lineHeight: "1.6",
      marginTop: "8px",
      marginBottom: "8px",
    },

    // ── Block quotes ─────────────────────────────────
    // Obsidian-style: 2px left border in a muted grey, no fill,
    // no italic, no rounded right. Indent is 16px.
    ".cm-quote": {
      borderLeft: "2px solid var(--essay-quote-border)",
      backgroundColor: "transparent",
      paddingLeft: "16px",
      paddingTop: "2px",
      paddingBottom: "2px",
      marginTop: "6px",
      marginBottom: "6px",
      color: "var(--essay-text-muted)",
      borderRadius: "0",
    },

    // ── Links ────────────────────────────────────────
    ".cm-link": {
      color: "#61afef",
      textDecoration: "underline",
      textUnderlineOffset: "2px",
    },
    ".cm-url": {
      color: "#4a90d9",
      fontSize: "0.85em",
      opacity: "0.6",
    },

    // ── Tags ─────────────────────────────────────────
    ".cm-hashtag": {
      color: "#c678dd",
      backgroundColor: "#2a1f3d",
      padding: "0 5px",
      borderRadius: "4px",
      fontSize: "0.92em",
      fontWeight: "500",
    },

    // ── Horizontal rule ──────────────────────────────
    ".cm-hr": {
      border: "none",
      borderTop: "1px solid #333",
      margin: "16px 0",
    },

    // ── Lists ────────────────────────────────────────
    ".cm-list": {
      paddingLeft: "4px",
    },

    // ── Live Preview: hide delimiters on inactive lines ──
    ".cm-line:not(.cm-activeLine) .cm-formatting": {
      opacity: "0",
    },
    ".cm-line:not(.cm-activeLine) .cm-formatting-header": {
      display: "none",
    },
    ".cm-line:not(.cm-activeLine) .cm-formatting-quote": {
      display: "none",
    },
    ".cm-line:not(.cm-activeLine) .cm-formatting-list": {
      opacity: "0",
    },
    ".cm-task-marker": {
      opacity: "1 !important" as unknown as number,
    },

    // ── Callout blocks ───────────────────────────────
    ".cm-callout": {
      borderRadius: "6px",
      borderLeft: "4px solid",
      padding: "10px 16px",
      margin: "8px 0",
      fontFamily:
        "'Segoe UI', 'Inter', -apple-system, sans-serif",
    },
    ".cm-callout-title": {
      fontWeight: "700",
      marginBottom: "4px",
      fontSize: "0.95em",
    },
    ".cm-callout-body": {
      color: "#c8c8c8",
      lineHeight: "1.7",
    },
    // Per-type colors (set via inline styles on the widget)
    ".cm-callout-note": {
      borderLeftColor: "#4a90d9",
      backgroundColor: "#151d2a",
    },
    ".cm-callout-warning": {
      borderLeftColor: "#d19a66",
      backgroundColor: "#1f1912",
    },
    ".cm-callout-danger": {
      borderLeftColor: "#e06c75",
      backgroundColor: "#1f1315",
    },
    ".cm-callout-info": {
      borderLeftColor: "#61afef",
      backgroundColor: "#121e2a",
    },
    ".cm-callout-tip": {
      borderLeftColor: "#98c379",
      backgroundColor: "#141d14",
    },
    ".cm-callout-example": {
      borderLeftColor: "#c678dd",
      backgroundColor: "#1d1728",
    },
    ".cm-callout-quote": {
      borderLeftColor: "#888",
      backgroundColor: "#181818",
    },

    // ── Image widget ─────────────────────────────────
    ".cm-image-widget": {
      maxWidth: "100%",
      maxHeight: "200px",
      borderRadius: "6px",
      margin: "8px 0",
      cursor: "pointer",
      display: "block",
      objectFit: "contain",
      border: "1px solid #2a2a2a",
    },
    ".cm-image-widget:hover": {
      borderColor: "#555",
    },

    // ── Table styling ────────────────────────────────
    ".cm-table-widget": {
      overflowX: "auto",
      padding: "4px 0",
    },
    ".cm-table-widget table": {
      borderCollapse: "collapse",
      width: "100%",
    },
    ".cm-table-widget td, .cm-table-widget th": {
      border: "1px solid #333",
      padding: "6px 10px",
      textAlign: "left",
    },
    ".cm-table-widget th": {
      backgroundColor: "#1a1a1a",
      fontWeight: "600",
    },
  },
  { dark: true },
)
