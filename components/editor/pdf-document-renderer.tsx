"use client"

import { useEffect, useMemo, useRef, forwardRef, useImperativeHandle } from "react"
import { Loader2 } from "lucide-react"
import { Document, Page, pdfjs } from "react-pdf"
import type { PDFDocumentProxy } from "pdfjs-dist"

pdfjs.GlobalWorkerOptions.workerSrc = new URL(
  "pdfjs-dist/build/pdf.worker.min.mjs",
  import.meta.url,
).toString()

export type PdfDocumentRendererHandle = {
  scrollToPage: (pageNumber: number) => void
}

type PdfDocumentRendererProps = {
  fileData: Uint8Array
  numPages: number
  onLoadSuccess: (document: PDFDocumentProxy) => void
  onLoadError: (error: Error) => void
  onVisiblePageChange: (pageNumber: number) => void
}

const PdfDocumentRenderer = forwardRef<PdfDocumentRendererHandle, PdfDocumentRendererProps>(
  function PdfDocumentRenderer(
    { fileData, numPages, onLoadSuccess, onLoadError, onVisiblePageChange },
    ref,
  ) {
    const file = useMemo(() => ({ data: fileData }), [fileData])
    const containerRef = useRef<HTMLDivElement>(null)
    const onVisiblePageChangeRef = useRef(onVisiblePageChange)
    onVisiblePageChangeRef.current = onVisiblePageChange

    useEffect(() => {
      const container = containerRef.current
      if (!container || numPages === 0) return

      const observer = new IntersectionObserver(
        (entries) => {
          let maxRatio = 0
          let currentPage = 0
          for (const entry of entries) {
            const pageNum = Number((entry.target as HTMLElement).dataset.pageNumber)
            if (entry.intersectionRatio > maxRatio) {
              maxRatio = entry.intersectionRatio
              currentPage = pageNum
            }
          }
          if (currentPage > 0) {
            onVisiblePageChangeRef.current(currentPage)
          }
        },
        { threshold: [0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0] },
      )

      const mutationObserver = new MutationObserver(() => {
        const pages = container.querySelectorAll<HTMLElement>("[data-page-number]")
        pages.forEach((el) => {
          if (!el.dataset.pdObserved) {
            observer.observe(el)
            el.dataset.pdObserved = "true"
          }
        })
      })

      mutationObserver.observe(container, { childList: true, subtree: true })

      return () => {
        observer.disconnect()
        mutationObserver.disconnect()
      }
    }, [numPages])

    useImperativeHandle(ref, () => ({
      scrollToPage(pageNumber: number) {
        const container = containerRef.current
        if (!container) return
        const el = container.querySelector<HTMLElement>(
          `[data-page-number="${pageNumber}"]`,
        )
        el?.scrollIntoView({ behavior: "smooth", block: "start" })
      },
    }))

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
        <div ref={containerRef}>
          {Array.from({ length: numPages }, (_, index) => (
            <div key={`page-wrapper-${index + 1}`} data-page-number={index + 1}>
              <Page
                pageNumber={index + 1}
                width={960}
                renderAnnotationLayer={false}
                renderTextLayer={false}
                className="overflow-hidden rounded-sm bg-white shadow-[0_2px_18px_rgba(0,0,0,0.35)]"
              />
            </div>
          ))}
        </div>
      </Document>
    )
  },
)

export default PdfDocumentRenderer
