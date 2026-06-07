"use client"

import { useEffect, useMemo, useState } from "react"
import { AlertCircle, FileImage, Loader2, Maximize2 } from "lucide-react"
import { readFileBase64 } from "@/lib/tauri-api"

type ImageViewerProps = {
  filePath: string
}

function imageMimeType(filePath: string) {
  const ext = filePath.split(/[./\\]/).pop()?.toLowerCase()
  if (ext === "jpg" || ext === "jpeg") return "image/jpeg"
  if (ext === "png") return "image/png"
  return "image/*"
}

export default function ImageViewer({ filePath }: ImageViewerProps) {
  const [dataUrl, setDataUrl] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const mimeType = useMemo(() => imageMimeType(filePath), [filePath])

  useEffect(() => {
    let cancelled = false

    async function load() {
      setLoading(true)
      setError(null)
      setDataUrl(null)
      try {
        const b64 = await readFileBase64(filePath)
        if (!cancelled) setDataUrl(`data:${mimeType};base64,${b64}`)
      } catch (e) {
        if (!cancelled) setError(String(e))
      } finally {
        if (!cancelled) setLoading(false)
      }
    }

    load()
    return () => {
      cancelled = true
    }
  }, [filePath, mimeType])

  const retry = () => {
    setLoading(true)
    setError(null)
    setDataUrl(null)
    readFileBase64(filePath)
      .then((b64) => setDataUrl(`data:${mimeType};base64,${b64}`))
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false))
  }

  return (
    <div className="flex h-full flex-col bg-[#0d0d0d]">
      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-[#373737] bg-[#121212] px-3">
        <div className="flex-1" />
        <span className="max-w-48 truncate text-xs text-[#555]">
          <FileImage className="mr-1 inline h-3.5 w-3.5 text-[#64b5f6]" />
          {filePath.split("/").pop()?.split("\\").pop()}
        </span>
        <button
          type="button"
          onClick={() => dataUrl && window.open(dataUrl, "_blank")}
          disabled={!dataUrl}
          className="rounded p-1 text-[#787878] hover:bg-[#232323] hover:text-[#e8e8e8] disabled:cursor-not-allowed disabled:opacity-30"
          title="Open image"
        >
          <Maximize2 className="h-3.5 w-3.5" />
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-auto bg-[#202020]">
        {loading && (
          <div className="flex h-full items-center justify-center gap-3 text-[#787878]">
            <Loader2 className="h-5 w-5 animate-spin text-[#d4a574]" />
            <span className="text-sm">Loading image...</span>
          </div>
        )}

        {error && (
          <div className="flex h-full items-center justify-center">
            <div className="flex flex-col items-center gap-3 rounded-lg border border-[#5f2424] bg-[#2d1a1a] px-6 py-4">
              <AlertCircle className="h-6 w-6 text-[#f44336]" />
              <p className="max-w-md text-center text-sm text-[#ffb4a8]">{error}</p>
              <button
                type="button"
                onClick={retry}
                className="rounded-md border border-[#5f2424] px-3 py-1 text-xs text-[#ffb4a8] transition-colors hover:bg-[#3d2424]"
              >
                Retry
              </button>
            </div>
          </div>
        )}

        {dataUrl && !loading && !error && (
          <div className="flex min-h-full items-center justify-center p-6">
            <img
              src={dataUrl}
              alt={filePath.split("/").pop()?.split("\\").pop() ?? "Preview"}
              className="max-h-full max-w-full object-contain"
            />
          </div>
        )}
      </div>
    </div>
  )
}
