// lib/codemirror/theme.ts
import { EditorView } from "@codemirror/view"

export const essayTheme = EditorView.theme(
  {
    "&": {
      backgroundColor: "#0d0d0d",
      color: "#d4d4d4",
      fontSize: "15px",
      height: "100%",
    },
    ".cm-content": {
      caretColor: "#e0e0e0",
      fontFamily: "'Geist Mono', 'JetBrains Mono', 'Fira Code', monospace",
      padding: "16px 24px",
      lineHeight: "1.75",
      maxWidth: "800px",
      margin: "0 auto",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "#e0e0e0",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
      {
        backgroundColor: "#264f78",
      },
    ".cm-activeLine": {
      backgroundColor: "#ffffff06",
    },
    ".cm-gutters": {
      backgroundColor: "#0d0d0d",
      color: "#555",
      border: "none",
      paddingRight: "8px",
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
    },
    // Heading styles for Live Preview rendering
    ".cm-heading": {
      fontWeight: "600",
    },
    ".cm-heading1": {
      fontSize: "1.6em",
      fontWeight: "700",
      marginTop: "0.6em",
    },
    ".cm-heading2": {
      fontSize: "1.35em",
      fontWeight: "600",
      marginTop: "0.5em",
    },
    ".cm-heading3": {
      fontSize: "1.15em",
      fontWeight: "600",
    },
    ".cm-strong": {
      fontWeight: "700",
    },
    ".cm-emphasis": {
      fontStyle: "italic",
    },
    ".cm-strikethrough": {
      textDecoration: "line-through",
    },
    ".cm-code": {
      fontFamily: "'Geist Mono', 'JetBrains Mono', monospace",
      fontSize: "0.9em",
    },
    ".cm-link": {
      color: "#569cd6",
      textDecoration: "underline",
    },
    ".cm-url": {
      color: "#4a90d9",
    },
    // Hide markdown delimiters when not on active line (Live Preview)
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
    // Always show task list checkboxes
    ".cm-task-marker": {
      opacity: "1 !important" as unknown as number,
    },
    // Table styling
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
      padding: "4px 8px",
      textAlign: "left",
    },
  },
  { dark: true },
)
