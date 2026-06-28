"use client"

import dynamic from "next/dynamic"
import { useCallback, useEffect, useRef } from "react"
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
  const observerRef = useRef<(() => void) | null>(null)

  useEffect(() => {
    editorRef.current?.updateOptions({ readOnly: readOnly || collaborative.readOnly })
  }, [readOnly, collaborative.readOnly])

  // Bind Yjs once the editor has mounted. The dynamic import is async, so a
  // remount or StrictMode double-invoke can fire teardown before the import
  // resolves — `cancelled` prevents a late bind, and `cancelBindRef` lets the
  // cleanup abort a still-pending import. Without this, a binding gets built
  // after destroy and Yjs warns "Tried to remove event handler that doesn't
  // exist" when its stale observer is detached.
  const cancelBindRef = useRef<(() => void) | null>(null)

  // Tear down the active Yjs stack exactly once. Idempotent: refs are read into
  // locals and nulled BEFORE destroy() runs, so a second invocation (StrictMode
  // double-cleanup, or a fileId change racing an unmount) finds nothing to
  // destroy and can't double-unobserve — which is what produced the Yjs
  // "Tried to remove event handler that doesn't exist" warning.
  const teardown = useCallback(() => {
    cancelBindRef.current?.()
    cancelBindRef.current = null

    const text = textRef.current
    const observer = observerRef.current
    const binding = bindingRef.current
    const provider = providerRef.current
    const doc = docRef.current

    textRef.current = null
    observerRef.current = null
    bindingRef.current = null
    providerRef.current = null
    docRef.current = null

    if (text && observer) {
      try { text.unobserve(observer) } catch { /* noop */ }
    }
    try { binding?.destroy() } catch { /* noop */ }
    try { provider?.destroy() } catch { /* noop */ }
    try { doc?.destroy() } catch { /* noop */ }
  }, [])

  const bindYjs = useCallback((editor: any) => {
    editorRef.current = editor
    editor.updateOptions({ readOnly: readOnly || collaborative.readOnly })

    // Defensively tear down any binding still attached from a prior mount
    // before building a new one, so observers never accumulate.
    teardown()

    let cancelled = false
    cancelBindRef.current = () => { cancelled = true }
    void Promise.all([import("yjs"), import("y-monaco")]).then(([Yjs, yMonaco]) => {
      if (cancelled) return
      const model = editor.getModel()
      if (!model) return
      const doc = new Yjs.Doc()
      const text = doc.getText("content")
      const provider = new YjsWebsocketProvider(doc, collaborative.fileId)
      const binding = new yMonaco.MonacoBinding(text, model, new Set([editor]), null)
      // Make binding.destroy() idempotent. y-monaco registers an
      // onWillDispose handler on the Monaco model that auto-destroys the
      // binding whenever the model goes away (StrictMode double-mount,
      // editor swap, unmount). Our teardown also calls binding.destroy().
      // The second call would re-unobserve observers that are already
      // gone, and yjs logs "[yjs] Tried to remove event handler that
      // doesn't exist." via console.error — which Next.js dev mode
      // surfaces as a Console Error and the surrounding try/catch can't
      // catch (it's not a throw). Guarding with a one-shot flag keeps
      // both paths safe.
      {
        let destroyed = false
        const originalDestroy = binding.destroy.bind(binding)
        binding.destroy = () => {
          if (destroyed) return
          destroyed = true
          originalDestroy()
        }
      }

      docRef.current = doc
      textRef.current = text
      providerRef.current = provider
      bindingRef.current = binding

      const emitChange = () => {
        const content = text.toString()
        onChange?.(content)
        collaborative.onDocumentChange?.(content)
      }
      observerRef.current = emitChange
      text.observe(emitChange)
      emitChange()
    })
  }, [collaborative.fileId, teardown])

  useEffect(() => {
    return () => teardown()
  }, [collaborative.fileId, teardown])

  return (
    <MonacoEditor
      key={collaborative.fileId}
      height="100%"
      language={language}
      theme="vs-dark"
      defaultValue={value}
      onMount={(editor) => {
        bindYjs(editor)
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
