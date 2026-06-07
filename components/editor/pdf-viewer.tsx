"use client"

import { useState, useEffect } from "react"
import dynamic from "next/dynamic"
import { Loader2, AlertCircle, FileText, Maximize2 } from "lucide-react"
import type { PDFDocumentProxy } from "pdfjs-dist"
import { readFileBase64 } from "@/lib/tauri-api"

const PdfDocumentRenderer = dynamic(
  () => import("@/components/editor/pdf-document-renderer"),
  {
    ssr: false,
    loading: () => (
      <div className="flex h-full items-center justify-center gap-3 text-[#787878]">
        <Loader2 className="h-5 w-5 animate-spin text-[#d4a574]" />
        <span className="text-sm">Preparing PDF renderer...</span>
      </div>
    ),
  },
)

interface PdfViewerProps {
  filePath: string
}

function base64ToBytes(value: string): Uint8Array {
  const binary = window.atob(value)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i)
  }
  return bytes
}

export default function PdfViewer({ filePath }: PdfViewerProps) {
  const [pdfBytes, setPdfBytes] = useState<Uint8Array | null>(null)
  const [pdfDataUrl, setPdfDataUrl] = useState<string | null>(null)
  const [numPages, setNumPages] = useState(0)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    async function load() {
      setLoading(true)
      setError(null)
      setPdfBytes(null)
      setPdfDataUrl(null)
      setNumPages(0)
      try {
        const b64 = await readFileBase64(filePath)
        if (cancelled) return
        setPdfBytes(base64ToBytes(b64))
        setPdfDataUrl(`data:application/pdf;base64,${b64}`)
        setLoading(false)
      } catch (e) {
        if (cancelled) return
        setError(String(e))
        setLoading(false)
      }
    }
    load()
    return () => { cancelled = true }
  }, [filePath])

  const openExternal = () => {
    if (pdfDataUrl) {
      window.open(pdfDataUrl, "_blank")
    }
  }

  const retry = () => {
    setError(null)
    setLoading(true)
    setPdfBytes(null)
    setPdfDataUrl(null)
    setNumPages(0)
    readFileBase64(filePath)
      .then((b64) => {
        setPdfBytes(base64ToBytes(b64))
        setPdfDataUrl(`data:application/pdf;base64,${b64}`)
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false))
  }

  const handleLoadSuccess = ({ numPages }: PDFDocumentProxy) => {
    setNumPages(numPages)
  }

  return (
    <div className="flex h-full flex-col bg-[#0d0d0d]">
      {/* Toolbar */}
      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-[#373737] bg-[#121212] px-3">
        <div className="flex-1" />
        <span className="text-xs text-[#555] truncate max-w-48">
          <FileText className="mr-1 inline h-3.5 w-3.5 text-[#f44336]" />
          {filePath.split("/").pop()?.split("\\").pop()}
        </span>
        <button
          type="button"
          onClick={openExternal}
          disabled={!pdfDataUrl}
          className="rounded p-1 text-[#787878] hover:bg-[#232323] hover:text-[#e8e8e8] disabled:opacity-30 disabled:cursor-not-allowed"
          title="Open in system viewer"
        >
          <Maximize2 className="h-3.5 w-3.5" />
        </button>
      </div>

      {/* Content */}
      <div className="min-h-0 flex-1 overflow-auto bg-[#323232]">
        {loading && (
          <div className="flex h-full items-center justify-center gap-3 text-[#787878]">
            <Loader2 className="h-5 w-5 animate-spin text-[#d4a574]" />
            <span className="text-sm">Loading PDF...</span>
          </div>
        )}

        {error && (
          <div className="flex h-full items-center justify-center">
            <div className="flex flex-col items-center gap-3 rounded-lg border border-[#5f2424] bg-[#2d1a1a] px-6 py-4">
              <AlertCircle className="h-6 w-6 text-[#f44336]" />
              <p className="text-sm text-[#ffb4a8] max-w-md text-center">
                {error}
              </p>
              <button
                type="button"
                onClick={retry}
                className="rounded-md border border-[#5f2424] px-3 py-1 text-xs text-[#ffb4a8] hover:bg-[#3d2424] transition-colors"
              >
                Retry
              </button>
            </div>
          </div>
        )}

        {pdfBytes && !loading && !error && (
          <PdfDocumentRenderer
            fileData={pdfBytes}
            numPages={numPages}
            onLoadSuccess={handleLoadSuccess}
            onLoadError={(e) => setError(e.message)}
          />
        )}
      </div>
    </div>
  )
}
