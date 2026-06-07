"use client"

import { useMemo } from "react"
import { Loader2 } from "lucide-react"
import { Document, Page, pdfjs } from "react-pdf"
import type { PDFDocumentProxy } from "pdfjs-dist"

pdfjs.GlobalWorkerOptions.workerSrc = new URL(
  "pdfjs-dist/build/pdf.worker.min.mjs",
  import.meta.url,
).toString()

type PdfDocumentRendererProps = {
  fileData: Uint8Array
  numPages: number
  onLoadSuccess: (document: PDFDocumentProxy) => void
  onLoadError: (error: Error) => void
}

export default function PdfDocumentRenderer({
  fileData,
  numPages,
  onLoadSuccess,
  onLoadError,
}: PdfDocumentRendererProps) {
  const file = useMemo(() => ({ data: fileData }), [fileData])

  return (
    <Document
      file={file}
      onLoadSuccess={onLoadSuccess}
      onLoadError={onLoadError}
      loading={(
        <div className="flex h-full items-center justify-center gap-3 text-[#787878]">
          <Loader2 className="h-5 w-5 animate-spin text-[#d4a574]" />
          <span className="text-sm">Rendering PDF...</span>
        </div>
      )}
      error={(
        <div className="flex h-full items-center justify-center text-sm text-[#ffb4a8]">
          Failed to render PDF.
        </div>
      )}
      className="mx-auto flex max-w-full flex-col items-center gap-4 py-4"
    >
      {Array.from({ length: numPages }, (_, index) => (
        <Page
          key={`page-${index + 1}`}
          pageNumber={index + 1}
          width={960}
          renderAnnotationLayer={false}
          renderTextLayer={false}
          className="overflow-hidden rounded-sm bg-white shadow-[0_2px_18px_rgba(0,0,0,0.35)]"
        />
      ))}
    </Document>
  )
}
