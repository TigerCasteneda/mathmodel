"use client"

import dynamic from "next/dynamic"
import { useEffect, useRef } from "react"
import type * as Y from "yjs"
import { YjsWebsocketProvider } from "@/lib/yjs-provider"

const MonacoEditor = dynamic(
  () => import("@monaco-editor/react").then((mod) => mod.default),
  { ssr: false },
)

const MonacoDiffEditor = dynamic(
  () => import("@monaco-editor/react").then((mod) => mod.DiffEditor),
  { ssr: false },
)

type CollaborativeEditorOptions = {
  fileId: string
  user?: {
    id?: string
    name?: string
    color?: string
  }
  readOnly?: boolean
  onDocumentChange?: (value: string) => void
}

type DiffEditorOptions = {
  left: string
  right: string
  leftTitle?: string
  rightTitle?: string
}

type CodeEditorProps = {
  language: string
  value: string
  readOnly?: boolean
  onChange?: (value: string) => void
  collaborative?: CollaborativeEditorOptions
  diff?: DiffEditorOptions
}

function CollaborativeCodeEditor({
  language,
  value,
  readOnly,
  collaborative,
  onChange,
}: {
  language: string
  value: string
  readOnly: boolean
  collaborative: CollaborativeEditorOptions
  onChange?: (value: string) => void
}) {
  const bindingRef = useRef<{ destroy: () => void } | null>(null)
  const providerRef = useRef<YjsWebsocketProvider | null>(null)
  const docRef = useRef<Y.Doc | null>(null)
  const editorRef = useRef<any>(null)
  const textRef = useRef<Y.Text | null>(null)

  useEffect(() => {
    editorRef.current?.updateOptions({ readOnly: readOnly || collaborative.readOnly })
  }, [readOnly, collaborative.readOnly])

  useEffect(() => {
    return () => {
      try { bindingRef.current?.destroy() } catch { /* noop */ }
      try { providerRef.current?.destroy() } catch { /* noop */ }
      try { docRef.current?.destroy() } catch { /* noop */ }
      bindingRef.current = null
      providerRef.current = null
      docRef.current = null
      textRef.current = null
    }
  }, [collaborative.fileId])

  return (
    <MonacoEditor
      key={collaborative.fileId}
      height="100%"
      language={language}
      theme="vs-dark"
      defaultValue={value}
      onMount={(editor) => {
        editorRef.current = editor
        editor.updateOptions({ readOnly: readOnly || collaborative.readOnly })
        void Promise.all([import("yjs"), import("y-monaco")]).then(([Yjs, yMonaco]) => {
          const model = editor.getModel()
          if (!model) return
          const doc = new Yjs.Doc()
          const text = doc.getText("content")
          const provider = new YjsWebsocketProvider(doc, collaborative.fileId)
          const binding = new yMonaco.MonacoBinding(text, model, new Set([editor]), null)

          docRef.current = doc
          textRef.current = text
          providerRef.current = provider
          bindingRef.current = binding

          const emitChange = () => {
            const content = text.toString()
            onChange?.(content)
            collaborative.onDocumentChange?.(content)
          }
          text.observe(emitChange)
          emitChange()
        })
      }}
      options={{
        minimap: { enabled: true, scale: 0.8 },
        fontSize: 13,
        lineNumbers: "on",
        scrollBeyondLastLine: false,
        padding: { top: 10, bottom: 10 },
        renderWhitespace: "selection",
        wordWrap: "on",
        readOnly: readOnly || collaborative.readOnly,
      }}
    />
  )
}

export function CodeEditor({
  language,
  value,
  readOnly = false,
  onChange,
  collaborative,
  diff,
}: CodeEditorProps) {
  if (diff) {
    return (
      <MonacoDiffEditor
        height="100%"
        language={language}
        theme="vs-dark"
        original={diff.left}
        modified={diff.right}
        options={{
          readOnly: true,
          renderSideBySide: true,
          minimap: { enabled: false },
          fontSize: 13,
          lineNumbers: "on",
          scrollBeyondLastLine: false,
          padding: { top: 10, bottom: 10 },
          renderWhitespace: "selection",
          wordWrap: "on",
        }}
      />
    )
  }

  if (collaborative) {
    return (
      <CollaborativeCodeEditor
        language={language}
        value={value}
        readOnly={readOnly}
        collaborative={collaborative}
        onChange={onChange}
      />
    )
  }

  return (
    <MonacoEditor
      height="100%"
      language={language}
      theme="vs-dark"
      value={value}
      onChange={(nextValue) => onChange?.(nextValue ?? "")}
      options={{
        minimap: { enabled: true, scale: 0.8 },
        fontSize: 13,
        lineNumbers: "on",
        scrollBeyondLastLine: false,
        padding: { top: 10, bottom: 10 },
        renderWhitespace: "selection",
        wordWrap: "on",
        readOnly,
      }}
    />
  )
}
