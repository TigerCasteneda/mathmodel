"use client"

import { Suspense } from "react"
import { Loader2 } from "lucide-react"
import SearchPage from "@/components/search/search-page"

export default function SearchRoute() {
  return (
    <Suspense
      fallback={
        <div className="flex h-screen items-center justify-center bg-[#0d0d0d]">
          <Loader2 className="h-6 w-6 animate-spin text-[#d4a574]" />
        </div>
      }
    >
      <SearchPage />
    </Suspense>
  )
}
