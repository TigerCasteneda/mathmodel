"use client"

import { useState, useEffect, useRef, useCallback } from "react"
import dynamic from "next/dynamic"
import { Loader2, AlertCircle, FileText, Maximize2, ChevronLeft, ChevronRight, Printer } from "lucide-react"
import type { PDFDocumentProxy } from "pdfjs-dist"
import type { PdfDocumentRendererHandle } from "@/components/editor/pdf-document-renderer"
import { readFileBase64, onFileBinaryChange } from "@/lib/tauri-api"

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

// The watcher emits workspace-relative paths ("paper/out.pdf") while the viewer
// may hold a fuller path; treat them as the same file if either ends with the
// other, after normalizing separators.
function pathsMatch(a: string, b: string) {
  const na = a.replace(/\\/g, "/")
  const nb = b.replace(/\\/g, "/")
  return na === nb || na.endsWith(`/${nb}`) || nb.endsWith(`/${na}`)
}

export default function PdfViewer({ filePath }: PdfViewerProps) {
  const [pdfBytes, setPdfBytes] = useState<Uint8Array | null>(null)
  const [pdfDataUrl, setPdfDataUrl] = useState<string | null>(null)
  const [numPages, setNumPages] = useState(0)
  const [currentPage, setCurrentPage] = useState(0)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const pdfRendererRef = useRef<PdfDocumentRendererHandle>(null)

  const loadPdf = useCallback(async (signal?: { cancelled: boolean }) => {
    setLoading(true)
    setError(null)
    setPdfBytes(null)
    setPdfDataUrl(null)
    setNumPages(0)
    try {
      const b64 = await readFileBase64(filePath)
      if (signal?.cancelled) return
      setPdfBytes(base64ToBytes(b64))
      setPdfDataUrl(`data:application/pdf;base64,${b64}`)
    } catch (e) {
      if (signal?.cancelled) return
      setError(String(e))
    } finally {
      if (!signal?.cancelled) setLoading(false)
    }
  }, [filePath])

  useEffect(() => {
    const signal = { cancelled: false }
    void loadPdf(signal)
    return () => { signal.cancelled = true }
  }, [loadPdf])

  // Reload when this PDF is rewritten on disk (e.g. an external LaTeX recompile).
  useEffect(() => {
    const unsubscribe = onFileBinaryChange((changedPath) => {
      if (pathsMatch(filePath, changedPath)) void loadPdf()
    })
    return unsubscribe
  }, [filePath, loadPdf])

  const openExternal = () => {
    if (pdfDataUrl) {
      window.open(pdfDataUrl, "_blank")
    }
  }

  const handlePrint = useCallback(() => {
    if (!pdfBytes) return
    const blob = new Blob([pdfBytes], { type: "application/pdf" })
    const url = URL.createObjectURL(blob)

    const iframe = document.createElement("iframe")
    iframe.style.display = "none"
    iframe.src = url
    document.body.appendChild(iframe)

    const cleanup = () => {
      URL.revokeObjectURL(url)
      if (iframe.parentNode) document.body.removeChild(iframe)
    }

    let fallbackTimer: ReturnType<typeof setTimeout> | null = null

    iframe.onload = () => {
      fallbackTimer = setTimeout(() => {
        try {
          iframe.contentWindow?.print()
        } catch {
          cleanup()
          window.open(url, "_blank")
        }
      }, 500)
    }

    iframe.onerror = () => {
      cleanup()
      window.open(url, "_blank")
    }

    setTimeout(() => {
      if (fallbackTimer) clearTimeout(fallbackTimer)
      cleanup()
    }, 30000)
  }, [pdfBytes])

  const handlePrevPage = useCallback(() => {
    const next = Math.max(1, currentPage - 1)
    setCurrentPage(next)
    pdfRendererRef.current?.scrollToPage(next)
  }, [currentPage])

  const handleNextPage = useCallback(() => {
    const next = Math.min(numPages, currentPage + 1)
    setCurrentPage(next)
    pdfRendererRef.current?.scrollToPage(next)
  }, [currentPage, numPages])

  const handlePageChange = useCallback((pageNumber: number) => {
    setCurrentPage(pageNumber)
  }, [])

  const retry = () => { void loadPdf() }

  const handleLoadSuccess = ({ numPages }: PDFDocumentProxy) => {
    setNumPages(numPages)
  }

  return (
    <div className="flex h-full flex-col bg-[#0d0d0d]">
      {/* Toolbar */}
      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-[#373737] bg-[#121212] px-3">
        <span className="text-xs text-[#555] truncate max-w-48">
          <FileText className="mr-1 inline h-3.5 w-3.5 text-[#f44336]" />
          {filePath.split("/").pop()?.split("\\").pop()}
        </span>

        <div className="flex-1" />

        {numPages > 0 && (
          <>
            <button
              type="button"
              onClick={handlePrevPage}
              disabled={currentPage <= 1}
              className="rounded p-1 text-[#787878] hover:bg-[#232323] hover:text-[#e8e8e8] disabled:opacity-30 disabled:cursor-not-allowed"
              title="Previous page"
            >
              <ChevronLeft className="h-3.5 w-3.5" />
            </button>

            <span className="text-xs text-[#787878] whitespace-nowrap">
              {currentPage} / {numPages}
            </span>

            <button
              type="button"
              onClick={handleNextPage}
              disabled={currentPage >= numPages}
              className="rounded p-1 text-[#787878] hover:bg-[#232323] hover:text-[#e8e8e8] disabled:opacity-30 disabled:cursor-not-allowed"
              title="Next page"
            >
              <ChevronRight className="h-3.5 w-3.5" />
            </button>

            <span className="w-px h-4 bg-[#373737]" />
          </>
        )}

        <button
          type="button"
          onClick={handlePrint}
          disabled={!pdfBytes}
          className="rounded p-1 text-[#787878] hover:bg-[#232323] hover:text-[#e8e8e8] disabled:opacity-30 disabled:cursor-not-allowed"
          title="Print"
        >
          <Printer className="h-3.5 w-3.5" />
        </button>

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
            ref={pdfRendererRef}
            fileData={pdfBytes}
            numPages={numPages}
            onLoadSuccess={handleLoadSuccess}
            onLoadError={(e) => setError(e.message)}
            onVisiblePageChange={handlePageChange}
          />
        )}
      </div>
    </div>
  )
}
