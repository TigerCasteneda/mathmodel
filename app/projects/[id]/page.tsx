"use client"

import { useParams } from "next/navigation"
import { Sidebar } from "@/components/dashboard/sidebar"
import { MainWorkspace } from "@/components/dashboard/main-workspace"
import { CodeCanvas } from "@/components/dashboard/code-canvas"
import { useState } from "react"

export default function ProjectPage() {
  const { id } = useParams<{ id: string }>()
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)

  return (
    <main className="flex h-screen overflow-hidden bg-background">
      <Sidebar
        collapsed={sidebarCollapsed}
        onToggle={() => setSidebarCollapsed(!sidebarCollapsed)}
      />
      <div className="flex flex-1 min-w-0">
        <div className="flex-1 min-w-0">
          <MainWorkspace />
        </div>
        <div className="w-[480px] hidden lg:block">
          <CodeCanvas />
        </div>
      </div>
    </main>
  )
}
